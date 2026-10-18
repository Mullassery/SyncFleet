use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use syncfleet::config::PeerConfig;
use syncfleet::identity::NodeIdentity;
use syncfleet::net;
use syncfleet::replication;
use syncfleet::storage::{self, Storage};
use syncfleet::sync_loop;
use syncfleet::tls::{self, cert, store};
use tokio::net::TcpListener;
use tokio_rustls::{TlsAcceptor, TlsConnector};

struct TestNode {
    identity: NodeIdentity,
    dir: tempfile::TempDir,
    storage: Storage,
    cert_path: std::path::PathBuf,
    key_path: std::path::PathBuf,
}

async fn setup_node(name: &str) -> TestNode {
    let dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate(None).unwrap();
    identity.save(dir.path()).unwrap();
    let cert_path = dir.path().join("node.pem");
    let key_path = dir.path().join("node-key.pem");
    cert::generate_and_store(
        &identity,
        name,
        &[IpAddr::from([127, 0, 0, 1])],
        &cert_path,
        &key_path,
    )
    .unwrap();
    let storage = Storage::new(dir.path()).await.unwrap();
    TestNode {
        identity,
        dir,
        storage,
        cert_path,
        key_path,
    }
}

fn pin(node: &TestNode, peer: &TestNode) -> std::path::PathBuf {
    let ca = node.dir.path().join("ca.pem");
    let peer_cert = cert::load_cert_pem(&peer.cert_path).unwrap();
    store::add_cert(&ca, &peer_cert.der).unwrap();
    ca
}

fn client_connector(node: &TestNode, ca: &std::path::Path) -> TlsConnector {
    let store = tls::TlsStore::load(&node.cert_path, &node.key_path).unwrap();
    let cfg = tls::client_config(&store, Arc::new(store::load_roots(ca).unwrap())).unwrap();
    TlsConnector::from(Arc::new(cfg))
}

async fn start_server(
    node: &TestNode,
    ca: &std::path::Path,
    name: &str,
) -> (net::ServerHandle, std::net::SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let store = tls::TlsStore::load(&node.cert_path, &node.key_path).unwrap();
    let cfg = tls::server_config(&store, Arc::new(store::load_roots(ca).unwrap())).unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(cfg));
    let handle = net::spawn_server(
        listener,
        acceptor,
        node.storage.pool().clone(),
        name.to_string(),
        1 << 20,
        Duration::from_secs(5),
        Duration::from_secs(5),
    )
    .unwrap();
    (handle, addr)
}

async fn register_dataset(node: &TestNode, id: &str) {
    let dataset = storage::datasets::Dataset {
        dataset_id: id.to_string(),
        schema_version: 1,
        conflict_policy: "single_writer".to_string(),
        table_allowlist: vec![id.to_string()],
        created_at: chrono::Utc::now(),
    };
    storage::datasets::register_dataset(node.storage.pool(), &dataset)
        .await
        .unwrap();
}

/// A trusts B and B trusts A; A runs the server.
struct Harness {
    a: TestNode,
    b: TestNode,
    addr: std::net::SocketAddr,
    handle: net::ServerHandle,
    connector: TlsConnector,
}

async fn harness() -> Harness {
    let a = setup_node("store-a").await;
    let b = setup_node("store-b").await;
    let ca_a = pin(&a, &b); // A trusts B
    let ca_b = pin(&b, &a); // B trusts A

    register_dataset(&a, "orders").await;
    register_dataset(&b, "orders").await;

    // B may read + write A's "orders"; A may read + write B's "orders".
    storage::peers::authorize_peer(a.storage.pool(), &b.identity.node_id, "orders", 3)
        .await
        .unwrap();
    storage::peers::authorize_peer(b.storage.pool(), &a.identity.node_id, "orders", 3)
        .await
        .unwrap();

    let (handle, addr) = start_server(&a, &ca_a, "store-a").await;
    let connector = client_connector(&b, &ca_b);

    Harness {
        a,
        b,
        addr,
        handle,
        connector,
    }
}

fn hello(node_id: &str, name: &str, datasets: &[&str]) -> net::Hello {
    net::Hello {
        proto: net::PROTO_VERSION,
        node_id: node_id.to_string(),
        node_name: name.to_string(),
        datasets: datasets.iter().map(|s| s.to_string()).collect(),
    }
}

async fn sync_with(
    addr: std::net::SocketAddr,
    connector: &TlsConnector,
    client: &TestNode,
    server_node_id: &str,
    max_ops_per_response: usize,
) -> replication::SyncReport {
    let server_name = rustls::pki_types::ServerName::IpAddress(addr.ip().into());
    let (_reply, report) = net::connect_and_sync(
        addr,
        server_name,
        connector,
        server_node_id,
        hello(&client.identity.node_id, "store-b", &["orders"]),
        client.storage.pool(),
        1 << 20,
        max_ops_per_response,
        Duration::from_secs(5),
        Duration::from_secs(5),
    )
    .await
    .unwrap();
    report
}

async fn sync(h: &Harness) -> replication::SyncReport {
    sync_with(h.addr, &h.connector, &h.b, &h.a.identity.node_id, 1000).await
}

async fn write(node: &TestNode, entity: &str, value: i64) {
    let payload = replication::encode_payload(&serde_json::json!({ "id": entity, "v": value }));
    replication::local_write(
        node.storage.pool(),
        &node.identity.node_id,
        "orders",
        entity,
        replication::parse_op_type("insert").unwrap(),
        1,
        payload,
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn bidirectional_incremental_replication() {
    let h = harness().await;

    // A originates an operation; B should pull it.
    write(&h.a, "order-1", 100).await;

    let report = sync(&h).await;
    assert_eq!(report.datasets, 1);
    assert_eq!(report.pulled, 1, "B should pull A's operation");
    assert_eq!(report.pushed, 0, "B had nothing to push yet");

    // B now holds the operation under A's origin.
    let b_vector = replication::local_vector(h.b.storage.pool(), "orders")
        .await
        .unwrap();
    assert_eq!(b_vector.get(&h.a.identity.node_id).copied(), Some(1));

    // B originates its own operation; A should pull it on the next pass.
    write(&h.b, "order-2", 200).await;

    let report = sync(&h).await;
    assert_eq!(report.pulled, 0, "A has nothing new for B");
    assert_eq!(report.pushed, 1, "B should push its new operation to A");

    let a_vector = replication::local_vector(h.a.storage.pool(), "orders")
        .await
        .unwrap();
    assert_eq!(a_vector.get(&h.b.identity.node_id).copied(), Some(1));

    // Idempotent: a third pass transfers nothing.
    let report = sync(&h).await;
    assert_eq!(report.pulled, 0);
    assert_eq!(report.pushed, 0);

    h.handle.shutdown().await;
}

#[tokio::test]
async fn read_only_peer_cannot_push() {
    let h = harness().await;

    // Revoke B's write permission on A (leave read).
    storage::peers::authorize_peer(h.a.storage.pool(), &h.b.identity.node_id, "orders", 1)
        .await
        .unwrap();

    // B writes locally, then syncs. A must refuse the push (applied 0) while
    // still serving A's own operations for B to pull.
    write(&h.a, "order-a", 1).await;
    write(&h.b, "order-b", 2).await;

    let report = sync(&h).await;
    assert_eq!(report.pulled, 1, "B still pulls A's op");
    assert_eq!(report.pushed, 0, "A refuses B's unauthorized push");

    let a_vector = replication::local_vector(h.a.storage.pool(), "orders")
        .await
        .unwrap();
    assert_eq!(a_vector.get(&h.b.identity.node_id).copied(), None);

    h.handle.shutdown().await;
}

#[tokio::test]
async fn state_survives_restart() {
    let mut h = harness().await;

    write(&h.a, "order-1", 100).await;
    assert_eq!(sync(&h).await.pulled, 1);

    write(&h.b, "order-2", 200).await;
    assert_eq!(sync(&h).await.pushed, 1);

    // Simulate a full restart of both nodes: stop the server, drop the pools,
    // and reopen each database from the same on-disk directory.
    h.handle.shutdown().await;
    h.a.storage.close().await;
    h.b.storage.close().await;
    h.a.storage = Storage::new(h.a.dir.path()).await.unwrap();
    h.b.storage = Storage::new(h.b.dir.path()).await.unwrap();

    // Vectors and checkpoints persisted across the restart.
    let a_vector = replication::local_vector(h.a.storage.pool(), "orders")
        .await
        .unwrap();
    let b_vector = replication::local_vector(h.b.storage.pool(), "orders")
        .await
        .unwrap();
    for (id, vector) in [
        (&h.a.identity.node_id, &a_vector),
        (&h.b.identity.node_id, &b_vector),
    ] {
        assert_eq!(vector.get(&h.a.identity.node_id).copied(), Some(1), "{id}");
        assert_eq!(vector.get(&h.b.identity.node_id).copied(), Some(1), "{id}");
    }

    // Bring the server back up and re-sync: both sides are already converged.
    let (handle, addr) = start_server(&h.a, &h.a.dir.path().join("ca.pem"), "store-a").await;
    let connector = client_connector(&h.b, &h.b.dir.path().join("ca.pem"));
    let report = sync_with(addr, &connector, &h.b, &h.a.identity.node_id, 1000).await;
    assert_eq!(report.pulled, 0);
    assert_eq!(report.pushed, 0);

    handle.shutdown().await;
}

#[tokio::test]
async fn bootstrap_transfers_all_batches_in_one_session() {
    let h = harness().await;

    // 7 operations, but only 2 per message: the session must loop to pull
    // every batch, otherwise a new peer could never catch up.
    for i in 0..7 {
        write(&h.a, &format!("order-{i}"), i).await;
    }

    let report = sync_with(h.addr, &h.connector, &h.b, &h.a.identity.node_id, 2).await;
    assert_eq!(
        report.pulled, 7,
        "all batches must be pulled in one session"
    );

    let b_vector = replication::local_vector(h.b.storage.pool(), "orders")
        .await
        .unwrap();
    assert_eq!(b_vector.get(&h.a.identity.node_id).copied(), Some(7));

    h.handle.shutdown().await;
}

#[tokio::test]
async fn multi_batch_push_in_one_session() {
    let h = harness().await;

    for i in 0..7 {
        write(&h.b, &format!("order-{i}"), i).await;
    }

    let report = sync_with(h.addr, &h.connector, &h.b, &h.a.identity.node_id, 2).await;
    assert_eq!(report.pulled, 0);
    assert_eq!(
        report.pushed, 7,
        "all push batches must land in one session"
    );

    let a_vector = replication::local_vector(h.a.storage.pool(), "orders")
        .await
        .unwrap();
    assert_eq!(a_vector.get(&h.b.identity.node_id).copied(), Some(7));

    h.handle.shutdown().await;
}

#[tokio::test]
async fn multiple_operations_transfer_in_order() {
    let h = harness().await;
    for i in 0..5 {
        write(&h.a, &format!("order-{i}"), i).await;
    }

    let report = sync(&h).await;
    assert_eq!(report.pulled, 5);

    let rows = sqlx::query("SELECT origin_seq FROM operation_journal WHERE dataset_id = 'orders' AND origin_node_id = ? ORDER BY origin_seq")
        .bind(&h.a.identity.node_id)
        .fetch_all(h.b.storage.pool())
        .await
        .unwrap();
    assert_eq!(rows.len(), 5);

    h.handle.shutdown().await;
}

fn loop_opts(interval: Duration, backoff_max: Duration) -> sync_loop::SyncLoopOptions {
    sync_loop::SyncLoopOptions {
        max_message_size: 1 << 20,
        max_ops_per_response: 1000,
        connect_timeout: Duration::from_secs(5),
        idle_timeout: Duration::from_secs(5),
        interval,
        backoff_max,
    }
}

#[tokio::test]
async fn background_loop_converges_without_manual_sync() {
    let h = harness().await;
    write(&h.a, "order-1", 100).await;

    let connector = client_connector(&h.b, &h.b.dir.path().join("ca.pem"));
    let peer = PeerConfig {
        node_id: h.a.identity.node_id.clone(),
        address: h.addr.to_string(),
    };
    let loop_handle = sync_loop::spawn_sync_loop(
        h.b.storage.pool().clone(),
        connector,
        h.b.identity.node_id.clone(),
        "store-b".to_string(),
        vec![peer],
        loop_opts(Duration::from_millis(50), Duration::from_millis(200)),
    );
    assert_eq!(loop_handle.peer_count(), 1);

    let mut converged = false;
    for _ in 0..100 {
        let vector = replication::local_vector(h.b.storage.pool(), "orders")
            .await
            .unwrap();
        if vector.get(&h.a.identity.node_id).copied() == Some(1) {
            converged = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    loop_handle.shutdown().await;
    h.handle.shutdown().await;
    assert!(converged, "background loop never replicated A's operation");
}

#[tokio::test]
async fn background_loop_survives_unreachable_peer() {
    let h = harness().await;
    h.handle.shutdown().await;

    let connector = client_connector(&h.b, &h.b.dir.path().join("ca.pem"));
    let peer = PeerConfig {
        node_id: h.a.identity.node_id.clone(),
        address: "127.0.0.1:1".to_string(),
    };
    let loop_handle = sync_loop::spawn_sync_loop(
        h.b.storage.pool().clone(),
        connector,
        h.b.identity.node_id.clone(),
        "store-b".to_string(),
        vec![peer],
        loop_opts(Duration::from_millis(20), Duration::from_millis(80)),
    );

    tokio::time::sleep(Duration::from_millis(300)).await;
    // Must not hang: shutdown cancels the backed-off sleep.
    tokio::time::timeout(Duration::from_secs(2), loop_handle.shutdown())
        .await
        .expect("sync loop failed to shut down");
}

#[tokio::test]
async fn dynamically_supplied_peer_is_synced() {
    let h = harness().await;
    write(&h.a, "order-1", 100).await;

    let connector = client_connector(&h.b, &h.b.dir.path().join("ca.pem"));
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let loop_handle = sync_loop::spawn_sync_loop_with_updates(
        h.b.storage.pool().clone(),
        connector,
        h.b.identity.node_id.clone(),
        "store-b".to_string(),
        vec![],
        loop_opts(Duration::from_millis(50), Duration::from_millis(200)),
        rx,
    );
    assert_eq!(loop_handle.peer_count(), 0);

    // Simulate discovery delivering a peer after the loop is already running.
    tx.send(PeerConfig {
        node_id: h.a.identity.node_id.clone(),
        address: h.addr.to_string(),
    })
    .unwrap();

    let mut converged = false;
    for _ in 0..100 {
        let vector = replication::local_vector(h.b.storage.pool(), "orders")
            .await
            .unwrap();
        if vector.get(&h.a.identity.node_id).copied() == Some(1) {
            converged = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert_eq!(loop_handle.peer_count(), 1);
    loop_handle.shutdown().await;
    h.handle.shutdown().await;
    assert!(converged, "dynamically added peer was never synced");
}
