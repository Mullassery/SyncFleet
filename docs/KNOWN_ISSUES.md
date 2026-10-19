# Known Issues and Limitations

## Current Limitations (MVP)

### 0. Networking & Replication Status (Phases 3–4 Implemented)
- **Status**: Resolved in v0.2.0.
- **Details**: Mutual-TLS peering (`src/net/mod.rs`), signed single-use enrollment tokens (`src/tls/enrollment.rs`), incremental pull/push replication (`src/replication/mod.rs`), mDNS peer discovery (`src/discovery/mod.rs`), background `sync_loop`, and disk-backed disconnection spooling with priority ring-buffer eviction (`src/storage::spool`) are now implemented.
- **Remaining**: Automated schema migration, mDNS interface pinning, and daemon stop/restart PID management. Field-level merge (`merge_fields`) now ships as a deterministic LWW-register policy over JSON fields.

### 1. Single-Writer Only
- **Issue**: Only one writer per dataset supported
- **Impact**: No concurrent writes from multiple peers
- **Workaround**: Designate writer per dataset; use partitioning for scale
- **Future**: Full multi-writer CRDT; field-level `merge_fields` LWW merge is available, but datasets still designate a writer for other policies

### 2. No Cross-VLAN Discovery
- **Issue**: mDNS doesn't traverse routers
- **Impact**: Discovery only works on single LAN segment
- **Workaround**: Static peer config + WireGuard/VPN
- **Future**: Gossip-based membership, relay nodes

### 3. Schema Changes Require Manual Migration
- **Issue**: No automatic schema migration
- **Impact**: Breaking changes require new dataset + dual-write
- **Workaround**: Application-level migration scripts
- **Future**: Schema registry, automated backward-compatible migrations

### 4. No Built-in Metrics Persistence
- **Issue**: Metrics only exposed via `/metrics` endpoint
- **Impact**: Need external Prometheus/Grafana for history
- **Workaround**: Deploy Prometheus stack
- **Future**: Optional embedded time-series (e.g., Redb)

### 5. Limited Windows Support
- **Issue**: Developed/tested on Linux/macOS
- **Impact**: Windows may have path, permission, or mDNS issues
- **Workaround**: WSL2 or Linux containers
- **Future**: CI on Windows, fix platform-specific issues

### 6. No GUI/Web UI
- **Issue**: CLI-only interface
- **Impact**: Operational tasks require CLI knowledge
- **Workaround**: Scripts, wrapper tools
- **Future**: Optional web dashboard

### 7. Enrollment Token Delivery Manual
- **Issue**: No automated token distribution
- **Impact**: Admin must securely deliver tokens out-of-band
- **Workaround**: QR codes, encrypted files, SSH
- **Future**: CA-signed certificates, ACME integration

---

## Known Defects

### DEF-001: Journal Compaction Not Implemented
- **Severity**: Medium
- **Status**: Resolved (guarded)
- **Description**: Journal grows unbounded until manual VACUUM
- **Resolution**: `syncfleet journal compact` and `doctor --repair` remove
  entries older than the retention window **only** when every active peer has
  acknowledged past them. Entries still needed by a peer are retained. Full
  automated snapshot-based reclamation remains Phase 7.
- **Mitigation**: `syncfleet journal stats`, `journal compact --dry-run`

### DEF-002: Snapshot Bootstrap Not Implemented
- **Severity**: High
- **Status**: Open
- **Description**: New peers must replay full journal
- **Mitigation**: Keep journal retention high; implement snapshots ASAP

### DEF-003: Peer Revocation Not Propagated
- **Severity**: Medium
- **Status**: Open
- **Description**: Revoked peer rejected locally but not pushed to other peers
- **Mitigation**: Manual revocation on all nodes

### DEF-004: Clock Skew Not Handled in Diagnostics
- **Severity**: Low
- **Status**: Open
- **Description**: Wall-clock timestamps in logs may be misleading
- **Mitigation**: Use monotonic/relative times in diagnostics

### DEF-005: Large Operation Batching Not Optimized
- **Severity**: Low
- **Status**: Open
- **Description**: Operations sent individually, not batched
- **Mitigation**: Acceptable for MVP; optimize later

---

## Technical Debt

| ID | Component | Description | Priority |
|----|-----------|-------------|----------|
| TD-001 | Storage | ~~Migrate from `rusqlite` to `sqlx`~~ (done: `sqlx` used throughout) | Done |
| TD-002 | Protocol | Define protobuf/flatbuffers wire format instead of ad-hoc JSON | Low |
| TD-003 | Crypto | Add post-quantum hybrid KEM (ML-KEM + X25519) | Low |
| TD-004 | Testing | Add chaos testing framework integration | Medium |
| TD-005 | CLI | Add shell completions (bash, zsh, fish) | Low |
| TD-006 | Docs | Generate OpenAPI spec for REST API (future) | Low |
| TD-007 | Journal | ~~Make local mutation + journal append atomic and auto-allocate per-node sequence numbers~~ (done: `append_local_operation` + `node_local_state`) | Done |
| TD-008 | Storage | ~~Crash-safe recovery / integrity verification on startup~~ (done: per-connection WAL/foreign_keys/busy_timeout via `SqliteConnectOptions`; `integrity_check_on_startup`) | Done |
| TD-009 | Replication | ~~Remote-operation application (`apply_remote_operation`) not yet implemented; `append_operation` still takes an explicit sequence~~ (done: `apply_remote_operation` validates checksum, dedupes, stores, and advances the checkpoint atomically; idempotent on replay) | Done |
| TD-010 | Storage | Snapshot-based journal reclamation (delete ops even when no peer has acked, by bootstrapping peers from a snapshot) — Phase 7 | Medium |

---

## Deferred Features

These are explicitly NOT in MVP scope:

1. **Full multi-writer CRDTs** - Requires significant design/verification; the field-level `merge_fields` LWW policy is shipped
2. **Consensus protocol** (Raft) - Not needed for current use cases
3. **Global transactions** - Out of scope
4. **Multi-datacenter geo-replication** - Single region only
5. **Built-in key management UI** - CLI only
6. **Real-time subscriptions** - Poll-based sync only
7. **Conflict auto-resolution** - Manual only
8. **End-to-end encryption (application layer)** - Transport encryption only
9. **Audit log export** - Structured logs only
10. **Multi-tenancy** - Single tenant per node

---

## Platform-Specific Issues

### Linux
- [ ] systemd service file needs review
- [ ] AppArmor/SELinux profiles needed
- [ ] Capability bounding (CAP_NET_BIND_SERVICE only)

### macOS
- [ ] launchd plist for service management
- [ ] mDNSResponder integration
- [ ] Keychain integration for key storage

### Raspberry Pi / ARM
- [ ] Test on Pi 4 (64-bit) and Pi Zero 2W
- [ ] Optimize for memory-constrained (256MB)
- [ ] SD card wear considerations (journal on tmpfs?)

### Containers
- [ ] Non-root user in Dockerfile
- [ ] Read-only rootfs compatibility
- [ ] Health check endpoint

---

## Security Issues Tracking

| ID | Severity | Component | Description | Status |
|----|----------|-----------|-------------|--------|
| SEC-001 | Info | Keys | No HSM/TPM support | Deferred |
| SEC-002 | Info | Audit | No tamper-evident audit log | Deferred |
| SEC-003 | Low | Config | Config file may contain sensitive paths | Documented |
| SEC-004 | Medium | Enrollment | Token delivery channel not enforced | Documented |

---

## Upgrade/Compatibility Notes

### Protocol Versioning
- Wire protocol version in Hello message
- Backward compatibility within major version
- Breaking changes = new major version

### Database Migrations
- `sqlx` migrations applied on startup
- Down migrations not supported (backup first)
- Schema version in `datasets` table

### Configuration Changes
- Most changes require restart
- Hot-reload: log level, peer list (future)