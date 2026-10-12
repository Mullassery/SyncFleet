# Architecture Decision Records

## ADR Format

Each decision record follows:
- **Status**: Proposed | Accepted | Superseded
- **Context**: Forces at play
- **Decision**: What we're doing
- **Consequences**: Trade-offs

---

## ADR-001: Rust as Primary Language
**Status**: Accepted
**Date**: 2024-01-15

### Context
Need memory safety, no runtime, good async ecosystem, strong crypto libraries, cross-compilation for edge devices.

### Decision
Use Rust (stable) for all core components.

### Consequences
- Steeper learning curve for contributors
- Excellent tooling (cargo, clippy, rust-analyzer)
- No GC pauses, predictable latency
- Mature async (tokio), crypto (rustls, ring), SQLite (sqlx/rusqlite)

---

## ADR-002: SQLite for Local Persistence
**Status**: Accepted
**Date**: 2024-01-15

### Context
Need embedded, zero-config, ACID, WAL mode, mature, portable.

### Decision
SQLite as sole local storage engine. Use WAL mode for concurrency.

### Consequences
- Single-file database (easy backup)
- No separate server process
- Limited write concurrency (one writer)
- WAL provides good read concurrency
- Use `sqlx` for compile-time checked queries

---

## ADR-003: TLS 1.3 for Transport Security
**Status**: Accepted
**Date**: 2024-01-15

### Context
Need mutual authentication, encryption, integrity. QUIC considered but adds complexity.

### Decision
TLS 1.3 with mutual authentication over TCP. Use `rustls` (no OpenSSL dependency).

### Consequences
- Standard, well-audited protocol
- Certificate management required
- No 0-RTT (replay risk)
- QUIC deferred to future optimization

---

## ADR-004: Logical Operation Replication (Not File Sync)
**Status**: Accepted
**Date**: 2024-01-15

### Context
File-based sync (rsync, Syncthing) doesn't work for live SQLite with WAL.

### Decision
Capture logical operations (insert/update/delete) at application layer. Durable journal with deduplication.

### Consequences
- Application must use SyncFleet write API
- External SQL writes bypass replication (documented)
- More complex than file sync
- Enables conflict detection, incremental sync, schema validation

---

## ADR-005: Single-Writer Conflict Model (MVP)
**Status**: Accepted
**Date**: 2024-01-15

### Context
Multi-writer requires CRDTs or complex conflict resolution. Not needed for initial use cases.

### Decision
Single designated writer per dataset. Other peers read-only. Conflicts quarantined for admin review.

### Consequences
- Simpler correctness argument
- No silent data loss
- Admin burden for conflict resolution
- Multi-writer deferred (CRDTs, LWW, custom merge)

---

## ADR-006: UUIDv7 for Operation IDs
**Status**: Accepted
**Date**: 2024-01-15

### Context
Need globally unique, sortable IDs for operations. Timestamp ordering useful for debugging.

### Decision
UUIDv7 (timestamp + random) for operation IDs. Per-node sequence for ordering.

### Consequences
- 128-bit IDs (larger than integers)
- Rough time ordering for diagnostics
- No central coordinator needed
- UUIDv7 supported in `uuid` crate v1.3+

---

## ADR-007: TOML for Configuration
**Status**: Accepted
**Date**: 2024-01-15

### Context
Human-readable, typed, comments, widely supported in Rust.

### Decision
TOML for config files. `config` crate for parsing with environment variable overrides.

### Consequences
- Familiar to Rust developers
- Supports nested structures
- Environment overrides for containers
- Schema validation via `serde`

---

## ADR-008: mDNS/DNS-SD for Discovery (Optional)
**Status**: Accepted
**Date**: 2024-01-15

### Context
Zero-config LAN discovery useful for edge. Not reliable across VLANs/VPNs.

### Decision
Optional mDNS discovery using `mdns-sd`. Static peer config as primary for production.

### Consequences
- Works on single LAN segment
- No multicast across routers
- Disabled by default for security
- Static config + WireGuard for production

---

## ADR-009: Ed25519 for Node Identity
**Status**: Accepted
**Date**: 2024-01-15

### Context
Need compact, fast, secure signatures for node identity. No RSA.

### Decision
Ed25519 via `ed25519-dalek`. Node ID = SHA-256(public_key)[:16] hex.

### Consequences
- 32-byte public keys, 64-byte signatures
- Fast verification, small certificates
- No patent issues
- Well-supported in `rustls`, `x509-cert`

---

## ADR-010: Append-Only Operation Journal
**Status**: Accepted
**Date**: 2024-01-15

### Context
Need durable, ordered, replayable operation log. SQLite WAL + append-only table.

### Decision
Operations never modified/deleted in journal. Compaction creates new journal, switches atomically.

### Consequences
- Monotonic growth until compaction
- Simple crash recovery (replay from checkpoint)
- Compaction requires coordination (no concurrent writers)
- Audit trail by default

---

## ADR-011: At-Least-Once Delivery with Idempotent Apply
**Status**: Accepted
**Date**: 2024-01-15

### Context
Network unreliable. Exactly-once requires consensus (too heavy).

### Decision
At-least-once transport. Deduplication via operation ID + applied_operations table.

### Consequences
- Simpler protocol
- Requires durable deduplication record
- Safe retries
- Exactly-once *effect* achieved

---

## ADR-012: Metrics via Prometheus
**Status**: Accepted
**Date**: 2024-01-15

### Context
Standard, pull-based, multi-dimensional, good tooling.

### Decision
Expose `/metrics` endpoint. Use `metrics` + `metrics-exporter-prometheus` crates.

### Consequences
- Standard ecosystem integration
- Cardinality management needed
- Push gateway not needed (pull model)
- Histograms for latency distributions