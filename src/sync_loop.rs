//! Background synchronization against peers (Phases 4/5).
//!
//! The daemon owns a long-lived task per peer. Each task connects to its peer,
//! runs one replication session, then sleeps for
//! `replication.sync_interval_sec` and repeats. Failures (peer offline, TLS
//! rejected, etc.) trigger exponential backoff up to
//! `replication.reconnect_backoff_max_sec`, so a missing peer never spins.
//!
//! Peers come from two sources: the static `[[peers]]` configuration and, when
//! mDNS discovery is enabled, peers found on the local network. Discovered
//! peers are fed in through [`spawn_sync_loop_with_updates`].

use crate::config::PeerConfig;
use crate::error::{Result, SyncFleetError};
use crate::net;
use crate::replication::SyncReport;
use crate::storage;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio_rustls::TlsConnector;

/// Timing and sizing knobs for the outbound sync loop.
#[derive(Debug, Clone)]
pub struct SyncLoopOptions {
    pub max_message_size: usize,
    pub max_ops_per_response: usize,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
    /// Delay between successful (or no-op) sync sessions.
    pub interval: Duration,
    /// Upper bound for the exponential backoff between failed attempts.
    pub backoff_max: Duration,
}

/// Handle to the running peer tasks; call [`SyncLoopHandle::shutdown`] to stop
/// them and wait for an orderly exit.
pub struct SyncLoopHandle {
    shutdown_tx: watch::Sender<bool>,
    manager: JoinHandle<()>,
    active: Arc<AtomicUsize>,
}

impl SyncLoopHandle {
    /// Signal every peer task to stop and await their completion.
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        let _ = self.manager.await;
    }

    /// Number of peer tasks currently running (static plus discovered).
    pub fn peer_count(&self) -> usize {
        self.active.load(Ordering::Relaxed)
    }
}

/// Spawn one background sync task per configured peer.
pub fn spawn_sync_loop(
    pool: SqlitePool,
    connector: TlsConnector,
    local_node_id: String,
    local_node_name: String,
    peers: Vec<PeerConfig>,
    opts: SyncLoopOptions,
) -> SyncLoopHandle {
    let (_keepalive, updates) = mpsc::unbounded_channel();
    spawn_sync_loop_with_updates(
        pool,
        connector,
        local_node_id,
        local_node_name,
        peers,
        opts,
        updates,
    )
}

/// Like [`spawn_sync_loop`], but also accepts additional peers at runtime from
/// `updates` (e.g. mDNS discovery). Duplicate node ids are ignored.
pub fn spawn_sync_loop_with_updates(
    pool: SqlitePool,
    connector: TlsConnector,
    local_node_id: String,
    local_node_name: String,
    initial: Vec<PeerConfig>,
    opts: SyncLoopOptions,
    mut updates: mpsc::UnboundedReceiver<PeerConfig>,
) -> SyncLoopHandle {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let connector = Arc::new(connector);
    let opts = Arc::new(opts);
    let active = Arc::new(AtomicUsize::new(0));

    // Spawn the static peers synchronously so `peer_count()` reflects them
    // immediately, then hand the task set to the manager for dynamic updates.
    let mut tasks: JoinSet<()> = JoinSet::new();
    let mut known: HashSet<String> = HashSet::new();
    for peer in initial {
        spawn_peer(
            &mut tasks,
            &shutdown_rx,
            &active,
            &pool,
            &connector,
            &local_node_id,
            &local_node_name,
            &opts,
            &mut known,
            peer,
        );
    }

    let active_mgr = active.clone();
    let manager = tokio::spawn(async move {
        let mut manager_shutdown = shutdown_rx.clone();
        let mut updates_open = true;
        loop {
            tokio::select! {
                _ = manager_shutdown.changed() => break,
                maybe = updates.recv(), if updates_open => match maybe {
                    Some(peer) => spawn_peer(
                        &mut tasks,
                        &shutdown_rx,
                        &active_mgr,
                        &pool,
                        &connector,
                        &local_node_id,
                        &local_node_name,
                        &opts,
                        &mut known,
                        peer,
                    ),
                    None => updates_open = false,
                },
            }
        }

        tasks.shutdown().await;
    });

    SyncLoopHandle {
        shutdown_tx,
        manager,
        active,
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_peer(
    tasks: &mut JoinSet<()>,
    shutdown: &watch::Receiver<bool>,
    active: &Arc<AtomicUsize>,
    pool: &SqlitePool,
    connector: &Arc<TlsConnector>,
    local_node_id: &str,
    local_node_name: &str,
    opts: &Arc<SyncLoopOptions>,
    known: &mut HashSet<String>,
    peer: PeerConfig,
) {
    if peer.node_id.is_empty() || peer.address.is_empty() {
        tracing::warn!(
            address = %peer.address,
            "skipping peer with missing node_id or address"
        );
        return;
    }
    if !known.insert(peer.node_id.clone()) {
        return;
    }

    tracing::info!(peer = %peer.node_id, address = %peer.address, "starting peer sync task");
    active.fetch_add(1, Ordering::Relaxed);

    let pool = pool.clone();
    let connector = connector.clone();
    let node_id = local_node_id.to_string();
    let node_name = local_node_name.to_string();
    let opts = opts.clone();
    let active = active.clone();
    let shutdown = shutdown.clone();

    tasks.spawn(async move {
        run_peer(
            &pool, &connector, &node_id, &node_name, &peer, &opts, shutdown,
        )
        .await;
        active.fetch_sub(1, Ordering::Relaxed);
    });
}

async fn run_peer(
    pool: &SqlitePool,
    connector: &TlsConnector,
    local_node_id: &str,
    local_node_name: &str,
    peer: &PeerConfig,
    opts: &SyncLoopOptions,
    mut shutdown: watch::Receiver<bool>,
) {
    let interval = opts.interval.max(Duration::from_secs(1));
    let backoff_max = opts.backoff_max.max(interval);
    let mut delay = interval;

    loop {
        match attempt(pool, connector, local_node_id, local_node_name, peer, opts).await {
            Ok(report) => {
                if report.pulled > 0 || report.pushed > 0 || report.datasets > 0 {
                    tracing::info!(
                        peer = %peer.node_id,
                        pulled = report.pulled,
                        pushed = report.pushed,
                        datasets = report.datasets,
                        "peer sync complete"
                    );
                } else {
                    tracing::debug!(peer = %peer.node_id, "peer sync: nothing to do");
                }
                delay = interval;
            }
            Err(e) => {
                tracing::warn!(
                    peer = %peer.node_id,
                    error = %e,
                    retry_in_secs = delay.as_secs(),
                    "peer sync failed; backing off"
                );
                delay = (delay * 2).min(backoff_max);
            }
        }

        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(delay) => {}
        }
    }
}

async fn attempt(
    pool: &SqlitePool,
    connector: &TlsConnector,
    local_node_id: &str,
    local_node_name: &str,
    peer: &PeerConfig,
    opts: &SyncLoopOptions,
) -> Result<SyncReport> {
    if peer.node_id.is_empty() {
        return Err(SyncFleetError::InvalidArgument(
            "peer is missing node_id".to_string(),
        ));
    }

    let (addr, server_name) = net::resolve_peer_addr(&peer.address)?;

    let datasets: Vec<String> = storage::datasets::list_datasets(pool)
        .await?
        .into_iter()
        .map(|d| d.dataset_id)
        .collect();
    if datasets.is_empty() {
        return Ok(SyncReport::default());
    }

    let hello = net::Hello {
        proto: net::PROTO_VERSION,
        node_id: local_node_id.to_string(),
        node_name: local_node_name.to_string(),
        datasets,
    };

    let (_reply, report) = net::connect_and_sync(
        addr,
        server_name,
        connector,
        &peer.node_id,
        hello,
        pool,
        opts.max_message_size,
        opts.max_ops_per_response,
        opts.connect_timeout,
        opts.idle_timeout,
    )
    .await?;

    Ok(report)
}

/// Build the [`PeerConfig`] set for peers that have at least one live
/// authorization, used to filter discovered peers before syncing with them.
pub async fn authorized_peer_ids(pool: &SqlitePool) -> Result<HashSet<String>> {
    let peers = storage::peers::list_peers(pool).await?;
    Ok(peers
        .into_iter()
        .filter(|p| p.revoked_at.is_none())
        .map(|p| p.peer_id)
        .collect())
}
