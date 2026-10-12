# SyncFleet Architecture

## System Overview

SyncFleet is a peer-to-peer synchronization engine built on the following core components:

1. **Storage Engine** - SQLite-backed durable operation journal with deduplication
2. **Replication Protocol** - Incremental, idempotent operation exchange
3. **Peer Manager** - Authentication, authorization, and connection lifecycle
4. **Transport Layer** - TLS 1.3 mutual authentication over TCP/QUIC
5. **Discovery** - mDNS/DNS-SD for LAN peer discovery (optional)
6. **CLI** - Configuration, daemon management, and diagnostics

## Data Model

### Node Identity
- Stable Ed25519 key pair per node
- Node ID = SHA-256(public_key) truncated to 16 bytes, hex-encoded
- Private keys stored with restricted permissions (0600)

### Datasets
- Logical replication units with explicit schema
- Registered with table allowlists and conflict policies
- Access controlled via peer authorization policies

### Operations
```
Operation {
    id: Uuid,                    // Globally unique
    dataset_id: String,
    entity_id: String,
    origin_node_id: NodeId,
    origin_seq: u64,             // Monotonic per-node sequence
    op_type: OperationType,      // Insert, Update, Delete
    schema_version: u32,
    payload: Vec<u8>,            // Serialized field changes
    causal_metadata: VersionVector,
    created_at: DateTime<Utc>,
    checksum: [u8; 32],          // SHA-256 of payload
}
```

### Checkpoints
Per-peer, per-dataset replication progress tracking:
```
Checkpoint {
    peer_id: NodeId,
    dataset_id: String,
    last_acked_seq: u64,
    last_applied_seq: u64,
    updated_at: DateTime<Utc>,
}
```

## Storage Schema

### SQLite Tables

```sql
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
    conflict_policy TEXT NOT NULL,  -- 'single_writer', 'lww', 'custom'
    table_allowlist TEXT NOT NULL,  -- JSON array of table names
    created_at TEXT NOT NULL
);

-- Durable operation journal (append-only)
CREATE TABLE operation_journal (
    id TEXT PRIMARY KEY,              -- UUID
    dataset_id TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    origin_node_id TEXT NOT NULL,
    origin_seq INTEGER NOT NULL,
    op_type INTEGER NOT NULL,         -- 1=insert, 2=update, 3=delete
    schema_version INTEGER NOT NULL,
    payload BLOB NOT NULL,
    causal_metadata BLOB NOT NULL,    -- Serialized VersionVector
    created_at TEXT NOT NULL,
    checksum BLOB NOT NULL,           -- SHA-256
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
    permissions INTEGER NOT NULL,     -- bitmask: 1=read, 2=write, 4=admin
    granted_at TEXT NOT NULL,
    revoked_at TEXT,
    PRIMARY KEY (peer_id, dataset_id)
);

-- Per-node monotonic sequence allocator (local origin_seq source)
CREATE TABLE node_local_state (
    node_id TEXT PRIMARY KEY,
    last_seq INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);
```

## Replication Protocol

### Message Types
```
SyncMsg {
    msg_id: Uuid,
    msg_type: MsgType,
    payload: Vec<u8>,
}

MsgType:
    - Hello { protocol_version, node_id, datasets[] }
    - HelloAck { accepted_datasets[], peer_checkpoints[] }
    - SyncRequest { dataset_id, from_seq, to_seq, max_ops }
    - SyncResponse { operations[], checkpoint }
    - Ack { dataset_id, acked_seq }
    - Conflict { operation_id, conflict_type, resolution }
    - Goodbye { reason }
```

### Sync Flow
1. **Connection**: TLS 1.3 mutual auth, verify peer identity
2. **Hello**: Exchange protocol version, node ID, dataset list
3. **Negotiation**: Intersect authorized datasets, compare checkpoints
4. **Sync**: Request missing operation ranges, stream operations
5. **Apply**: Validate, deduplicate, apply atomically with checkpoint update
6. **Ack**: Confirm durable persistence
7. **Loop**: Periodic sync, handle new operations

## Security Model

### Trust Boundaries
- **Node Identity**: Ed25519 key pair, certified via enrollment
- **Peer Authentication**: TLS 1.3 mutual auth with certificate pinning
- **Authorization**: Per-dataset ACLs, no implicit trust
- **Transport**: All traffic encrypted, bounded message sizes

### Enrollment Flow
1. Admin generates single-use enrollment token (cryptographically random, 256-bit)
2. Token delivered out-of-band (QR, file, secure channel)
3. New node presents token during first Hello
4. Admin node validates token, issues peer certificate
5. Token invalidated after use, expires after 24h

## Conflict Handling

### Single-Writer Model (Default)
- One designated writer per dataset (or per partition)
- Other peers: read-only replication
- Conflicts detected and quarantined for admin review

### Future: Multi-Writer Policies
- Last-Writer-Wins (LWW) for specific fields
- CRDT counters/sets for compatible types
- Application-defined merge functions

## Resource Bounds

| Resource | Limit | Enforcement |
|----------|-------|-------------|
| Max message size | 1 MB | Transport layer |
| Max in-flight requests | 32 | Peer manager |
| Max concurrent peers | 64 | Configurable |
| Journal retention | 30 days / 1 GB | Compaction job |
| Snapshot size | 500 MB | Chunked transfer |

## Failure Modes

| Scenario | Behavior |
|----------|----------|
| Network partition | Local writes continue, sync resumes on reconnect |
| Peer crash mid-sync | Checkpoint-based resume, no duplicate application |
| Disk full | Backpressure, reject new writes, alert |
| Schema mismatch | Explicit error, quarantine operations |
| Corrupted journal | Integrity check on startup, recovery from checkpoint |
| Revoked peer | Immediate connection drop, reject future connections |

## Configuration

See `config/syncfleet.example.toml` for full configuration reference.