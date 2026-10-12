use syncfleet::identity::NodeIdentity;

#[test]
fn generates_stable_identity_and_node_id() {
    let identity = NodeIdentity::generate(None).unwrap();

    assert_eq!(
        identity.node_id.len(),
        32,
        "node id is 16 bytes hex-encoded"
    );
    assert_eq!(identity.ed25519_public.len(), 32);
    assert_eq!(identity.x25519_public.len(), 32);
    // Without a passphrase the private seed is stored verbatim.
    assert_eq!(identity.ed25519_private_encrypted.len(), 32);
}

#[test]
fn sign_and_verify_roundtrip() {
    let identity = NodeIdentity::generate(None).unwrap();
    let message = b"replication handshake transcript";

    let signature = identity.sign(message, None).unwrap();
    assert!(identity.verify(message, &signature).is_ok());
    assert!(identity.verify(b"tampered", &signature).is_err());
}

#[test]
fn passphrase_encrypts_keys_and_roundtrips() {
    let identity = NodeIdentity::generate(Some("correct horse")).unwrap();

    // Encrypted blobs must include the nonce and exceed the raw key length.
    assert!(identity.ed25519_private_encrypted.len() > 32);

    // Correct passphrase recovers a usable signing key.
    let signing = identity.ed25519_signing_key(Some("correct horse")).unwrap();
    assert_eq!(
        signing.verifying_key().to_bytes().to_vec(),
        identity.ed25519_public
    );

    // Wrong passphrase must fail rather than return garbage.
    let err = identity.x25519_static_secret(Some("wrong"));
    assert!(err.is_err(), "wrong passphrase must not decrypt");
}

#[test]
fn save_and_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let identity = NodeIdentity::generate(None).unwrap();
    identity.save(dir.path()).unwrap();

    assert!(NodeIdentity::exists(dir.path()));
    let loaded = NodeIdentity::load(dir.path()).unwrap();
    assert_eq!(loaded.node_id, identity.node_id);
    assert_eq!(loaded.ed25519_public, identity.ed25519_public);
}

#[test]
fn enrollment_tokens_are_valid_then_expire_on_use() {
    let mut token = NodeIdentity::enrollment_token();
    assert!(token.is_valid());

    token.mark_used();
    assert!(!token.is_valid());
}
