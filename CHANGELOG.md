# Changelog

All notable changes to SyncFleet will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.0] - 2026-10-19

### Added
- `syncfleet dataset schema <dataset> <version>` updates the local dataset's
  schema version after an operator-run migration, so operations built against
  the new version are accepted. This does not migrate data.
- Storage test covers the update and missing-dataset cases.

## [0.6.0] - 2026-10-19

### Added
- Schema-version guard: a remote operation whose `schema_version` differs from
  the local dataset is rejected, recorded as a `schema_version_mismatch`
  conflict, and its checkpoint is advanced so it is not redelivered. This is
  detection, not migration; the operator updates the dataset or republishes.
- Storage test covers the mismatch path.

## [0.5.0] - 2026-10-19

### Added
- `conflict_policy = "local_wins"` rejects a concurrent remote operation
  without journaling it, records the conflict as resolved `local`, and still
  advances the checkpoint so the peer does not redeliver it.
- `conflict_policy = "last_write_wins"` compares operation timestamps and
  routes to the remote or local path accordingly.
- Storage test covers the `local_wins` rejection path.

## [0.4.0] - 2026-10-19

### Added
- `conflict_policy = "remote_wins"` (or `"remote"`) auto-resolves the conflict
  record in favour of the applied remote operation (`resolved_at` set,
  `resolution = "remote"`). Other policies still record an open conflict for
  manual resolution. `local_wins` is intentionally not implemented because the
  engine does not materialize local row state.
- Storage test covers the auto-resolution path.

## [0.3.0] - 2026-10-19

### Added
- Concurrent-write detection during `apply_remote_operation`: when a remote
  operation's version vector is concurrent with the latest journal entry for the
  same dataset/entity from another origin, a `concurrent_write` row is recorded
  in `conflicts`. The operation is still applied; operators resolve it with the
  existing `syncfleet conflicts list|show|resolve` commands. Automatic
  policy-based resolution is not implemented.
- Test: `concurrent_remote_write_records_a_conflict` (storage suite).

## [Unreleased]

### Added
- Initial project structure and documentation
- Architecture, security, consistency, threat model docs
- Roadmap with phased implementation plan
- Testing strategy and operations guide
- Architecture decision records
- Working CLI: `init`, `status`, `config validate|show|example`,
  `dataset list|register|show|remove`, `peer list|pair|show|revoke`,
  `sync status`, `conflicts list|show|resolve`, `doctor`
- Ed25519 + X25519 node identity with optional AES-256-GCM passphrase
  encryption and `0600` key file permissions
- SQLite storage layer (`sqlx`) with migrations, operation journal,
  applied-operation dedupe, per-peer checkpoints, dataset registry,
  peer authorization, and conflict records
- Environment-variable configuration overrides
  (`SYNCFLEET_<SECTION>__<KEY>`)
- Library target plus integration tests for identity, storage, and CLI
  (20 tests)
- Phase 2 completion: atomic local operation append with gap-free per-node
  sequence allocation (`node_local_state`), SHA-256 payload checksums,
  `syncfleet journal stats|compact`, crash-safe startup (WAL +
  `integrity_check_on_startup`), and peer-ack-guarded journal compaction
- `doctor --full` reports journal/integrity state; `doctor --repair` performs a
  WAL checkpoint and safe compaction
- Phase 2 completed: `apply_remote_operation` applies a peer operation
  atomically — checksum validation, idempotent dedupe by operation id, journal
  storage, and monotonic checkpoint advance (resolves TD-009)
- Phase 3 (in progress): TLS 1.3 mutual-authentication transport scaffolding
  (`src/tls`, `src/net`), self-signed Ed25519 node certificates, a pinned
  trust-on-first-use trust store, single-use signed enrollment tokens, and the
  `003_peering.sql` migration
- Phase 3 CLI wiring: `init` now generates the node's TLS material; new
  `peer token|redeem|export-cert|import-cert|certs` commands issue/redeem
  enrollment tokens, pin peer certificates, and authorize peers
- Integration tests for TLS (`tests/tls.rs`: cert round-trip, trust store,
  enrollment sign/verify/tamper/expiry) and peering (`tests/net.rs`: loopback
  mutual-TLS exchange, unauthorized/mismatched/unpinned rejection); CLI tests
  cover the enrollment-token flow (38 tests total)
- Python wrapper package (`python/syncfleet`) with a `syncfleet` console script
  that forwards to the native executable; published to PyPI
- `scripts/release.py` to bump `Cargo.toml`, the Python wrapper version and the
  changelog together, and `scripts/build_vendor.py` to build platform wheels
  that bundle the native binary
- `.github/workflows/pypi.yml` builds per-OS wheels (Linux, macOS x86_64/arm64,
  Windows) and publishes to PyPI on `v*` tags using the `PYPI_API_TOKEN`
  repository secret

### Changed
- Configuration now loads with developer-friendly defaults
  (`~/.local/share/syncfleet`) and only requires TLS material when actually
  serving network traffic (`validate_for_network`)
- Example config converted to a fully commented, all-defaults template

### Fixed
- Identity encryption previously generated a fresh nonce on every decryption,
  making passphrase-protected keys undecryptable; the nonce is now stored
  alongside the ciphertext (AES-256-GCM nonce ‖ ciphertext)
- Environment-variable config overrides were silently ignored due to an
  incorrect prefix/separator configuration
- Enrollment tokens are now signed with the certificate's Ed25519 key (the key
  the certificate certifies) instead of the separate node-identity key, so the
  embedded signature actually verifies against the embedded certificate
- rustls now builds with a single crypto provider (disabled default
  `aws-lc-rs`, kept `ring`) so TLS configuration no longer panics with
  "could not automatically determine the process-level CryptoProvider"

### Security
- Passphrase-derived key via HKDF-SHA256; per-secret random 96-bit nonce
  persisted with ciphertext

### Deprecated
- N/A

### Removed
- N/A

---

## [0.1.0] - TBD

### Added
- Project initialization
- Core documentation

### Changed
- N/A

### Deprecated
- N/A

### Removed
- N/A

### Fixed
- N/A

### Security
- N/A

---

## Release Process

1. Update version in `Cargo.toml`
2. Update this changelog
3. Run full test suite
4. Create git tag: `git tag v0.1.0`
5. Push tag: `git push origin v0.1.0`
6. GitHub Actions builds and publishes release