use assert_cmd::Command;
use predicates::prelude::*;

/// Build a command with an isolated data directory via environment override.
fn cmd(data_dir: &std::path::Path) -> Command {
    let mut c = Command::cargo_bin("syncfleet").unwrap();
    c.env("SYNCFLEET_NODE__DATA_DIR", data_dir);
    c.env("SYNCFLEET_NODE__LOG_FORMAT", "text");
    c
}

#[test]
fn init_then_status_and_doctor() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");

    cmd(&data)
        .args(["init", "--node", "store-a"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized SyncFleet node"))
        .stdout(predicate::str::contains("Data dir:"));

    // Re-initialization without --force is rejected.
    cmd(&data)
        .args(["init", "--node", "store-a"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already initialized"));

    cmd(&data)
        .args(["status", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"node_id\""));

    cmd(&data)
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("[ok] database"));
}

#[test]
fn commands_before_init_fail_helpfully() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");

    cmd(&data)
        .args(["dataset", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not initialized"));
}

#[test]
fn dataset_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    cmd(&data)
        .args(["init", "--node", "store-a"])
        .assert()
        .success();

    cmd(&data)
        .args([
            "dataset",
            "register",
            "--name",
            "orders",
            "--tables",
            "orders,order_items",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Registered dataset 'orders'"));

    // Duplicate without --force fails.
    cmd(&data)
        .args([
            "dataset", "register", "--name", "orders", "--tables", "orders",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("already registered"));

    cmd(&data)
        .args(["dataset", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("orders").and(predicate::str::contains("order_items")));

    cmd(&data)
        .args(["dataset", "remove", "orders"])
        .assert()
        .success();

    cmd(&data)
        .args(["dataset", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No datasets registered"));
}

#[test]
fn peer_pair_and_revoke() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    cmd(&data)
        .args(["init", "--node", "store-a"])
        .assert()
        .success();
    cmd(&data)
        .args([
            "dataset", "register", "--name", "orders", "--tables", "orders",
        ])
        .assert()
        .success();

    cmd(&data)
        .args([
            "peer",
            "pair",
            "--address",
            "10.0.0.2:8443",
            "--peer-id",
            "store-b",
            "--dataset",
            "orders",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Authorized peer 'store-b'"));

    cmd(&data)
        .args(["peer", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("store-b").and(predicate::str::contains("active")));

    cmd(&data)
        .args(["peer", "revoke", "store-b"])
        .assert()
        .success();

    cmd(&data)
        .args(["peer", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("revoked"));
}

#[test]
fn config_show_reflects_env_override() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");

    cmd(&data)
        .args(["config", "validate"])
        .assert()
        .success()
        .stdout(predicate::str::contains("valid"));

    cmd(&data)
        .args(["config", "show"])
        .assert()
        .success()
        .stdout(predicate::str::contains(data.to_string_lossy().as_ref()));
}

#[test]
fn journal_stats_and_compact_dry_run() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    cmd(&data)
        .args(["init", "--node", "store-a"])
        .assert()
        .success();

    cmd(&data)
        .args(["journal", "stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Journal operations:"));

    cmd(&data)
        .args(["journal", "compact", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("eligible for compaction"));
}

#[test]
fn doctor_full_and_repair() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    cmd(&data)
        .args(["init", "--node", "store-a"])
        .assert()
        .success();

    cmd(&data)
        .args(["doctor", "--full"])
        .assert()
        .success()
        .stdout(predicate::str::contains("integrity_check"))
        .stdout(predicate::str::contains("journal"));

    cmd(&data)
        .args(["doctor", "--repair"])
        .assert()
        .success()
        .stdout(predicate::str::contains("wal_checkpoint"));
}

#[test]
fn init_generates_tls_material() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    cmd(&data)
        .args(["init", "--node", "store-a"])
        .assert()
        .success()
        .stdout(predicate::str::contains("TLS cert:"))
        .stdout(predicate::str::contains("Cert fp:"));

    assert!(data.join("certs").join("node.pem").exists());
    assert!(data.join("certs").join("node-key.pem").exists());

    cmd(&data)
        .args(["peer", "export-cert"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BEGIN CERTIFICATE"));
}

#[test]
fn peer_token_requires_registered_dataset() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    cmd(&data)
        .args(["init", "--node", "store-a"])
        .assert()
        .success();

    cmd(&data)
        .args(["peer", "token", "--dataset", "orders"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not registered"));
}

#[test]
fn peer_token_redeem_and_import_cert_flow() {
    let dir = tempfile::tempdir().unwrap();
    let a_data = dir.path().join("a");
    let b_data = dir.path().join("b");

    // Node A owns the dataset and issues a token.
    cmd(&a_data)
        .args(["init", "--node", "store-a"])
        .assert()
        .success();
    cmd(&a_data)
        .args([
            "dataset", "register", "--name", "orders", "--tables", "orders",
        ])
        .assert()
        .success();

    let output = cmd(&a_data)
        .args([
            "peer",
            "token",
            "--dataset",
            "orders",
            "--permissions",
            "1",
            "--ttl-hours",
            "1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Enrollment token"))
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    let token = stdout
        .lines()
        .find(|l| l.starts_with("sf1:"))
        .expect("token line")
        .to_string();

    // Node A exports its certificate.
    let a_cert = dir.path().join("a.pem");
    cmd(&a_data)
        .args(["peer", "export-cert", "--out", a_cert.to_str().unwrap()])
        .assert()
        .success();
    assert!(a_cert.exists());

    // Node B redeems the token, pinning A's certificate.
    cmd(&b_data)
        .args(["init", "--node", "store-b"])
        .assert()
        .success();
    cmd(&b_data)
        .args(["peer", "redeem", "--token", &token])
        .assert()
        .success()
        .stdout(predicate::str::contains("Redeemed enrollment token"));

    cmd(&b_data)
        .args(["peer", "certs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No pinned").not());

    // B exports its own certificate for A to import.
    let b_cert = dir.path().join("b.pem");
    cmd(&b_data)
        .args(["peer", "export-cert", "--out", b_cert.to_str().unwrap()])
        .assert()
        .success();

    // A imports B's certificate (peer id derived from the cert) and
    // authorizes it for the dataset.
    cmd(&a_data)
        .args([
            "peer",
            "import-cert",
            "--file",
            b_cert.to_str().unwrap(),
            "--dataset",
            "orders",
            "--permissions",
            "1",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Imported certificate"))
        .stdout(predicate::str::contains("Authorized"));

    cmd(&a_data)
        .args(["peer", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("orders").and(predicate::str::contains("active")));

    // A second import of the same cert is idempotent at the trust-store level.
    cmd(&a_data)
        .args(["peer", "import-cert", "--file", b_cert.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("already pinned"));
}

#[test]
fn peer_import_cert_rejects_invalid_file() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    cmd(&data)
        .args(["init", "--node", "store-a"])
        .assert()
        .success();

    let bogus = dir.path().join("bogus.pem");
    std::fs::write(&bogus, b"not a certificate").unwrap();

    cmd(&data)
        .args(["peer", "import-cert", "--file", bogus.to_str().unwrap()])
        .assert()
        .failure();
}
