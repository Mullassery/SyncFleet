# SyncFleet Consistency Model

## Guarantees Provided

### 1. Durability
- **Local writes**: Once `syncfleet write` returns success, the operation is durably persisted in SQLite (WAL mode, fsync) AND recorded in the operation journal.
- **Replication**: An operation is only acknowledged to the origin after the recipient has durably persisted it and updated its checkpoint.

### 2. Eventual Convergence
- **Assumption**: Network eventually delivers messages between authorized peers.
- **Guarantee**: All authorized peers that remain online will eventually apply the same set of operations for each dataset.
- **Caveat**: Convergence requires operational overlap (at least one common peer online).

### 3. Idempotency
- Operations carry globally unique IDs (UUIDv7).
- Applying an already-applied operation is a no-op (deduplication via `applied_operations` table).
- Safe to retry sends indefinitely.

### 4. Causal Ordering (Per-Origin)
- Operations from a single origin node are applied in sequence number order.
- Sequence numbers are monotonic per origin node per dataset.
- Gaps in sequence numbers are detected and reported.

## Guarantees NOT Provided

| Guarantee | Status | Reason |
|-----------|--------|--------|
| Strong consistency | ❌ | No consensus protocol; local writes don't wait for replication |
| Linearizability | ❌ | No global total order; per-origin ordering only |
| Exactly-once delivery | ❌ | At-least-once transport; idempotent application provides exactly-once *effect* |
| Global transactions | ❌ | No distributed transaction coordinator |
| Conflict-free replication | ❌ | Single-writer model; conflicts detected not auto-resolved |
| Bounded staleness | ❌ | No latency guarantees; depends on network |
| Read-your-writes (remote) | ❌ | Local reads see local writes immediately; remote reads see writes after sync |

## Conflict Handling

### Single-Writer Model (Default)
- One node designated as writer per dataset (or per partition key)
- Other peers: read-only replication
- Write attempts from non-writer peers:
  - Rejected at origin (local validation)
  - If somehow replicated: quarantined on recipient
- Admin can reassign writer role

### Conflict Detection
```
Conflict {
    operation_id: Uuid,
    conflict_type: ConflictType,
    local_operation: Option<Operation>,
    remote_operation: Operation,
    detected_at: DateTime<Utc>,
}
```

### Conflict Types
- `UnauthorizedWrite` - Non-writer attempted write
- `ConcurrentWrite` - Multiple writers (if policy allows)
- `SchemaMismatch` - Operation schema version incompatible
- `DuplicateEntity` - Insert with existing primary key
- `DeletedEntityModified` - Update/delete on tombstoned entity

### Conflict Resolution
- **Automatic**: None in MVP (all conflicts quarantined)
- **Manual**: Admin reviews via `syncfleet conflicts list`, applies resolution
- **Future**: LWW, CRDT, custom merge functions

## Deletion and Tombstones

### Soft Delete (Tombstone)
- Delete operations create tombstone records
- Tombstones replicate like normal operations
- Reads filter out tombstoned entities

### Garbage Collection
- Tombstones retained until **all** authorized peers have acknowledged
- Peer retirement: explicit `syncfleet peer retire` removes peer from GC calculation
- Configurable minimum retention (default: 30 days)

## Schema Evolution

### Compatibility Rules
- Schema version in every operation
- Only backward-compatible changes allowed (add optional fields)
- Breaking changes require new dataset registration
- Receiver validates schema_version <= local_schema_version

### Migration
- Admin registers new dataset version
- Data migration via application logic (outside SyncFleet)
- Dual-write period during transition

## Network Partition Behavior

### During Partition
- Local writes continue on each partition
- Each partition accumulates operations independently
- No cross-partition coordination

### On Healing
- Checkpoint exchange identifies missing operations
- Incremental sync transfers missing ranges
- Conflicts detected per conflict policy
- Convergence to single state (per policy)

### Split-Brain Risk
- **Single-writer**: Impossible (only one writer)
- **Multi-writer (future)**: Possible; requires conflict policy

## Assumptions

1. **Clock sync**: Not required for correctness; wall-clock only for diagnostics
2. **Message delivery**: Eventually, with possible duplication/reordering
3. **Peer honesty**: Authorized peers follow protocol (Byzantine not addressed)
4. **Storage reliability**: SQLite WAL + fsync provides durability
5. **Admin trust**: Enrollment/admin actions trusted

## Formal Properties (To Be Verified)

- **Convergence**: ∀ authorized peers p1, p2: eventually applied_ops(p1) = applied_ops(p2) for each dataset
- **No phantom reads**: If operation O is applied, all its causal dependencies are applied
- **No lost updates**: If origin node commits O, O will eventually reach all authorized peers
- **Idempotence**: apply(apply(O)) = apply(O)

## Testing Consistency

See [TESTING.md](TESTING.md) for fault-injection test scenarios.