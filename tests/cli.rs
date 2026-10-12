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
