//! Incremental replication protocol (Phase 4).
//!
//! This module turns the single round-trip peering handshake (Phase 3) into a
//! bidirectional, idempotent operation exchange between two already-enrolled
//! nodes.
//!
//! The exchange for each authorized dataset is (repeated as needed until both
//! sides are caught up, so a single session can bootstrap a peer that is
//! arbitrarily far behind):
//!
//! ```text
//! initiator                  responder
//!   |  Have { vector }          |
//!   |-------------------------->|
//!   |      Ops { ops, vec }     |
//!   |<--------------------------|   (ops the initiator is missing)
//!   |  ...repeat Have/Ops...     |   (until the initiator is caught up)
//!   |  Ops { ops, vec }          |
//!   |-------------------------->|   (ops the responder is missing)
//!   |    Ack { applied, vec }    |
//!   |<--------------------------|
//!   |  ...repeat Ops/Ack...       |   (until the responder is caught up)
//! ```
//!
//! A *vector* is the highest `origin_seq` seen per originating node for a
//! dataset, so each side only transfers the operations the other lacks. Every
//! operation is applied through [`storage::operations::apply_remote_operation`],
//! which deduplicates by operation id and advances the origin node's
//! checkpoint atomically — so at-least-once delivery is safe and interrupted
//! transfers resume from the last checkpoint.
//!
//! Writes are permission-checked: a peer may only push operations into a
//! dataset it holds write (bit 2) authorization for; otherwise the operations
//! are dropped and reported as `applied: 0`.

use crate::error::{ReplicationError, Result, SyncFleetError};
use crate::net::{read_message, write_message};
use crate::storage::operations::{self, ApplyOutcome, Operation, OperationType};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite};

/// Default upper bound on operations exchanged per `Ops` message.
pub const DEFAULT_MAX_OPS_PER_RESPONSE: usize = 1000;

/// Safety cap on the number of batch round-trips per dataset in a single
/// session. With the default batch size this allows ~1M operations per
/// session, which is far beyond any expected single-dataset backlog.
pub const MAX_SYNC_ROUNDS: usize = 1000;

/// Read (bit 1) permission required to receive operations.
pub const PERM_READ: i32 = 1;
/// Write (bit 2) permission required to push operations.
pub const PERM_WRITE: i32 = 2;

/// Wire messages exchanged after the peering `Hello`/`HelloReply`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncMsg {
    /// Initiator's per-origin high-water marks for a dataset.
    Have {
        dataset_id: String,
        vector: BTreeMap<String, u64>,
    },
    /// Operations the sender believes the receiver lacks, plus the sender's
    /// own updated vector for the dataset.
    Ops {
        dataset_id: String,
        operations: Vec<Operation>,
        vector: BTreeMap<String, u64>,
    },
    /// Acknowledgement of how many pushed operations were newly applied, plus
    /// the responder's vector after applying (so the initiator knows whether
    /// more of its operations still need to be sent).
    Ack {
        dataset_id: String,
        applied: usize,
        vector: BTreeMap<String, u64>,
    },
    /// Initiator finished all datasets; the connection can close cleanly.
    Done,
}

/// Summary of a completed replication pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Operations received from the peer and newly applied locally.
    pub pulled: usize,
    /// Operations we sent that the peer reported as newly applied.
    pub pushed: usize,
    /// Number of datasets synchronised.
    pub datasets: usize,
}

/// Highest `origin_seq` per originating node for a dataset, read from the
/// local journal (and any locally-allocated sequence state).
pub async fn local_vector(pool: &SqlitePool, dataset_id: &str) -> Result<BTreeMap<String, u64>> {
    let rows = sqlx::query(
        "SELECT origin_node_id, MAX(origin_seq) AS max_seq FROM operation_journal \
         WHERE dataset_id = ? GROUP BY origin_node_id",
    )
    .bind(dataset_id)
    .fetch_all(pool)
    .await?;

    use sqlx::Row;
    let mut vector = BTreeMap::new();
    for row in rows {
        let origin: String = row.try_get("origin_node_id")?;
        let seq: i64 = row.try_get("max_seq")?;
        vector.insert(origin, seq as u64);
    }
    Ok(vector)
}

/// Operations held locally for `dataset_id` that `remote` has not seen, capped
/// at `limit` in total and ordered per origin by ascending sequence.
pub async fn missing_operations(
    pool: &SqlitePool,
    dataset_id: &str,
    remote: &BTreeMap<String, u64>,
    limit: usize,
) -> Result<Vec<Operation>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let ours = local_vector(pool, dataset_id).await?;
    let mut out: Vec<Operation> = Vec::new();

    for (origin, our_max) in &ours {
        let since = remote.get(origin).copied().unwrap_or(0);
        if since >= *our_max {
            continue;
        }
        let remaining = limit.saturating_sub(out.len());
        if remaining == 0 {
            break;
        }
        let mut ops =
            operations::get_operations_since(pool, dataset_id, origin, since, remaining).await?;
        out.append(&mut ops);
    }

    // Deterministic ordering by (origin_seq, origin) so both sides agree.
    out.sort_by(|a, b| {
        a.origin_seq
            .cmp(&b.origin_seq)
            .then_with(|| a.origin_node_id.cmp(&b.origin_node_id))
    });
    out.truncate(limit);
    Ok(out)
}

/// Whether `theirs` references any operation that `ours` has not yet recorded
/// (i.e. `ours` is still behind and should keep pulling).
fn vector_lags(ours: &BTreeMap<String, u64>, theirs: &BTreeMap<String, u64>) -> bool {
    theirs
        .iter()
        .any(|(origin, &their_seq)| ours.get(origin).copied().unwrap_or(0) < their_seq)
}

/// Apply a batch of remote operations, returning how many were newly applied.
pub async fn apply_batch(pool: &SqlitePool, ops: &[Operation]) -> Result<usize> {
    let mut applied = 0usize;
    for op in ops {
        if operations::apply_remote_operation(pool, op).await? == ApplyOutcome::Applied {
            applied += 1;
        }
    }
    Ok(applied)
}

/// Serve the responder side of the exchange on an established stream.
///
/// Returns the number of operations applied locally on behalf of the peer.
/// `authorized` is the set of datasets the peer may read; `peer_node_id` is the
/// authenticated node id, used to enforce write permission on pushed ops.
pub async fn serve_sync<S>(
    stream: &mut S,
    pool: &SqlitePool,
    peer_node_id: &str,
    authorized: &[String],
    max_ops_per_response: usize,
    max_message_size: usize,
    idle_timeout: Duration,
) -> Result<usize>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut applied_total = 0usize;

    loop {
        let raw = match tokio::time::timeout(idle_timeout, read_message(stream, max_message_size))
            .await
        {
            // Client hung up or timed out without a follow-up: a clean end.
            Err(_) => break,
            Ok(Ok(bytes)) => bytes,
            Ok(Err(SyncFleetError::Transport(crate::error::TransportError::Disconnected))) => break,
            Ok(Err(e)) => return Err(e),
        };

        let msg: SyncMsg = serde_json::from_slice(&raw)?;
        match msg {
            SyncMsg::Have { dataset_id, vector } => {
                ensure_authorized(authorized, &dataset_id, "read")?;
                let ops =
                    missing_operations(pool, &dataset_id, &vector, max_ops_per_response).await?;
                let ours = local_vector(pool, &dataset_id).await?;
                write_msg(
                    stream,
                    &SyncMsg::Ops {
                        dataset_id,
                        operations: ops,
                        vector: ours,
                    },
                    max_message_size,
                )
                .await?;
            }
            SyncMsg::Ops {
                dataset_id,
                operations,
                ..
            } => {
                ensure_authorized(authorized, &dataset_id, "read")?;
                // Write authorization is required to push into the dataset;
                // a read-only peer may still pull, so we report zero applied
                // rather than failing the whole exchange.
                let can_write = crate::storage::peers::is_authorized(
                    pool,
                    peer_node_id,
                    &dataset_id,
                    PERM_WRITE,
                )
                .await?;
                let applied = if can_write {
                    apply_batch(pool, &operations).await?
                } else {
                    if !operations.is_empty() {
                        tracing::warn!(
                            peer = %peer_node_id,
                            dataset = %dataset_id,
                            "dropping {} pushed operations: no write authorization",
                            operations.len()
                        );
                    }
                    0
                };
                applied_total += applied;
                let ours = local_vector(pool, &dataset_id).await?;
                write_msg(
                    stream,
                    &SyncMsg::Ack {
                        dataset_id,
                        applied,
                        vector: ours,
                    },
                    max_message_size,
                )
                .await?;
            }
            SyncMsg::Done => break,
            SyncMsg::Ack { .. } => {
                return Err(ReplicationError::Protocol(
                    "unexpected Ack from initiator".to_string(),
                )
                .into());
            }
        }
    }

    Ok(applied_total)
}

/// Run the initiator side of the exchange for each dataset in `datasets`.
///
/// `datasets` should be the intersection of the local dataset set and the
/// peer's advertised authorized datasets (typically from the `HelloReply`).
pub async fn client_sync<S>(
    stream: &mut S,
    pool: &SqlitePool,
    datasets: &[String],
    max_ops_per_response: usize,
    max_message_size: usize,
    idle_timeout: Duration,
) -> Result<SyncReport>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let mut report = SyncReport::default();

    for dataset_id in datasets {
        // Phase A — pull: repeatedly advertise our vector and apply the batch
        // the responder returns, until we are caught up (or the peer is out of
        // batches for us).
        let mut their_vector;
        let mut rounds = 0;
        loop {
            let our_vector = local_vector(pool, dataset_id).await?;
            write_msg(
                stream,
                &SyncMsg::Have {
                    dataset_id: dataset_id.clone(),
                    vector: our_vector,
                },
                max_message_size,
            )
            .await?;

            let raw = tokio::time::timeout(idle_timeout, read_message(stream, max_message_size))
                .await
                .map_err(|_| SyncFleetError::Transport(crate::error::TransportError::Timeout))??;
            let (their_ops, vec) = match serde_json::from_slice::<SyncMsg>(&raw)? {
                SyncMsg::Ops {
                    dataset_id: ds,
                    operations,
                    vector,
                } if &ds == dataset_id => (operations, vector),
                other => {
                    return Err(ReplicationError::Protocol(format!(
                        "expected Ops for {dataset_id}, got {other:?}"
                    ))
                    .into());
                }
            };

            let batch_len = their_ops.len();
            report.pulled += apply_batch(pool, &their_ops).await?;
            their_vector = vec;

            let after = local_vector(pool, dataset_id).await?;
            if !vector_lags(&after, &their_vector) || batch_len < max_ops_per_response {
                break;
            }
            rounds += 1;
            if rounds >= MAX_SYNC_ROUNDS {
                tracing::warn!(
                    dataset = %dataset_id,
                    "pull round cap reached; peer still has more operations"
                );
                break;
            }
        }

        // Phase B — push: send the responder the operations it lacks, looping
        // while we still have a full batch to hand over and it keeps accepting.
        let mut rounds = 0;
        loop {
            let our_vector = local_vector(pool, dataset_id).await?;
            let to_send =
                missing_operations(pool, dataset_id, &their_vector, max_ops_per_response).await?;
            let sent_len = to_send.len();
            if sent_len == 0 {
                break;
            }

            write_msg(
                stream,
                &SyncMsg::Ops {
                    dataset_id: dataset_id.clone(),
                    operations: to_send,
                    vector: our_vector,
                },
                max_message_size,
            )
            .await?;

            let raw = tokio::time::timeout(idle_timeout, read_message(stream, max_message_size))
                .await
                .map_err(|_| SyncFleetError::Transport(crate::error::TransportError::Timeout))??;
            let (applied, vec) = match serde_json::from_slice::<SyncMsg>(&raw)? {
                SyncMsg::Ack {
                    dataset_id: ds,
                    applied,
                    vector,
                } if &ds == dataset_id => (applied, vector),
                other => {
                    return Err(ReplicationError::Protocol(format!(
                        "expected Ack for {dataset_id}, got {other:?}"
                    ))
                    .into());
                }
            };
            report.pushed += applied;
            their_vector = vec;

            // A non-empty batch that applied nothing means the peer has no
            // write authorization (or already had these ops): stop pushing.
            if applied == 0 || sent_len < max_ops_per_response {
                break;
            }
            let after = local_vector(pool, dataset_id).await?;
            if !vector_lags(&their_vector, &after) {
                break;
            }
            rounds += 1;
            if rounds >= MAX_SYNC_ROUNDS {
                tracing::warn!(
                    dataset = %dataset_id,
                    "push round cap reached; more operations remain to send"
                );
                break;
            }
        }

        report.datasets += 1;
    }

    // Signal a clean finish.
    write_msg(stream, &SyncMsg::Done, max_message_size).await?;
    Ok(report)
}

fn ensure_authorized(authorized: &[String], dataset_id: &str, verb: &str) -> Result<()> {
    if authorized.iter().any(|d| d == dataset_id) {
        Ok(())
    } else {
        Err(ReplicationError::PeerNotAuthorized(format!(
            "{verb} access to dataset '{dataset_id}' not authorized"
        ))
        .into())
    }
}

async fn write_msg<S>(stream: &mut S, msg: &SyncMsg, _max: usize) -> Result<()>
where
    S: AsyncWrite + Unpin + Send,
{
    let bytes = serde_json::to_vec(msg)?;
    write_message(stream, &bytes).await
}

/// Build a locally-originated operation payload for a test/CLI write.
pub fn encode_payload(fields: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(fields).unwrap_or_default()
}

/// Convenience wrapper used by the CLI's `dataset apply` command.
pub async fn local_write(
    pool: &SqlitePool,
    origin_node_id: &str,
    dataset_id: &str,
    entity_id: &str,
    op_type: OperationType,
    schema_version: u32,
    payload: Vec<u8>,
) -> Result<Operation> {
    operations::append_local_operation(
        pool,
        operations::LocalOperation {
            dataset_id: dataset_id.to_string(),
            entity_id: entity_id.to_string(),
            op_type,
            schema_version,
            payload,
            origin_node_id: origin_node_id.to_string(),
        },
    )
    .await
}

/// Parse an op-type string (`insert`, `update`, `delete`).
pub fn parse_op_type(raw: &str) -> Result<OperationType> {
    match raw.to_ascii_lowercase().as_str() {
        "insert" => Ok(OperationType::Insert),
        "update" => Ok(OperationType::Update),
        "delete" => Ok(OperationType::Delete),
        other => Err(SyncFleetError::InvalidArgument(format!(
            "unknown op type '{other}' (expected insert|update|delete)"
        ))),
    }
}
