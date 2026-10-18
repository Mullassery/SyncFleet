use crate::error::{Result, SyncFleetError};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Row, SqlitePool,
};
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

/// Options controlling how the local database is opened.
#[derive(Debug, Clone)]
pub struct OpenOptions {
    /// SQLite busy timeout in milliseconds.
    pub busy_timeout_ms: u32,
    /// Run `PRAGMA integrity_check` on startup and fail if it does not pass.
    pub integrity_check_on_startup: bool,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self {
            busy_timeout_ms: 5_000,
            integrity_check_on_startup: false,
        }
    }
}

pub struct Storage {
    pool: SqlitePool,
}

impl Storage {
    /// Open with default options (no startup integrity check).
    pub async fn new(data_dir: &Path) -> Result<Self> {
        Self::open(data_dir, &OpenOptions::default()).await
    }

    /// Open the local database, applying durability pragmas and optionally
    /// performing a crash-recovery integrity check before returning.
    pub async fn open(data_dir: &Path, options: &OpenOptions) -> Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db_path = data_dir.join("syncfleet.db");

        // Durability and integrity settings are applied to *every* pooled
        // connection via SqliteConnectOptions (PRAGMAs executed as statements
        // would only affect a single connection). WAL keeps readers
        // non-blocking and gives crash-recovery semantics; foreign_keys
        // enforces referential integrity for the journal and checkpoints.
        let connect_options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_millis(options.busy_timeout_ms as u64));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(connect_options)
            .await?;

        let storage = Self { pool };
        storage.migrate().await?;

        if options.integrity_check_on_startup {
            let status = storage.integrity_check().await?;
            if status != "ok" {
                return Err(SyncFleetError::Storage(
                    crate::error::StorageError::IntegrityCheckFailed(status),
                ));
            }
        }

        Ok(storage)
    }

    async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn health_check(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    /// Run `PRAGMA integrity_check`, returning the status string (`ok` on success).
    pub async fn integrity_check(&self) -> Result<String> {
        let status: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&self.pool)
            .await?;
        Ok(status)
    }

    /// Flush the write-ahead log back into the main database file.
    pub async fn checkpoint_wal(&self) -> Result<()> {
        // TRUNCATE keeps the WAL small after a clean checkpoint.
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .fetch_optional(&self.pool)
            .await?;
        Ok(())
    }
}

/// Persist (or refresh) the node identity row after `syncfleet init`.
pub async fn record_node_identity(
    pool: &SqlitePool,
    node_id: &str,
    public_key: &[u8],
    private_key_encrypted: &[u8],
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO node_identity (node_id, public_key, private_key_encrypted, created_at)
        VALUES (?, ?, ?, ?)
        ON CONFLICT(node_id) DO UPDATE SET
            public_key = excluded.public_key,
            private_key_encrypted = excluded.private_key_encrypted
        "#,
    )
    .bind(node_id)
    .bind(public_key)
    .bind(private_key_encrypted)
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

pub mod operations {
    use super::*;
    use crate::error::ReplicationError;
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Operation {
        pub id: Uuid,
        pub dataset_id: String,
        pub entity_id: String,
        pub origin_node_id: String,
        pub origin_seq: u64,
        pub op_type: OperationType,
        pub schema_version: u32,
        pub payload: Vec<u8>,
        pub causal_metadata: VersionVector,
        pub created_at: DateTime<Utc>,
        pub checksum: [u8; 32],
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
    #[repr(i32)]
    pub enum OperationType {
        Insert = 1,
        Update = 2,
        Delete = 3,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, Default)]
    pub struct VersionVector {
        pub versions: HashMap<String, u64>,
    }

    impl VersionVector {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn increment(&mut self, node_id: &str) {
            *self.versions.entry(node_id.to_string()).or_insert(0) += 1;
        }

        pub fn get(&self, node_id: &str) -> u64 {
            self.versions.get(node_id).copied().unwrap_or(0)
        }

        pub fn happens_before(&self, other: &VersionVector) -> bool {
            let mut dominated = false;
            for (node, &ver) in &self.versions {
                let other_ver = other.versions.get(node).copied().unwrap_or(0);
                if ver > other_ver {
                    return false;
                }
                if ver < other_ver {
                    dominated = true;
                }
            }
            dominated
        }
    }

    pub async fn append_operation(pool: &SqlitePool, op: &Operation) -> Result<()> {
        let causal_json = serde_json::to_string(&op.causal_metadata)?;

        sqlx::query(
            r#"
            INSERT INTO operation_journal (id, dataset_id, entity_id, origin_node_id, origin_seq, op_type, schema_version, payload, causal_metadata, created_at, checksum)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(op.id.to_string())
        .bind(&op.dataset_id)
        .bind(&op.entity_id)
        .bind(&op.origin_node_id)
        .bind(op.origin_seq as i64)
        .bind(op.op_type as i32)
        .bind(op.schema_version as i32)
        .bind(&op.payload)
        .bind(causal_json)
        .bind(op.created_at.to_rfc3339())
        .bind(&op.checksum[..])
        .execute(pool)
        .await?;

        Ok(())
    }

    /// SHA-256 checksum over an operation payload, used to detect corruption
    /// in transit and at rest.
    pub fn compute_checksum(payload: &[u8]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(payload);
        let digest = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        out
    }

    /// Verify that an operation's stored checksum matches its payload.
    pub fn verify_checksum(op: &Operation) -> bool {
        compute_checksum(&op.payload) == op.checksum
    }

    /// Parameters for appending a locally-originated operation.
    #[derive(Debug, Clone)]
    pub struct LocalOperation {
        pub dataset_id: String,
        pub entity_id: String,
        pub op_type: OperationType,
        pub schema_version: u32,
        pub payload: Vec<u8>,
        pub origin_node_id: String,
    }

    /// Atomically allocate the next per-node sequence number, build the
    /// operation (id, checksum, version vector), and append it to the journal
    /// in a single transaction. If any step fails the sequence number is not
    /// consumed, so the journal stays gap-free per origin node.
    pub async fn append_local_operation(
        pool: &SqlitePool,
        local: LocalOperation,
    ) -> Result<Operation> {
        let mut tx = pool.begin().await?;

        let now = Utc::now();
        let origin_seq: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO node_local_state (node_id, last_seq, updated_at)
            VALUES (?, 1, ?)
            ON CONFLICT(node_id) DO UPDATE SET
                last_seq = node_local_state.last_seq + 1,
                updated_at = excluded.updated_at
            RETURNING last_seq
            "#,
        )
        .bind(&local.origin_node_id)
        .bind(now.to_rfc3339())
        .fetch_one(&mut *tx)
        .await?;

        let mut causal_metadata = VersionVector::new();
        causal_metadata.increment(&local.origin_node_id);

        let op = Operation {
            id: Uuid::now_v7(),
            dataset_id: local.dataset_id,
            entity_id: local.entity_id,
            origin_node_id: local.origin_node_id,
            origin_seq: origin_seq as u64,
            op_type: local.op_type,
            schema_version: local.schema_version,
            checksum: compute_checksum(&local.payload),
            payload: local.payload,
            causal_metadata,
            created_at: now,
        };

        let causal_json = serde_json::to_string(&op.causal_metadata)?;
        sqlx::query(
            r#"
            INSERT INTO operation_journal (id, dataset_id, entity_id, origin_node_id, origin_seq, op_type, schema_version, payload, causal_metadata, created_at, checksum)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(op.id.to_string())
        .bind(&op.dataset_id)
        .bind(&op.entity_id)
        .bind(&op.origin_node_id)
        .bind(op.origin_seq as i64)
        .bind(op.op_type as i32)
        .bind(op.schema_version as i32)
        .bind(&op.payload)
        .bind(causal_json)
        .bind(op.created_at.to_rfc3339())
        .bind(&op.checksum[..])
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(op)
    }

    /// Outcome of [`apply_remote_operation`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ApplyOutcome {
        /// The operation was newly applied.
        Applied,
        /// The operation had already been applied; the call was a no-op.
        Duplicate,
    }

    /// Apply an operation received from a peer.
    ///
    /// Validates the payload checksum, then — in a single transaction —
    /// deduplicates by operation id (via `applied_operations`), stores the
    /// operation in the journal, records it as applied, and advances the
    /// checkpoint for the operation's origin node up to `origin_seq`.
    ///
    /// Re-applying the same operation id is an idempotent no-op returning
    /// [`ApplyOutcome::Duplicate`], so at-least-once delivery from a peer
    /// cannot double-apply an operation.
    ///
    /// The dataset must already be registered locally (the journal and
    /// checkpoints reference `datasets(dataset_id)`), otherwise the foreign
    /// key constraint fails.
    pub async fn apply_remote_operation(pool: &SqlitePool, op: &Operation) -> Result<ApplyOutcome> {
        if !verify_checksum(op) {
            return Err(SyncFleetError::Replication(
                ReplicationError::ValidationFailed(format!(
                    "checksum mismatch for operation {}",
                    op.id
                )),
            ));
        }

        let mut tx = pool.begin().await?;

        // Dedup first: applied_operations.operation_id is the primary key, so
        // a zero row count means we have already applied this operation.
        let marked = sqlx::query(
            "INSERT OR IGNORE INTO applied_operations (operation_id, applied_at, local_seq) VALUES (?, ?, ?)",
        )
        .bind(op.id.to_string())
        .bind(Utc::now().to_rfc3339())
        .bind(op.origin_seq as i64)
        .execute(&mut *tx)
        .await?;

        if marked.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(ApplyOutcome::Duplicate);
        }

        let causal_json = serde_json::to_string(&op.causal_metadata)?;
        // OR IGNORE guards against re-storing a row we already have (for
        // example an operation this node originated and is now echoed back).
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO operation_journal (id, dataset_id, entity_id, origin_node_id, origin_seq, op_type, schema_version, payload, causal_metadata, created_at, checksum)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(op.id.to_string())
        .bind(&op.dataset_id)
        .bind(&op.entity_id)
        .bind(&op.origin_node_id)
        .bind(op.origin_seq as i64)
        .bind(op.op_type as i32)
        .bind(op.schema_version as i32)
        .bind(&op.payload)
        .bind(causal_json)
        .bind(op.created_at.to_rfc3339())
        .bind(&op.checksum[..])
        .execute(&mut *tx)
        .await?;

        // Advance the checkpoint monotonically: never move it backwards if an
        // older operation arrives after a newer one.
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT last_applied_seq FROM replication_checkpoints WHERE peer_id = ? AND dataset_id = ?",
        )
        .bind(&op.origin_node_id)
        .bind(&op.dataset_id)
        .fetch_optional(&mut *tx)
        .await?;

        let last_applied = existing.unwrap_or(0).max(op.origin_seq as i64);

        sqlx::query(
            r#"
            INSERT INTO replication_checkpoints (peer_id, dataset_id, last_acked_seq, last_applied_seq, updated_at)
            VALUES (?, ?, 0, ?, ?)
            ON CONFLICT(peer_id, dataset_id) DO UPDATE SET
                last_applied_seq = excluded.last_applied_seq,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&op.origin_node_id)
        .bind(&op.dataset_id)
        .bind(last_applied)
        .bind(Utc::now().to_rfc3339())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(ApplyOutcome::Applied)
    }

    /// Current sequence allocation state for a node (0 if it has never written).
    pub async fn last_local_seq(pool: &SqlitePool, node_id: &str) -> Result<u64> {
        let seq: Option<i64> =
            sqlx::query_scalar("SELECT last_seq FROM node_local_state WHERE node_id = ?")
                .bind(node_id)
                .fetch_optional(pool)
                .await?;
        Ok(seq.unwrap_or(0) as u64)
    }

    #[derive(Debug, Clone)]
    pub struct JournalStats {
        pub operations: i64,
        pub applied_operations: i64,
        pub oldest: Option<DateTime<Utc>>,
        pub newest: Option<DateTime<Utc>>,
    }

    /// Summarise journal contents for diagnostics and retention planning.
    pub async fn journal_stats(pool: &SqlitePool) -> Result<JournalStats> {
        let operations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM operation_journal")
            .fetch_one(pool)
            .await?;
        let applied_operations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM applied_operations")
            .fetch_one(pool)
            .await?;
        let oldest: Option<String> =
            sqlx::query_scalar("SELECT MIN(created_at) FROM operation_journal")
                .fetch_one(pool)
                .await?;
        let newest: Option<String> =
            sqlx::query_scalar("SELECT MAX(created_at) FROM operation_journal")
                .fetch_one(pool)
                .await?;

        let parse = |s: Option<String>| -> Result<Option<DateTime<Utc>>> {
            match s {
                Some(v) => Ok(Some(DateTime::parse_from_rfc3339(&v)?.with_timezone(&Utc))),
                None => Ok(None),
            }
        };

        Ok(JournalStats {
            operations,
            applied_operations,
            oldest: parse(oldest)?,
            newest: parse(newest)?,
        })
    }

    /// Identify journal entries older than `cutoff` that are safe to discard.
    ///
    /// An entry is safe when every active peer authorized for its dataset has a
    /// checkpoint whose `last_acked_seq` covers the entry's `origin_seq`. Peers
    /// without a covering checkpoint (including datasets with active peers but
    /// no checkpoint) protect their entries from deletion.
    pub async fn plan_compaction(
        pool: &SqlitePool,
        cutoff: DateTime<Utc>,
        protected: &[(String, String)],
    ) -> Result<Vec<String>> {
        // Load checkpoints once: (peer, dataset) -> last_acked_seq.
        let checkpoint_rows =
            sqlx::query("SELECT peer_id, dataset_id, last_acked_seq FROM replication_checkpoints")
                .fetch_all(pool)
                .await?;
        let mut acked: HashMap<(String, String), u64> = HashMap::new();
        for row in checkpoint_rows {
            let peer: String = row.try_get("peer_id")?;
            let dataset: String = row.try_get("dataset_id")?;
            let seq: i64 = row.try_get("last_acked_seq")?;
            acked.insert((peer, dataset), seq as u64);
        }

        let candidates = sqlx::query(
            "SELECT id, dataset_id, origin_node_id, origin_seq FROM operation_journal WHERE created_at < ?",
        )
        .bind(cutoff.to_rfc3339())
        .fetch_all(pool)
        .await?;

        let mut to_delete: Vec<String> = Vec::new();
        for row in candidates {
            let dataset: String = row.try_get("dataset_id")?;
            let origin_node: String = row.try_get("origin_node_id")?;
            let origin_seq: u64 = row.try_get::<i64, _>("origin_seq")? as u64;

            // Every active peer on this dataset must have acknowledged past it.
            let mut all_acked = true;
            for (peer, peer_dataset) in protected {
                if peer_dataset != &dataset {
                    continue;
                }
                // A peer does not need to acknowledge its own operations.
                if peer == &origin_node {
                    continue;
                }
                let covered = acked
                    .get(&(peer.clone(), dataset.clone()))
                    .copied()
                    .unwrap_or(0)
                    >= origin_seq;
                if !covered {
                    all_acked = false;
                    break;
                }
            }

            if all_acked {
                to_delete.push(row.try_get::<String, _>("id")?);
            }
        }

        Ok(to_delete)
    }

    /// Delete the journal entries identified by [`plan_compaction`], together
    /// with their `applied_operations` dedup rows, in a single transaction.
    ///
    /// Returns the number of journal entries deleted.
    pub async fn compact_journal(
        pool: &SqlitePool,
        cutoff: DateTime<Utc>,
        protected: &[(String, String)],
    ) -> Result<u64> {
        let to_delete = plan_compaction(pool, cutoff, protected).await?;

        if to_delete.is_empty() {
            return Ok(0);
        }

        let mut tx = pool.begin().await?;
        for id in &to_delete {
            sqlx::query("DELETE FROM operation_journal WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM applied_operations WHERE operation_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;

        Ok(to_delete.len() as u64)
    }

    pub async fn get_operations_since(
        pool: &SqlitePool,
        dataset_id: &str,
        origin_node_id: &str,
        since_seq: u64,
        limit: usize,
    ) -> Result<Vec<Operation>> {
        let rows = sqlx::query(
            r#"
            SELECT id, dataset_id, entity_id, origin_node_id, origin_seq, op_type, schema_version, payload, causal_metadata, created_at, checksum
            FROM operation_journal
            WHERE dataset_id = ? AND origin_node_id = ? AND origin_seq > ?
            ORDER BY origin_seq ASC
            LIMIT ?
            "#,
        )
        .bind(dataset_id)
        .bind(origin_node_id)
        .bind(since_seq as i64)
        .bind(limit as i64)
        .fetch_all(pool)
        .await?;

        let mut ops = Vec::new();
        for row in rows {
            let causal_json: String = row.try_get("causal_metadata")?;
            let causal_metadata: VersionVector = serde_json::from_str(&causal_json)?;

            let checksum_bytes: Vec<u8> = row.try_get("checksum")?;
            let mut checksum = [0u8; 32];
            checksum.copy_from_slice(&checksum_bytes);

            ops.push(Operation {
                id: Uuid::parse_str(&row.try_get::<String, _>("id")?)?,
                dataset_id: row.try_get("dataset_id")?,
                entity_id: row.try_get("entity_id")?,
                origin_node_id: row.try_get("origin_node_id")?,
                origin_seq: row.try_get::<i64, _>("origin_seq")? as u64,
                op_type: match row.try_get::<i32, _>("op_type")? {
                    1 => OperationType::Insert,
                    2 => OperationType::Update,
                    3 => OperationType::Delete,
                    _ => return Err(SyncFleetError::Internal("Invalid op_type".to_string())),
                },
                schema_version: row.try_get::<i32, _>("schema_version")? as u32,
                payload: row.try_get("payload")?,
                causal_metadata,
                created_at: DateTime::parse_from_rfc3339(&row.try_get::<String, _>("created_at")?)?
                    .with_timezone(&Utc),
                checksum,
            });
        }

        Ok(ops)
    }

    pub async fn mark_applied(pool: &SqlitePool, operation_id: Uuid, local_seq: u64) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO applied_operations (operation_id, applied_at, local_seq) VALUES (?, ?, ?)"
        )
        .bind(operation_id.to_string())
        .bind(Utc::now().to_rfc3339())
        .bind(local_seq as i64)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn is_applied(pool: &SqlitePool, operation_id: Uuid) -> Result<bool> {
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM applied_operations WHERE operation_id = ?")
                .bind(operation_id.to_string())
                .fetch_one(pool)
                .await?;

        Ok(count > 0)
    }
}

pub mod checkpoints {
    use super::*;
    use chrono::{DateTime, Utc};

    #[derive(Debug, Clone)]
    pub struct Checkpoint {
        pub peer_id: String,
        pub dataset_id: String,
        pub last_acked_seq: u64,
        pub last_applied_seq: u64,
        pub updated_at: DateTime<Utc>,
    }

    pub async fn get_checkpoint(
        pool: &SqlitePool,
        peer_id: &str,
        dataset_id: &str,
    ) -> Result<Option<Checkpoint>> {
        let row = sqlx::query(
            "SELECT peer_id, dataset_id, last_acked_seq, last_applied_seq, updated_at FROM replication_checkpoints WHERE peer_id = ? AND dataset_id = ?"
        )
        .bind(peer_id)
        .bind(dataset_id)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = row {
            Ok(Some(Checkpoint {
                peer_id: row.try_get("peer_id")?,
                dataset_id: row.try_get("dataset_id")?,
                last_acked_seq: row.try_get::<i64, _>("last_acked_seq")? as u64,
                last_applied_seq: row.try_get::<i64, _>("last_applied_seq")? as u64,
                updated_at: DateTime::parse_from_rfc3339(&row.try_get::<String, _>("updated_at")?)?
                    .with_timezone(&Utc),
            }))
        } else {
            Ok(None)
        }
    }

    /// All checkpoints for a dataset, one per origin node.
    pub async fn list_for_dataset(pool: &SqlitePool, dataset_id: &str) -> Result<Vec<Checkpoint>> {
        let rows = sqlx::query(
            "SELECT peer_id, dataset_id, last_acked_seq, last_applied_seq, updated_at FROM replication_checkpoints WHERE dataset_id = ? ORDER BY peer_id",
        )
        .bind(dataset_id)
        .fetch_all(pool)
        .await?;

        let mut out = Vec::new();
        for row in rows {
            out.push(Checkpoint {
                peer_id: row.try_get("peer_id")?,
                dataset_id: row.try_get("dataset_id")?,
                last_acked_seq: row.try_get::<i64, _>("last_acked_seq")? as u64,
                last_applied_seq: row.try_get::<i64, _>("last_applied_seq")? as u64,
                updated_at: DateTime::parse_from_rfc3339(&row.try_get::<String, _>("updated_at")?)?
                    .with_timezone(&Utc),
            });
        }
        Ok(out)
    }

    pub async fn update_checkpoint(pool: &SqlitePool, checkpoint: &Checkpoint) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO replication_checkpoints (peer_id, dataset_id, last_acked_seq, last_applied_seq, updated_at)
            VALUES (?, ?, ?, ?, ?)
            ON CONFLICT(peer_id, dataset_id) DO UPDATE SET
                last_acked_seq = excluded.last_acked_seq,
                last_applied_seq = excluded.last_applied_seq,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&checkpoint.peer_id)
        .bind(&checkpoint.dataset_id)
        .bind(checkpoint.last_acked_seq as i64)
        .bind(checkpoint.last_applied_seq as i64)
        .bind(checkpoint.updated_at.to_rfc3339())
        .execute(pool)
        .await?;

        Ok(())
    }
}

pub mod datasets {
    use super::*;
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Dataset {
        pub dataset_id: String,
        pub schema_version: u32,
        pub conflict_policy: String,
        pub table_allowlist: Vec<String>,
        pub created_at: DateTime<Utc>,
    }

    pub async fn register_dataset(pool: &SqlitePool, dataset: &Dataset) -> Result<()> {
        let tables_json = serde_json::to_string(&dataset.table_allowlist)?;

        sqlx::query(
            "INSERT INTO datasets (dataset_id, schema_version, conflict_policy, table_allowlist, created_at) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&dataset.dataset_id)
        .bind(dataset.schema_version as i32)
        .bind(&dataset.conflict_policy)
        .bind(tables_json)
        .bind(dataset.created_at.to_rfc3339())
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn get_dataset(pool: &SqlitePool, dataset_id: &str) -> Result<Option<Dataset>> {
        let row = sqlx::query(
            "SELECT dataset_id, schema_version, conflict_policy, table_allowlist, created_at FROM datasets WHERE dataset_id = ?"
        )
        .bind(dataset_id)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = row {
            let tables_json: String = row.try_get("table_allowlist")?;
            let table_allowlist: Vec<String> = serde_json::from_str(&tables_json)?;

            Ok(Some(Dataset {
                dataset_id: row.try_get("dataset_id")?,
                schema_version: row.try_get::<i32, _>("schema_version")? as u32,
                conflict_policy: row.try_get("conflict_policy")?,
                table_allowlist,
                created_at: DateTime::parse_from_rfc3339(&row.try_get::<String, _>("created_at")?)?
                    .with_timezone(&Utc),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn list_datasets(pool: &SqlitePool) -> Result<Vec<Dataset>> {
        let rows = sqlx::query(
            "SELECT dataset_id, schema_version, conflict_policy, table_allowlist, created_at FROM datasets"
        )
        .fetch_all(pool)
        .await?;

        let mut datasets = Vec::new();
        for row in rows {
            let tables_json: String = row.try_get("table_allowlist")?;
            let table_allowlist: Vec<String> = serde_json::from_str(&tables_json)?;

            datasets.push(Dataset {
                dataset_id: row.try_get("dataset_id")?,
                schema_version: row.try_get::<i32, _>("schema_version")? as u32,
                conflict_policy: row.try_get("conflict_policy")?,
                table_allowlist,
                created_at: DateTime::parse_from_rfc3339(&row.try_get::<String, _>("created_at")?)?
                    .with_timezone(&Utc),
            });
        }

        Ok(datasets)
    }

    pub async fn remove_dataset(pool: &SqlitePool, dataset_id: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM datasets WHERE dataset_id = ?")
            .bind(dataset_id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

pub mod conflicts {
    use super::*;
    use chrono::{DateTime, Utc};

    #[derive(Debug, Clone)]
    pub struct Conflict {
        pub id: String,
        pub operation_id: String,
        pub conflict_type: String,
        pub local_operation_id: Option<String>,
        pub remote_operation_id: String,
        pub detected_at: DateTime<Utc>,
        pub resolved_at: Option<DateTime<Utc>>,
        pub resolution: Option<String>,
    }

    pub async fn list_conflicts(pool: &SqlitePool) -> Result<Vec<Conflict>> {
        let rows = sqlx::query(
            "SELECT id, operation_id, conflict_type, local_operation_id, remote_operation_id, detected_at, resolved_at, resolution FROM conflicts ORDER BY detected_at DESC"
        )
        .fetch_all(pool)
        .await?;

        let mut conflicts = Vec::new();
        for row in rows {
            let resolved_at: Option<String> = row.try_get("resolved_at")?;
            conflicts.push(Conflict {
                id: row.try_get("id")?,
                operation_id: row.try_get("operation_id")?,
                conflict_type: row.try_get("conflict_type")?,
                local_operation_id: row.try_get("local_operation_id")?,
                remote_operation_id: row.try_get("remote_operation_id")?,
                detected_at: DateTime::parse_from_rfc3339(
                    &row.try_get::<String, _>("detected_at")?,
                )?
                .with_timezone(&Utc),
                resolved_at: resolved_at
                    .map(|s| DateTime::parse_from_rfc3339(&s).map(|d| d.with_timezone(&Utc)))
                    .transpose()?,
                resolution: row.try_get("resolution")?,
            });
        }

        Ok(conflicts)
    }

    pub async fn get_conflict(pool: &SqlitePool, id: &str) -> Result<Option<Conflict>> {
        let rows = sqlx::query(
            "SELECT id, operation_id, conflict_type, local_operation_id, remote_operation_id, detected_at, resolved_at, resolution FROM conflicts WHERE id = ?"
        )
        .bind(id)
        .fetch_all(pool)
        .await?;

        if let Some(row) = rows.into_iter().next() {
            let resolved_at: Option<String> = row.try_get("resolved_at")?;
            Ok(Some(Conflict {
                id: row.try_get("id")?,
                operation_id: row.try_get("operation_id")?,
                conflict_type: row.try_get("conflict_type")?,
                local_operation_id: row.try_get("local_operation_id")?,
                remote_operation_id: row.try_get("remote_operation_id")?,
                detected_at: DateTime::parse_from_rfc3339(
                    &row.try_get::<String, _>("detected_at")?,
                )?
                .with_timezone(&Utc),
                resolved_at: resolved_at
                    .map(|s| DateTime::parse_from_rfc3339(&s).map(|d| d.with_timezone(&Utc)))
                    .transpose()?,
                resolution: row.try_get("resolution")?,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn resolve_conflict(pool: &SqlitePool, id: &str, resolution: &str) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE conflicts SET resolved_at = ?, resolution = ? WHERE id = ? AND resolved_at IS NULL"
        )
        .bind(Utc::now().to_rfc3339())
        .bind(resolution)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

pub mod peers {
    use super::*;
    use chrono::{DateTime, Utc};

    #[derive(Debug, Clone)]
    pub struct PeerAuthorization {
        pub peer_id: String,
        pub dataset_id: String,
        pub permissions: i32,
        pub granted_at: DateTime<Utc>,
        pub revoked_at: Option<DateTime<Utc>>,
    }

    pub async fn authorize_peer(
        pool: &SqlitePool,
        peer_id: &str,
        dataset_id: &str,
        permissions: i32,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO peer_authorizations (peer_id, dataset_id, permissions, granted_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(peer_id, dataset_id) DO UPDATE SET
                permissions = excluded.permissions,
                revoked_at = NULL
            "#,
        )
        .bind(peer_id)
        .bind(dataset_id)
        .bind(permissions)
        .bind(Utc::now().to_rfc3339())
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn revoke_peer(pool: &SqlitePool, peer_id: &str, dataset_id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE peer_authorizations SET revoked_at = ? WHERE peer_id = ? AND dataset_id = ?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(peer_id)
        .bind(dataset_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn list_peers(pool: &SqlitePool) -> Result<Vec<PeerAuthorization>> {
        let rows = sqlx::query(
            "SELECT peer_id, dataset_id, permissions, granted_at, revoked_at FROM peer_authorizations ORDER BY peer_id, dataset_id"
        )
        .fetch_all(pool)
        .await?;

        let mut peers = Vec::new();
        for row in rows {
            let revoked_at: Option<String> = row.try_get("revoked_at")?;
            peers.push(PeerAuthorization {
                peer_id: row.try_get("peer_id")?,
                dataset_id: row.try_get("dataset_id")?,
                permissions: row.try_get("permissions")?,
                granted_at: DateTime::parse_from_rfc3339(&row.try_get::<String, _>("granted_at")?)?
                    .with_timezone(&Utc),
                revoked_at: revoked_at
                    .map(|s| DateTime::parse_from_rfc3339(&s).map(|d| d.with_timezone(&Utc)))
                    .transpose()?,
            });
        }

        Ok(peers)
    }

    pub async fn is_authorized(
        pool: &SqlitePool,
        peer_id: &str,
        dataset_id: &str,
        required_permissions: i32,
    ) -> Result<bool> {
        let row = sqlx::query(
            "SELECT permissions, revoked_at FROM peer_authorizations WHERE peer_id = ? AND dataset_id = ?"
        )
        .bind(peer_id)
        .bind(dataset_id)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = row {
            let revoked_at: Option<String> = row.try_get("revoked_at")?;
            if revoked_at.is_some() {
                return Ok(false);
            }
            let permissions: i32 = row.try_get("permissions")?;
            Ok((permissions & required_permissions) == required_permissions)
        } else {
            Ok(false)
        }
    }

    #[derive(Debug, Clone)]
    pub struct PeerCertificate {
        pub peer_id: String,
        pub fingerprint: String,
        pub pinned_at: chrono::DateTime<chrono::Utc>,
    }

    /// Store the fingerprint of a peer's pinned certificate.
    pub async fn pin_certificate(
        pool: &SqlitePool,
        peer_id: &str,
        fingerprint: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO peer_certificates (peer_id, cert_der, fingerprint, pinned_at)
            VALUES (?, X'', ?, ?)
            ON CONFLICT(peer_id) DO UPDATE SET fingerprint = excluded.fingerprint
            "#,
        )
        .bind(peer_id)
        .bind(fingerprint)
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn list_peer_certificates(pool: &SqlitePool) -> Result<Vec<PeerCertificate>> {
        let rows = sqlx::query(
            "SELECT peer_id, fingerprint, pinned_at FROM peer_certificates ORDER BY peer_id",
        )
        .fetch_all(pool)
        .await?;

        let mut certs = Vec::new();
        for row in rows {
            certs.push(PeerCertificate {
                peer_id: row.try_get("peer_id")?,
                fingerprint: row.try_get("fingerprint")?,
                pinned_at: chrono::DateTime::parse_from_rfc3339(
                    &row.try_get::<String, _>("pinned_at")?,
                )?
                .with_timezone(&chrono::Utc),
            });
        }
        Ok(certs)
    }

    pub async fn remove_peer_certificate(pool: &SqlitePool, peer_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM peer_certificates WHERE peer_id = ?")
            .bind(peer_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

pub mod enrollment {
    use super::*;

    #[derive(Debug, Clone)]
    pub struct ClaimedToken {
        pub issuer_node_id: String,
        pub dataset_id: String,
        pub permissions: i32,
        pub expires_at: chrono::DateTime<chrono::Utc>,
    }

    /// Record an issued token so redemption can be validated and enforced
    /// single-use on this node.
    pub async fn issue(
        pool: &SqlitePool,
        token_hash: &str,
        issuer_node_id: &str,
        dataset_id: &str,
        permissions: u32,
        expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO enrollment_tokens (token_hash, issuer_node_id, dataset_id, permissions, expires_at, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(token_hash)
        .bind(issuer_node_id)
        .bind(dataset_id)
        .bind(permissions as i32)
        .bind(expires_at.to_rfc3339())
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Atomically claim a token (single use) and return what it grants.
    ///
    /// Returns `None` when the token is unknown, already used or expired.
    pub async fn claim(pool: &SqlitePool, token_hash: &str) -> Result<Option<ClaimedToken>> {
        let now = chrono::Utc::now();
        let result = sqlx::query(
            r#"
            UPDATE enrollment_tokens
            SET used_at = ?
            WHERE token_hash = ?
              AND used_at IS NULL
              AND expires_at > ?
            "#,
        )
        .bind(now.to_rfc3339())
        .bind(token_hash)
        .bind(now.to_rfc3339())
        .execute(pool)
        .await?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        let row = sqlx::query(
            "SELECT issuer_node_id, dataset_id, permissions, expires_at FROM enrollment_tokens WHERE token_hash = ?",
        )
        .bind(token_hash)
        .fetch_one(pool)
        .await?;

        Ok(Some(ClaimedToken {
            issuer_node_id: row.try_get("issuer_node_id")?,
            dataset_id: row.try_get("dataset_id")?,
            permissions: row.try_get("permissions")?,
            expires_at: chrono::DateTime::parse_from_rfc3339(
                &row.try_get::<String, _>("expires_at")?,
            )?
            .with_timezone(&chrono::Utc),
        }))
    }
}
