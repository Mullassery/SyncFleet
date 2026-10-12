-- Initial SyncFleet schema
-- Version 1

-- Node identity and metadata
CREATE TABLE node_identity (
    node_id TEXT PRIMARY KEY,
    public_key BLOB NOT NULL,
    private_key_encrypted BLOB NOT NULL,
    created_at TEXT NOT NULL
);

-- Dataset registration
CREATE TABLE datasets (
    dataset_id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    conflict_policy TEXT NOT NULL,
    table_allowlist TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- Durable operation journal (append-only)
CREATE TABLE operation_journal (
    id TEXT PRIMARY KEY,
    dataset_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    origin_node_id TEXT NOT NULL,
    origin_seq INTEGER NOT NULL,
    op_type INTEGER NOT NULL,
    schema_version INTEGER NOT NULL,
    payload BLOB NOT NULL,
    causal_metadata TEXT NOT NULL,
    created_at TEXT NOT NULL,
    checksum BLOB NOT NULL,
    FOREIGN KEY (dataset_id) REFERENCES datasets(dataset_id)
);
CREATE INDEX idx_journal_dataset_seq ON operation_journal(dataset_id, origin_node_id, origin_seq);

-- Applied operation deduplication
CREATE TABLE applied_operations (
    operation_id TEXT PRIMARY KEY,
    applied_at TEXT NOT NULL,
    local_seq INTEGER NOT NULL
);

-- Per-peer checkpoints
CREATE TABLE replication_checkpoints (
    peer_id TEXT NOT NULL,
    dataset_id TEXT NOT NULL,
    last_acked_seq INTEGER NOT NULL,
    last_applied_seq INTEGER NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (peer_id, dataset_id),
    FOREIGN KEY (dataset_id) REFERENCES datasets(dataset_id)
);

-- Peer authorization
CREATE TABLE peer_authorizations (
    peer_id TEXT NOT NULL,
    dataset_id TEXT NOT NULL,
    permissions INTEGER NOT NULL,
    granted_at TEXT NOT NULL,
    revoked_at TEXT,
    PRIMARY KEY (peer_id, dataset_id)
);

-- Conflicts
CREATE TABLE conflicts (
    id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL,
    conflict_type TEXT NOT NULL,
    local_operation_id TEXT,
    remote_operation_id TEXT NOT NULL,
    detected_at TEXT NOT NULL,
    resolved_at TEXT,
    resolution TEXT
);
CREATE INDEX idx_conflicts_dataset ON conflicts(operation_id);

-- Schema version tracking
CREATE TABLE schema_version (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL
);

INSERT INTO schema_version (version, applied_at) VALUES (1, datetime('now'));