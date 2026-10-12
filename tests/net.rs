use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use syncfleet::identity::NodeIdentity;
use syncfleet::net;
use syncfleet::storage::{self, Storage};
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

/// Pin `peer`'s certificate into `node`'s trust store and return its path.
fn pin(node: &TestNode, peer: &TestNode) -> std::path::PathBuf {
    let ca = node.dir.path().join("ca.pem");
    let peer_cert = cert::load_cert_pem(&peer.cert_path).unwrap();
    store::add_cert(&ca, &peer_cert.der).unwrap();
    ca
}

struct Harness {
    a: TestNode,
    b: TestNode,
    addr: std::net::SocketAddr,
    handle: net::ServerHandle,
    connector: TlsConnector,
    ca_a: std::path::PathBuf,
}

async fn harness() -> Harness {
    let a = setup_node("store-a").await;
    let b = setup_node("store-b").await;
    let ca_a = pin(&a, &b); // A trusts B
    let ca_b = pin(&b, &a); // B trusts A

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_store = tls::TlsStore::load(&a.cert_path, &a.key_path).unwrap();
    let server_cfg =
        tls::server_config(&server_store, Arc::new(store::load_roots(&ca_a).unwrap())).unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(server_cfg));

    let handle = net::spawn_server(
        listener,
        acceptor,
        a.storage.pool().clone(),
        "store-a".to_string(),
        1 << 20,
        Duration::from_secs(5),
        Duration::from_secs(5),
    )
    .unwrap();

    let client_store = tls::TlsStore::load(&b.cert_path, &b.key_path).unwrap();
    let client_cfg =
        tls::client_config(&client_store, Arc::new(store::load_roots(&ca_b).unwrap())).unwrap();
    let connector = TlsConnector::from(Arc::new(client_cfg));

    Harness {
        a,
        b,
        addr,
        handle,
        connector,
        ca_a,
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

#[tokio::test]
async fn mutual_tls_authorized_exchange() {
    let h = harness().await;
    storage::peers::authorize_peer(h.a.storage.pool(), &h.b.identity.node_id, "orders", 1)
        .await
        .unwrap();

    let server_name = rustls::pki_types::ServerName::IpAddress(h.addr.ip().into());
    let reply = net::connect(
        h.addr,
        server_name,
        &h.connector,
        &h.a.identity.node_id,
        hello(&h.b.identity.node_id, "store-b", &["orders"]),
        1 << 20,
        Duration::from_secs(5),
    )
    .await
    .unwrap();

    assert!(reply.ok);
    assert_eq!(reply.peer_node_id, h.b.identity.node_id);
    assert_eq!(reply.authorized_datasets, vec!["orders".to_string()]);

    h.handle.shutdown().await;
}

#[tokio::test]
async fn unauthorized_peer_is_rejected() {
    let h = harness().await;
    // No authorization recorded for B on A.

    let server_name = rustls::pki_types::ServerName::IpAddress(h.addr.ip().into());
    let err = net::connect(
        h.addr,
        server_name,
        &h.connector,
        &h.a.identity.node_id,
        hello(&h.b.identity.node_id, "store-b", &["orders"]),
        1 << 20,
        Duration::from_secs(5),
    )
    .await
    .unwrap_err();

    assert!(matches!(
        err,
        syncfleet::error::SyncFleetError::Transport(
            syncfleet::error::TransportError::PeerRejected(_)
        )
    ));

    h.handle.shutdown().await;
    let _ = &h.ca_a;
}

#[tokio::test]
async fn hello_node_id_mismatch_is_rejected() {
    let h = harness().await;
    storage::peers::authorize_peer(h.a.storage.pool(), &h.b.identity.node_id, "orders", 1)
        .await
        .unwrap();

    let server_name = rustls::pki_types::ServerName::IpAddress(h.addr.ip().into());
    let err = net::connect(
        h.addr,
        server_name,
        &h.connector,
        &h.a.identity.node_id,
        hello("someone-else", "store-b", &["orders"]),
        1 << 20,
        Duration::from_secs(5),
    )
    .await
    .unwrap_err();

    assert!(matches!(
        err,
        syncfleet::error::SyncFleetError::Transport(
            syncfleet::error::TransportError::PeerRejected(_)
        )
    ));

    h.handle.shutdown().await;
}

#[tokio::test]
async fn unpinned_server_cert_fails_handshake() {
    let a = setup_node("store-a").await;
    let b = setup_node("store-b").await;
    let c = setup_node("store-c").await; // unrelated third node

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // A trusts B so the server side is willing to talk.
    let ca_a = pin(&a, &b);
    let server_store = tls::TlsStore::load(&a.cert_path, &a.key_path).unwrap();
    let server_cfg =
        tls::server_config(&server_store, Arc::new(store::load_roots(&ca_a).unwrap())).unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(server_cfg));
    let handle = net::spawn_server(
        listener,
        acceptor,
        a.storage.pool().clone(),
        "store-a".to_string(),
        1 << 20,
        Duration::from_secs(5),
        Duration::from_secs(5),
    )
    .unwrap();

    // B trusts C, but NOT A: verification of A's certificate must fail.
    let ca_b = b.dir.path().join("ca.pem");
    let c_cert = cert::load_cert_pem(&c.cert_path).unwrap();
    store::add_cert(&ca_b, &c_cert.der).unwrap();

    let client_store = tls::TlsStore::load(&b.cert_path, &b.key_path).unwrap();
    let client_cfg =
        tls::client_config(&client_store, Arc::new(store::load_roots(&ca_b).unwrap())).unwrap();
    let connector = TlsConnector::from(Arc::new(client_cfg));

    let server_name = rustls::pki_types::ServerName::IpAddress(addr.ip().into());
    let result = net::connect(
        addr,
        server_name,
        &connector,
        &a.identity.node_id,
        hello(&b.identity.node_id, "store-b", &["orders"]),
        1 << 20,
        Duration::from_secs(5),
    )
    .await;

    assert!(result.is_err());
    handle.shutdown().await;
}
