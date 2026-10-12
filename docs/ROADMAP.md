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
- [x] Authorization enforced on the peering handshake (per-operation
  enforcement lands with replication in Phase 4)
- [x] Peer revocation (`syncfleet peer revoke`)
- [x] Revocation checking on connection

## Phase 4: Incremental Replication

### Milestone 4.1 - Sync Protocol
- [ ] Hello/HelloAck handshake
- [ ] Checkpoint exchange
- [ ] Missing operation range identification
- [ ] Bounded operation streaming

### Milestone 4.2 - Idempotent Application
- [ ] Operation validation (checksum, schema, auth)
- [ ] Deduplication via applied_operations table
- [ ] Atomic apply + checkpoint update
- [ ] Acknowledgment protocol

### Milestone 4.3 - Resilience
- [ ] Connection interruption handling
- [ ] Automatic reconnection with backoff
- [ ] Resume from last checkpoint
- [ ] Snapshot/bootstrap for new peers

### Milestone 4.4 - End-to-End Test
- [ ] Two-node integration test
- [ ] Offline modification test
- [ ] Restart persistence test
- [ ] Conflict detection test

## Phase 5: Discovery and Multi-Peer Topology

### Milestone 5.1 - mDNS Discovery
- [ ] Service advertisement (using `mdns-sd`)
- [ ] Peer discovery and connection
- [ ] Configuration controls (enable/disable, interface binding)

### Milestone 5.2 - Static Peers
- [ ] Configured peer list
- [ ] Connection management
- [ ] Health checking

### Milestone 5.3 - Topology Management
- [ ] Connection pooling
- [ ] Peer state machine
- [ ] Concurrency bounds

## Phase 6: Conflict Handling and Consistency

### Milestone 6.1 - Single-Writer Enforcement
- [ ] Writer designation per dataset
- [ ] Unauthorized write detection
- [ ] Conflict quarantine

### Milestone 6.2 - Conflict Inspection
- [ ] `syncfleet conflicts list` command
- [ ] Conflict metadata recording
- [ ] Admin resolution workflow

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