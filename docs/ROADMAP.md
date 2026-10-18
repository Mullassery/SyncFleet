# SyncFleet Implementation Roadmap

## Phase 1: Project Foundation and CLI (Current)

### Milestone 1.1 - Project Setup ✓
- [x] Initialize Rust project
- [x] Set up workspace structure
- [x] Add core dependencies
- [x] Create documentation skeleton

### Milestone 1.2 - CLI Framework ✓
- [x] Implement CLI with clap (derive API)
- [x] Commands: init, status, config validate, dataset list, peer list, daemon start/stop
- [x] Configuration file parsing (TOML)
- [x] Structured logging with tracing
- [ ] Graceful shutdown handling (deferred with daemon, Phase 8)

### Milestone 1.3 - Node Identity ✓
- [x] Ed25519 key generation (using `ed25519-dalek`)
- [x] Secure private key storage (AES-256-GCM at rest, optional passphrase, 0600 perms)
- [x] Node ID derivation (SHA-256 of public key)
- [x] `syncfleet init` implementation
- [x] X25519 key agreement key pair

### Milestone 1.4 - Configuration System ✓
- [x] TOML config schema with validation
- [x] Config file discovery (--config, default paths)
- [x] Environment variable overrides (`SYNCFLEET_<SECTION>__<KEY>`)
- [x] `syncfleet config validate` command

## Phase 2: Storage Engine and Durable Operation Journal (In Progress)

### Milestone 2.1 - SQLite Backend ✓
- [x] Database initialization and migrations (using `sqlx`)
- [x] Connection pooling
- [x] Schema as defined in ARCHITECTURE.md
- [x] WAL mode, foreign keys, busy timeout applied per connection

### Milestone 2.2 - Operation Journal ✓
- [x] Durable append-only journal
- [x] Transactional write API (sequence allocation + journal entry atomic)
- [x] Operation ID generation (UUIDv7 for time-ordering)
- [x] Per-node sequence allocation (gap-free; rolled back on failure)
- [x] Payload checksums (SHA-256) computed on append and verified on read

### Milestone 2.3 - Deduplication and Checkpoints ✓
- [x] Applied operations table with unique constraint
- [x] Per-peer, per-dataset checkpoints
- [x] Crash-safe recovery: WAL checkpoint + startup integrity check
- [x] Journal retention and compaction (peer-ack-guarded; `syncfleet journal compact`)

### Milestone 2.4 - Dataset Management ✓ (local)
- [x] Dataset registration API
- [x] Schema version tracking
- [x] Table allowlist storage
- [x] Conflict policy configuration

## Phase 3: Secure Peer Communication

### Milestone 3.1 - TLS Transport
- [x] TLS 1.3 with mutual authentication (using `rustls`)
- [x] Certificate generation and management
- [x] Certificate pinning / trust-on-first-use
- [x] Connection timeouts and bounded messages

### Milestone 3.2 - Peer Enrollment
- [x] Single-use enrollment token generation
- [x] Token validation and exchange
- [x] Peer certificate issuance
- [x] `syncfleet peer pair` command
- [x] `syncfleet peer token|redeem|export-cert|import-cert|certs` commands

### Milestone 3.3 - Authorization
- [x] Per-dataset ACLs (read/write/admin)
- [x] Authorization enforced on the peering handshake and per-operation
  during replication (`replication::ensure_authorized`)
- [x] Peer revocation (`syncfleet peer revoke`)
- [x] Revocation checking on connection

## Phase 4: Incremental Replication

### Milestone 4.1 - Sync Protocol
- [x] Hello/HelloAck handshake (reuses the peering `Hello`/`HelloReply`)
- [x] Checkpoint exchange (version-vector `Have`/`Ops` messages)
- [x] Missing operation range identification
- [x] Bounded operation streaming (`replication.max_ops_per_response`)

### Milestone 4.2 - Idempotent Application
- [x] Operation validation (checksum, schema, auth)
- [x] Deduplication via applied_operations table
- [x] Atomic apply + checkpoint update
- [x] Acknowledgment protocol (`Ack { applied }`)

### Milestone 4.3 - Resilience
- [x] Connection interruption handling (bounded timeouts on every read/write;
  an interrupted session simply resumes on the next one)
- [x] Automatic reconnection with backoff (daemon sync loop; see `sync_loop`)
- [x] Resume from last checkpoint (re-sync is idempotent and vector-based)
- [x] Snapshot/bootstrap for new peers (full journal replay, batched and looped
  to completion in a single session — no compacted-table snapshot yet)

### Milestone 4.4 - End-to-End Test
- [x] Two-node integration test
- [x] Offline modification test (ops staged locally before connecting)
- [x] Restart persistence test (`state_survives_restart`)
- [ ] Conflict detection test (Phase 6)

## Phase 5: Discovery and Multi-Peer Topology

### Milestone 5.1 - mDNS Discovery
- [x] Service advertisement (`_syncfleet._tcp.local.` via `mdns-sd`, TXT node
  id / name / version, `enable_addr_auto`)
- [x] Peer discovery and connection (resolved peers are fed into the sync loop
  and only dialled once they appear in `peer_authorizations`)
- [ ] Configuration controls (enable/disable via `discovery.enabled` is wired;
  `discovery.interface` binding is not yet applied)

### Milestone 5.2 - Static Peers
- [x] Configured peer list (`[[peers]]` in config)
- [x] Connection management (one background task per peer in `sync_loop`)
- [ ] Health checking (peers are retried with backoff; no explicit liveness probe)

### Milestone 5.3 - Topology Management
- [ ] Connection pooling
- [ ] Peer state machine
- [ ] Concurrency bounds

## Phase 6: Conflict Handling and Consistency

### Milestone 6.1 - Single-Writer Enforcement
- [ ] Writer designation per dataset
- [ ] Unauthorized write detection
- [x] Concurrent-write detection (version vectors) and conflict quarantine

### Milestone 6.2 - Conflict Inspection
- [x] `syncfleet conflicts list|show|resolve` commands
- [x] Conflict metadata recording
- [x] Manual admin resolution workflow
- [ ] Automatic policy-based resolution (LWW/CRDT)

## Phase 7: Snapshots, Backup, and Recovery

### Milestone 7.1 - Snapshots
- [ ] Consistent SQLite backup (VACUUM INTO or page streaming)
- [ ] Integrity verification (checksums)
- [ ] Atomic snapshot promotion

### Milestone 7.2 - Recovery
- [ ] Bootstrap from snapshot
- [ ] Journal replay from checkpoint
- [ ] Schema validation on restore

## Phase 8: Observability and Operations

### Milestone 8.1 - Metrics
- [ ] Prometheus metrics endpoint
- [ ] Key metrics: replication lag, peer state, queue depths, errors

### Milestone 8.2 - Diagnostics
- [ ] `syncfleet status` - node health
- [ ] `syncfleet peer list` - peer details
- [ ] `syncfleet sync status` - replication progress
- [ ] `syncfleet doctor` - safe diagnostics

### Milestone 8.3 - Deployment
- [ ] systemd service file
- [ ] Docker Compose example
- [ ] Log rotation configuration

## Optional Extensions (Post-MVP)

- [ ] MQTT adapter for IoT ingestion
- [ ] Developer tooling integrations
- [ ] CRDT support for multi-writer datasets
- [ ] Gossip-based membership for larger clusters

## Status Legend

- ✓ Completed
- 🔄 In Progress
- ⏳ Pending
- ✗ Blocked