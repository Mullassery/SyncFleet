use syncfleet::storage::{self, Storage};

use uuid::Uuid;

fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(fut)
}

#[test]
fn migrations_apply_and_health_check_passes() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();
        storage.health_check().await.unwrap();
    });
}

async fn register_orders(storage: &Storage) {
    let dataset = storage::datasets::Dataset {
        dataset_id: "orders".to_string(),
        schema_version: 1,
        conflict_policy: "single_writer".to_string(),
        table_allowlist: vec!["orders".to_string()],
        created_at: chrono::Utc::now(),
    };
    storage::datasets::register_dataset(storage.pool(), &dataset)
        .await
        .unwrap();
}

#[test]
fn local_append_allocates_gap_free_sequences() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();
        register_orders(&storage).await;

        let mut seqs = Vec::new();
        for i in 0..5 {
            let op = storage::operations::append_local_operation(
                storage.pool(),
                storage::operations::LocalOperation {
                    dataset_id: "orders".to_string(),
                    entity_id: format!("order-{i}"),
                    op_type: storage::operations::OperationType::Insert,
                    schema_version: 1,
                    payload: format!("{{\"n\":{i}}}").into_bytes(),
                    origin_node_id: "node-a".to_string(),
                },
            )
            .await
            .unwrap();
            seqs.push(op.origin_seq);
            assert!(
                storage::operations::verify_checksum(&op),
                "checksum must match payload"
            );
        }

        assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
        assert_eq!(
            storage::operations::last_local_seq(storage.pool(), "node-a")
                .await
                .unwrap(),
            5
        );

        // Sequences are per-node and independent.
        assert_eq!(
            storage::operations::last_local_seq(storage.pool(), "node-b")
                .await
                .unwrap(),
            0
        );
    });
}

#[test]
fn failed_local_append_does_not_consume_sequence() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();
        // Note: no dataset registered, so the FK constraint must reject the op.
        let err = storage::operations::append_local_operation(
            storage.pool(),
            storage::operations::LocalOperation {
                dataset_id: "missing".to_string(),
                entity_id: "x".to_string(),
                op_type: storage::operations::OperationType::Insert,
                schema_version: 1,
                payload: b"{}".to_vec(),
                origin_node_id: "node-a".to_string(),
            },
        )
        .await;
        assert!(err.is_err(), "append for unknown dataset must fail");

        // The transaction rolled back: no sequence was consumed and no row exists.
        assert_eq!(
            storage::operations::last_local_seq(storage.pool(), "node-a")
                .await
                .unwrap(),
            0
        );
        let stats = storage::operations::journal_stats(storage.pool())
            .await
            .unwrap();
        assert_eq!(stats.operations, 0);
    });
}

#[test]
fn integrity_check_and_wal_checkpoint_succeed() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let options = storage::OpenOptions {
            busy_timeout_ms: 1_000,
            integrity_check_on_startup: true,
        };
        let storage = Storage::open(dir.path(), &options).await.unwrap();
        assert_eq!(storage.integrity_check().await.unwrap(), "ok");
        storage.checkpoint_wal().await.unwrap();
    });
}

#[test]
fn compaction_respects_peer_checkpoints() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();
        register_orders(&storage).await;

        for i in 0..3 {
            storage::operations::append_local_operation(
                storage.pool(),
                storage::operations::LocalOperation {
                    dataset_id: "orders".to_string(),
                    entity_id: format!("order-{i}"),
                    op_type: storage::operations::OperationType::Insert,
                    schema_version: 1,
                    payload: b"{}".to_vec(),
                    origin_node_id: "node-a".to_string(),
                },
            )
            .await
            .unwrap();
        }

        let future = chrono::Utc::now() + chrono::Duration::minutes(1);
        let protected = vec![("store-b".to_string(), "orders".to_string())];

        // store-b has not acked anything -> nothing may be deleted.
        let plan = storage::operations::plan_compaction(storage.pool(), future, &protected)
            .await
            .unwrap();
        assert!(plan.is_empty(), "unacked peers must protect the journal");

        // After store-b acks through seq 2, only seq 3 stays protected.
        storage::checkpoints::update_checkpoint(
            storage.pool(),
            &storage::checkpoints::Checkpoint {
                peer_id: "store-b".to_string(),
                dataset_id: "orders".to_string(),
                last_acked_seq: 2,
                last_applied_seq: 2,
                updated_at: chrono::Utc::now(),
            },
        )
        .await
        .unwrap();
        let plan = storage::operations::plan_compaction(storage.pool(), future, &protected)
            .await
            .unwrap();
        assert_eq!(plan.len(), 2);

        let deleted = storage::operations::compact_journal(storage.pool(), future, &protected)
            .await
            .unwrap();
        assert_eq!(deleted, 2);
        let stats = storage::operations::journal_stats(storage.pool())
            .await
            .unwrap();
        assert_eq!(stats.operations, 1);

        // With no active peers, everything before the cutoff is eligible.
        let deleted = storage::operations::compact_journal(storage.pool(), future, &[])
            .await
            .unwrap();
        assert_eq!(deleted, 1);
    });
}

#[test]
fn dataset_registration_and_listing() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();

        let dataset = storage::datasets::Dataset {
            dataset_id: "orders".to_string(),
            schema_version: 1,
            conflict_policy: "single_writer".to_string(),
            table_allowlist: vec!["orders".to_string(), "order_items".to_string()],
            created_at: chrono::Utc::now(),
        };
        storage::datasets::register_dataset(storage.pool(), &dataset)
            .await
            .unwrap();

        let fetched = storage::datasets::get_dataset(storage.pool(), "orders")
            .await
            .unwrap()
            .expect("dataset exists");
        assert_eq!(fetched.table_allowlist, vec!["orders", "order_items"]);

        let all = storage::datasets::list_datasets(storage.pool())
            .await
            .unwrap();
        assert_eq!(all.len(), 1);

        assert!(storage::datasets::remove_dataset(storage.pool(), "orders")
            .await
            .unwrap());
        assert!(storage::datasets::list_datasets(storage.pool())
            .await
            .unwrap()
            .is_empty());
    });
}

#[test]
fn peer_authorization_lifecycle() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();

        storage::peers::authorize_peer(storage.pool(), "store-b", "orders", 3)
            .await
            .unwrap();
        assert!(
            storage::peers::is_authorized(storage.pool(), "store-b", "orders", 1)
                .await
                .unwrap()
        );

        storage::peers::revoke_peer(storage.pool(), "store-b", "orders")
            .await
            .unwrap();
        assert!(
            !storage::peers::is_authorized(storage.pool(), "store-b", "orders", 1)
                .await
                .unwrap()
        );

        let peers = storage::peers::list_peers(storage.pool()).await.unwrap();
        assert_eq!(peers.len(), 1);
        assert!(peers[0].revoked_at.is_some());
    });
}

#[test]
fn operation_journal_append_and_dedupe() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();

        let dataset = storage::datasets::Dataset {
            dataset_id: "orders".to_string(),
            schema_version: 1,
            conflict_policy: "single_writer".to_string(),
            table_allowlist: vec!["orders".to_string()],
            created_at: chrono::Utc::now(),
        };
        storage::datasets::register_dataset(storage.pool(), &dataset)
            .await
            .unwrap();

        let mut vv = storage::operations::VersionVector::new();
        vv.increment("node-a");

        let op = storage::operations::Operation {
            id: Uuid::now_v7(),
            dataset_id: "orders".to_string(),
            entity_id: "order-1".to_string(),
            origin_node_id: "node-a".to_string(),
            origin_seq: 1,
            op_type: storage::operations::OperationType::Insert,
            schema_version: 1,
            payload: b"{}".to_vec(),
            causal_metadata: vv,
            created_at: chrono::Utc::now(),
            checksum: [0u8; 32],
        };

        storage::operations::append_operation(storage.pool(), &op)
            .await
            .unwrap();

        let ops =
            storage::operations::get_operations_since(storage.pool(), "orders", "node-a", 0, 10)
                .await
                .unwrap();
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].entity_id, "order-1");

        // Applied-operation deduplication is idempotent.
        assert!(!storage::operations::is_applied(storage.pool(), op.id)
            .await
            .unwrap());
        storage::operations::mark_applied(storage.pool(), op.id, 1)
            .await
            .unwrap();
        storage::operations::mark_applied(storage.pool(), op.id, 1)
            .await
            .unwrap();
        assert!(storage::operations::is_applied(storage.pool(), op.id)
            .await
            .unwrap());
    });
}

fn remote_op(seq: u64, payload: &[u8]) -> storage::operations::Operation {
    let mut vv = storage::operations::VersionVector::new();
    vv.increment("node-b");
    storage::operations::Operation {
        id: Uuid::now_v7(),
        dataset_id: "orders".to_string(),
        entity_id: format!("order-{seq}"),
        origin_node_id: "node-b".to_string(),
        origin_seq: seq,
        op_type: storage::operations::OperationType::Insert,
        schema_version: 1,
        payload: payload.to_vec(),
        causal_metadata: vv,
        created_at: chrono::Utc::now(),
        checksum: storage::operations::compute_checksum(payload),
    }
}

#[test]
fn apply_remote_operation_dedupes_and_advances_checkpoint() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();
        register_orders(&storage).await;

        let op = remote_op(1, b"{\"n\":1}");

        let outcome = storage::operations::apply_remote_operation(storage.pool(), &op)
            .await
            .unwrap();
        assert_eq!(outcome, storage::operations::ApplyOutcome::Applied);

        // Re-applying the same operation is an idempotent no-op.
        let outcome = storage::operations::apply_remote_operation(storage.pool(), &op)
            .await
            .unwrap();
        assert_eq!(outcome, storage::operations::ApplyOutcome::Duplicate);

        // Exactly one journal row and one applied-operation row.
        let ops =
            storage::operations::get_operations_since(storage.pool(), "orders", "node-b", 0, 10)
                .await
                .unwrap();
        assert_eq!(ops.len(), 1);
        assert!(storage::operations::is_applied(storage.pool(), op.id)
            .await
            .unwrap());

        // The origin node's checkpoint advanced to the applied sequence.
        let checkpoint = storage::checkpoints::get_checkpoint(storage.pool(), "node-b", "orders")
            .await
            .unwrap()
            .expect("checkpoint created");
        assert_eq!(checkpoint.last_applied_seq, 1);
        assert_eq!(checkpoint.last_acked_seq, 0);
    });
}

#[test]
fn apply_remote_operation_advances_checkpoint_monotonically() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();
        register_orders(&storage).await;

        let newer = remote_op(2, b"{\"n\":2}");
        let older = remote_op(1, b"{\"n\":1}");

        storage::operations::apply_remote_operation(storage.pool(), &newer)
            .await
            .unwrap();
        // A late-arriving older operation must not move the checkpoint back.
        storage::operations::apply_remote_operation(storage.pool(), &older)
            .await
            .unwrap();

        let checkpoint = storage::checkpoints::get_checkpoint(storage.pool(), "node-b", "orders")
            .await
            .unwrap()
            .expect("checkpoint created");
        assert_eq!(checkpoint.last_applied_seq, 2);
    });
}

#[test]
fn apply_remote_operation_rejects_bad_checksum() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();
        register_orders(&storage).await;

        let mut op = remote_op(1, b"{\"n\":1}");
        op.checksum = [0u8; 32];

        let err = storage::operations::apply_remote_operation(storage.pool(), &op)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            syncfleet::error::SyncFleetError::Replication(_)
        ));

        // A rejected operation is not recorded as applied.
        assert!(!storage::operations::is_applied(storage.pool(), op.id)
            .await
            .unwrap());
    });
}

#[test]
fn apply_remote_operation_requires_registered_dataset() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();

        // No dataset registered: the journal foreign key rejects the insert.
        let op = remote_op(1, b"{\"n\":1}");
        assert!(
            storage::operations::apply_remote_operation(storage.pool(), &op)
                .await
                .is_err()
        );
    });
}

#[test]
fn concurrent_remote_write_records_a_conflict() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();
        register_orders(&storage).await;

        // Local write from node-a.
        storage::operations::append_local_operation(
            storage.pool(),
            storage::operations::LocalOperation {
                dataset_id: "orders".to_string(),
                entity_id: "order-1".to_string(),
                op_type: storage::operations::OperationType::Update,
                schema_version: 1,
                payload: b"{\"n\":1}".to_vec(),
                origin_node_id: "node-a".to_string(),
            },
        )
        .await
        .unwrap();

        // Remote write from node-b with a concurrent version vector.
        let payload = b"{\"n\":2}".to_vec();
        let mut causal = storage::operations::VersionVector::new();
        causal.increment("node-b");
        let remote = storage::operations::Operation {
            id: Uuid::new_v4(),
            dataset_id: "orders".to_string(),
            entity_id: "order-1".to_string(),
            origin_node_id: "node-b".to_string(),
            origin_seq: 1,
            op_type: storage::operations::OperationType::Update,
            schema_version: 1,
            payload: payload.clone(),
            causal_metadata: causal,
            created_at: chrono::Utc::now(),
            checksum: storage::operations::compute_checksum(&payload),
        };
        let outcome = storage::operations::apply_remote_operation(storage.pool(), &remote)
            .await
            .unwrap();
        assert_eq!(outcome, storage::operations::ApplyOutcome::Applied);

        let conflicts = storage::conflicts::list_conflicts(storage.pool())
            .await
            .unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].conflict_type, "concurrent_write");
        assert_eq!(conflicts[0].remote_operation_id, remote.id.to_string());
        assert!(conflicts[0].resolved_at.is_none());
    });
}

#[test]
fn remote_wins_policy_auto_resolves_concurrent_conflict() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();
        let dataset = storage::datasets::Dataset {
            dataset_id: "orders".to_string(),
            schema_version: 1,
            conflict_policy: "remote_wins".to_string(),
            table_allowlist: vec!["orders".to_string()],
            created_at: chrono::Utc::now(),
        };
        storage::datasets::register_dataset(storage.pool(), &dataset)
            .await
            .unwrap();

        storage::operations::append_local_operation(
            storage.pool(),
            storage::operations::LocalOperation {
                dataset_id: "orders".to_string(),
                entity_id: "order-1".to_string(),
                op_type: storage::operations::OperationType::Update,
                schema_version: 1,
                payload: b"{\"n\":1}".to_vec(),
                origin_node_id: "node-a".to_string(),
            },
        )
        .await
        .unwrap();

        let payload = b"{\"n\":2}".to_vec();
        let mut causal = storage::operations::VersionVector::new();
        causal.increment("node-b");
        let remote = storage::operations::Operation {
            id: Uuid::new_v4(),
            dataset_id: "orders".to_string(),
            entity_id: "order-1".to_string(),
            origin_node_id: "node-b".to_string(),
            origin_seq: 1,
            op_type: storage::operations::OperationType::Update,
            schema_version: 1,
            payload: payload.clone(),
            causal_metadata: causal,
            created_at: chrono::Utc::now(),
            checksum: storage::operations::compute_checksum(&payload),
        };
        storage::operations::apply_remote_operation(storage.pool(), &remote)
            .await
            .unwrap();

        let conflicts = storage::conflicts::list_conflicts(storage.pool())
            .await
            .unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].resolution.as_deref(), Some("remote"));
        assert!(conflicts[0].resolved_at.is_some());
    });
}

#[test]
fn local_wins_policy_rejects_remote_without_journaling_it() {
    block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::new(dir.path()).await.unwrap();
        let dataset = storage::datasets::Dataset {
            dataset_id: "orders".to_string(),
            schema_version: 1,
            conflict_policy: "local_wins".to_string(),
            table_allowlist: vec!["orders".to_string()],
            created_at: chrono::Utc::now(),
        };
        storage::datasets::register_dataset(storage.pool(), &dataset)
            .await
            .unwrap();

        storage::operations::append_local_operation(
            storage.pool(),
            storage::operations::LocalOperation {
                dataset_id: "orders".to_string(),
                entity_id: "order-1".to_string(),
                op_type: storage::operations::OperationType::Update,
                schema_version: 1,
                payload: b"{\"n\":1}".to_vec(),
                origin_node_id: "node-a".to_string(),
            },
        )
        .await
        .unwrap();

        let payload = b"{\"n\":2}".to_vec();
        let mut causal = storage::operations::VersionVector::new();
        causal.increment("node-b");
        let remote = storage::operations::Operation {
            id: Uuid::new_v4(),
            dataset_id: "orders".to_string(),
            entity_id: "order-1".to_string(),
            origin_node_id: "node-b".to_string(),
            origin_seq: 1,
            op_type: storage::operations::OperationType::Update,
            schema_version: 1,
            payload: payload.clone(),
            causal_metadata: causal,
            created_at: chrono::Utc::now(),
            checksum: storage::operations::compute_checksum(&payload),
        };
        let outcome = storage::operations::apply_remote_operation(storage.pool(), &remote)
            .await
            .unwrap();
        assert_eq!(outcome, storage::operations::ApplyOutcome::Rejected);

        let conflicts = storage::conflicts::list_conflicts(storage.pool())
            .await
            .unwrap();
        assert_eq!(conflicts[0].resolution.as_deref(), Some("local"));
        let stats = storage::operations::journal_stats(storage.pool())
            .await
            .unwrap();
        assert_eq!(stats.operations, 1, "remote op must not be journaled");
        // The remote op is marked applied so it is not redelivered.
        let stats_after = storage::operations::journal_stats(storage.pool())
            .await
            .unwrap();
        assert_eq!(stats_after.applied_operations, 1);
    });
}
