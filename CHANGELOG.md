# Changelog

All notable changes to SyncFleet will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
- Phase 3 (in progress): TLS 1.3 mutual-authentication transport scaffolding
  (`src/tls`, `src/net`), self-signed Ed25519 node certificates, a pinned
  trust-on-first-use trust store, single-use signed enrollment tokens, and the
  `003_peering.sql` migration
- Python wrapper package (`python/syncfleet`) with a `syncfleet` console script
  that forwards to the native executable; published to PyPI
- `scripts/release.py` to bump `Cargo.toml`, the Python wrapper version and the
  changelog together, and `scripts/build_vendor.py` to build platform wheels
  that bundle the native binary
- `.github/workflows/pypi.yml` builds per-OS wheels (Linux, macOS x86_64/arm64,
  Windows) and publishes to PyPI on `v*` tags via trusted publishing

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