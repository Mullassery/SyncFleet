# SyncFleet Testing Strategy

## Test Levels

### 1. Unit Tests (Target: >80% coverage)
- Serialization/deserialization round-trips
- Protocol version validation
- Sequence number handling
- Deduplication logic
- Authorization checks
- Dataset policy enforcement
- Schema compatibility rules
- Conflict detection
- Checkpoint logic
- Tombstone lifecycle
- Cryptographic operations

### 2. Integration Tests
- Two-node synchronization
- Multi-node replication (3-5 nodes)
- Authentication and revocation
- Offline catch-up
- Interrupted transfers
- Process restart recovery
- Snapshot bootstrap
- Schema mismatch handling
- Database corruption handling
- Concurrent writes (single-writer)
- Large dataset sync (10k+ operations)

### 3. Fault Injection Tests
Using a test harness that can:
- Drop messages
- Duplicate messages
- Delay/reorder delivery
- Abrupt process termination (SIGKILL)
- Disk-full simulation
- Connection interruption
- Stale peer simulation
- Corrupted payload injection
- Clock skew simulation
- Repeated reconnection storms
- Malformed/excessive data from peer

### 4. Property-Based Tests
- Operation ID uniqueness
- Checkpoint monotonicity
- Deduplication idempotence
- Sequence gap detection
- Convergence properties

### 5. Chaos/Long-Running Tests
- 24-hour soak test with periodic disconnections
- Network partition simulation
- Resource exhaustion scenarios
- Upgrade/downgrade compatibility

## Test Infrastructure

### Test Harness
```rust
// tests/harness.rs
pub struct TestCluster {
    nodes: Vec<TestNode>,
    network: NetworkSimulator,
}

pub struct NetworkSimulator {
    // Control: latency, packet loss, partition, reorder
}

impl TestCluster {
    pub async fn spawn(n: usize) -> Self;
    pub async fn partition(&mut self, groups: Vec<Vec<usize>>);
    pub async fn heal(&mut self);
    pub async fn inject_fault(&mut self, fault: Fault);
    pub fn node(&self, idx: usize) -> &TestNode;
}
```

### Fixtures
- Pre-populated databases
- Valid/invalid certificates
- Enrollment tokens
- Schema definitions
- Operation sequences

## Test Scenarios

### Happy Path
1. **Basic Sync**: Node A writes → sync → Node B reads
2. **Multi-peer**: A→B, A→C, verify all converge
3. **Reconnect**: Disconnect, modify both sides, reconnect, verify merge
4. **Restart**: Kill processes, restart, verify no data loss

### Error Paths
1. **Unauthorized peer**: Connection rejected
2. **Revoked peer**: Previously authorized, now rejected
3. **Schema mismatch**: Explicit error, no corruption
4. **Corrupted journal**: Detected on startup, recovery
5. **Disk full**: Graceful degradation, alert
6. **Network partition**: Local writes continue, sync on heal

### Concurrency
1. **Concurrent writes**: Single-writer enforced
2. **Concurrent syncs**: Multiple peers syncing same dataset
3. **Rapid writes**: Burst handling, queue bounds

### Fault Injection Matrix

| Fault | Injected At | Expected Behavior |
|-------|-------------|-------------------|
| Drop SYNC_REQUEST | Network | Retry with backoff |
| Duplicate SYNC_RESPONSE | Network | Idempotent apply, single ack |
| Delay HELLO_ACK | Network | Timeout, retry |
| Kill during apply | Process | On restart: resume from checkpoint |
| Corrupt operation payload | Peer send | Reject, log, continue |
| Send stale checkpoint | Peer | Detect gap, request full range |
| Clock skew +1hr | Node | No correctness impact |
| Rapid connect/disconnect | Network | Backoff, no resource leak |

## CI Pipeline

```yaml
# .github/workflows/test.yml
jobs:
  unit:
    runs-on: ubuntu-latest
    steps:
      - cargo test --lib --workspace
      - cargo test --doc --workspace

  integration:
    runs-on: ubuntu-latest
    steps:
      - cargo test --test integration --workspace

  fault_injection:
    runs-on: ubuntu-latest
    steps:
      - cargo test --test fault_injection --workspace

  property:
    runs-on: ubuntu-latest
    steps:
      - cargo test --test property --workspace --features proptest

  quality:
    runs-on: ubuntu-latest
    steps:
      - cargo fmt --check --all
      - cargo clippy --all-targets --all-features -- -D warnings
      - cargo audit
      - cargo deny check
```

## Benchmarks

Run on: GitHub Actions runner (for CI) + Raspberry Pi 4 (for ARM)

| Benchmark | Metric | Target |
|-----------|--------|--------|
| Idle memory | RSS | < 50 MB |
| Idle CPU | % | < 1% |
| Connection setup | ms | < 100ms |
| Small op latency (1KB) | ms | < 10ms |
| Throughput (1KB ops) | ops/sec | > 1000 |
| Reconnection time | ms | < 500ms |
| Restart recovery | ms | < 1s |
| Journal growth (1M ops) | MB | < 500 MB |

## Test Data Management

- Deterministic seeds for reproducibility
- Golden files for serialization format
- Schema evolution test vectors
- Large dataset generators (configurable size)

## Coverage Tracking

- `cargo llvm-cov` for line/branch coverage
- Minimum thresholds in CI
- Exclude generated code, test harness

## Release Gates

All must pass:
- [ ] Unit tests: 100% pass
- [ ] Integration tests: 100% pass
- [ ] Fault injection: 100% pass
- [ ] Property tests: 100% pass
- [ ] `cargo fmt --check`: pass
- [ ] `cargo clippy`: zero warnings
- [ ] `cargo audit`: zero vulnerabilities
- [ ] `cargo deny`: policy pass
- [ ] Benchmarks: no regression >10%