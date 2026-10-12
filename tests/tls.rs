use std::net::IpAddr;
use std::path::Path;
use syncfleet::identity::NodeIdentity;
use syncfleet::tls::{cert, enrollment, store};

fn make_node(dir: &Path, name: &str) -> (NodeIdentity, cert::CertMaterial) {
    let identity = NodeIdentity::generate(None).unwrap();
    identity.save(dir).unwrap();
    let cert_path = dir.join("node.pem");
    let key_path = dir.join("node-key.pem");
    let material = cert::generate_and_store(
        &identity,
        name,
        &[IpAddr::from([127, 0, 0, 1])],
        &cert_path,
        &key_path,
    )
    .unwrap();
    assert!(cert_path.exists());
    assert!(key_path.exists());
    (identity, material)
}

fn signing_key(dir: &Path) -> ed25519_dalek::SigningKey {
    cert::signing_key_from_pem(&dir.join("node-key.pem")).unwrap()
}

#[test]
fn cert_generate_parse_and_node_id_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let (identity, material) = make_node(dir.path(), "store-a");

    assert_eq!(
        cert::node_id_from_cert(&material.der).unwrap(),
        identity.node_id
    );
    assert_eq!(cert::fingerprint(&material.der), material.fingerprint);
    assert_eq!(cert::subject_name(&material.der).unwrap(), "store-a");

    let loaded = cert::load_cert_pem(&dir.path().join("node.pem")).unwrap();
    assert_eq!(loaded.der, material.der);
    cert::load_private_key_pem(&dir.path().join("node-key.pem")).unwrap();

    let again = cert::ensure_material(
        dir.path(),
        "store-a",
        &[],
        &dir.path().join("node.pem"),
        &dir.path().join("node-key.pem"),
    )
    .unwrap();
    assert_eq!(again.fingerprint, material.fingerprint);
}

#[test]
fn trust_store_add_load_contains_remove() {
    let dir = tempfile::tempdir().unwrap();
    let ca = dir.path().join("certs").join("ca.pem");
    let (_identity, material) = make_node(dir.path(), "store-a");

    assert!(store::add_cert(&ca, &material.der).unwrap());
    assert!(!store::add_cert(&ca, &material.der).unwrap());
    assert_eq!(store::load_roots(&ca).unwrap().len(), 1);

    assert!(store::remove_cert(&ca, &material.fingerprint).unwrap());
    assert_eq!(store::load_roots(&ca).unwrap().len(), 0);
}

#[test]
fn enrollment_token_sign_and_verify() {
    let dir = tempfile::tempdir().unwrap();
    let (identity, material) = make_node(dir.path(), "store-a");

    let raw = enrollment::issue(&material, &signing_key(dir.path()), "orders", 3, 24).unwrap();
    assert!(raw.starts_with("sf1:"));

    let token = enrollment::parse(&raw).unwrap();
    enrollment::verify(&token).unwrap();
    assert_eq!(enrollment::issuer_node_id(&token), identity.node_id);
    assert_eq!(enrollment::dataset(&token), "orders");
    assert_eq!(enrollment::permissions(&token), 3);
    assert_eq!(enrollment::issuer_cert_der(&token).unwrap(), material.der);
}

#[test]
fn enrollment_token_tamper_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (_identity, material) = make_node(dir.path(), "store-a");
    let raw = enrollment::issue(&material, &signing_key(dir.path()), "orders", 3, 24).unwrap();

    let mut bad = enrollment::parse(&raw).unwrap();
    bad.claims.dataset = "invoices".to_string();
    assert!(enrollment::verify(&bad).is_err());

    let mut bad_cert = enrollment::parse(&raw).unwrap();
    bad_cert.claims.cert = hex::encode([0u8; 8]);
    assert!(enrollment::verify(&bad_cert).is_err());
}

#[test]
fn enrollment_token_expiry_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (_identity, material) = make_node(dir.path(), "store-a");
    let raw = enrollment::issue(&material, &signing_key(dir.path()), "orders", 1, 0).unwrap();

    let mut token = enrollment::parse(&raw).unwrap();
    token.claims.exp = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
    assert!(enrollment::verify(&token).is_err());
}

#[test]
fn enrollment_parse_rejects_bad_prefix() {
    assert!(enrollment::parse("not-a-token").is_err());
}
