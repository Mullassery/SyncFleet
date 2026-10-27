# SyncFleet — Systems Design Audit

Audit date: 2026-10-27
Repository: `SyncFleet`
Revision observed: commit dated 2026-10-25, working tree clean
Version: 0.7.0
License: MIT
MSRV: Rust 1.89

## Overview

SyncFleet is a local-first, peer-to-peer synchronization engine for selected
SQLite datasets. Each node owns a SQLite database; operators register named
datasets (table allowlists), authorize peers per dataset, and nodes reconcile
incremental operations over mutual-TLS 1.3 connections. There is no required
cloud service, broker, or central control plane. Trust is explicit: a peer must
be enrolled and its certificate pinned before it can connect.

The project is at “Phase 4 — Incremental replication.” Node identity, storage,
datasets/peers, TLS certificates, pinned trust, mTLS peering, signed enrollment
tokens, incremental pull/push replication, per-operation authorization,
journaling, daemon serving/sync loops, mDNS discovery, conflict recording, and
journal compaction are implemented. Full CRDT support, mDNS interface binding,
and automated schema migration remain open.

## Architecture

```text
CLI (src/cli)
  ├── config (src/config)
  ├── identity (src/identity)
  ├── storage (src/storage: SQLite, journal, datasets, peers, spool)
  ├── tls (src/tls: cert, store, enrollment)
  ├── net (src/net: mTLS peering/transport)
  ├── replication (src/replication: version vectors, op streaming)
  ├── sync_loop (background sync per peer)
  ├── discovery (src/discovery: mDNS)
  └── daemon (serve + sync + shutdown)
```

Data flow:

1. `init` creates an Ed25519/X25519 identity, SQLite DB, and self-signed TLS
   certificate.
2. `dataset register` records a table allowlist and conflict policy.
3. `peer token` issues a signed, time-limited enrollment token containing the
   issuer’s node ID, certificate, dataset, permissions, nonce, and expiry.
4. `peer redeem` verifies the token, pins the issuer certificate, and records
   the fingerprint; the redeemer exports its own cert.
5. The issuer imports/pins the redeemer cert and grants per-dataset permissions.
6. `daemon start` serves mTLS connections; `sync now` or the background sync
   loop exchanges version-vector checkpoints and streams missing operations.
7. Operations are validated, applied idempotently, journaled, and conflicts are
   recorded according to the dataset policy.

## Entry Points

`syncfleet` CLI (`src/main.rs`, `src/cli/mod.rs`):

- `init`, `status`, `config validate|show|example`
- `dataset list|register|show|remove|schema|apply`
- `peer list|pair|token|redeem|export-cert|import-cert|certs|show|revoke|retire|reissue`
- `sync now|status`
- `conflicts list|show|resolve`
- `journal stats|compact`
- `daemon start|stop|restart`
- `doctor`

Python wrapper (`python/`): a launcher that execs the native `syncfleet`
binary, resolving `SYNCFLEET_BIN`, a bundled wheel binary, or `PATH`. Published
as a macOS universal2 wheel plus sdist for 0.7.0.

## Dependencies

- `clap` 4.5 — CLI.
- `config` 0.14, `serde`, `serde_json`, `toml` — configuration.
- `tokio` 1.38 — async runtime.
- `tracing` / `tracing-subscriber` / `tracing-appender` — logs.
- `ed25519-dalek` 2, `x25519-dalek` 2, `rand` 0.8, `sha2`, `pbkdf2`,
  `aes-gcm` — identity, key agreement, KDF, and at-rest encryption.
- `rustls` 0.23, `tokio-rustls` 0.26, `rcgen` 0.12, `rustls-pemfile`, `pem`,
  `x509-parser` — TLS 1.3 and certificates.
- `sqlx` 0.7 with SQLite — storage, migrations, WAL.
- `bincode`, `hex`, `uuid` v7, `chrono`, `time` — serialization/IDs/time.
- `metrics`, `metrics-exporter-prometheus` — declared, but no metrics endpoint
  is wired.
- `mdns-sd` 0.11 — optional/default mDNS discovery.
- Dev: `tokio-test`, `proptest`, `tempfile`, `assert_cmd`, `predicates`.

## Module Structure

- `src/identity/mod.rs` — `NodeIdentity`, Ed25519/X25519 keys, identity file
  save/load, PBKDF2+AES-GCM passphrase encryption, enrollment token helper.
- `src/config/mod.rs` — config schema/defaults/validation and `config show`.
- `src/storage/mod.rs` — SQLite pool/PRAGMAs/migrations, journal, datasets,
  peers, enrollment token table, spool.
- `src/tls/cert.rs` — self-signed Ed25519 cert generation, SANs, fingerprints.
- `src/tls/store.rs` — pinned trust store (`certs/ca.pem`).
- `src/tls/enrollment.rs` — signed `sf1:` token issue/parse/verify.
- `src/tls/mod.rs` — TLS 1.3 server/client configs with pinned verifiers.
- `src/net/mod.rs` — mTLS peering and transport framing.
- `src/replication/mod.rs` — version vectors, checkpoints, incremental
  operation streaming, apply.
- `src/sync_loop.rs` — per-peer background sync with backoff.
- `src/discovery/mod.rs` — mDNS advertisement/discovery.
- `src/cli/mod.rs` — command dispatch and daemon management.
- `tests/{cli,identity,net,replication,storage,tls}.rs` — integration tests.

## Data, State, APIs, and Contracts

### Storage

SQLite via `sqlx`, WAL mode, `synchronous=Normal`, `foreign_keys=ON`,
configurable `busy_timeout` (default 5 s), optional startup `PRAGMA
integrity_check`, and `PRAGMA wal_checkpoint(TRUNCATE)`. Core tables include
datasets, peers, peer authorizations, operation journal, checkpoints,
conflicts, enrollment tokens, and a disconnection spool. Operations use UUIDv7
IDs and per-origin sequence numbers; checkpoints are per peer+dataset.

### Identity and key storage

`NodeIdentity { node_id, ed25519_public, ed25519_private_encrypted,
x25519_public, x25519_private_encrypted, created_at }`.

- `generate(Some(passphrase))`: PBKDF2-HMAC-SHA256, 600,000 iterations, static
  salt `syncfleet-passphrase-salt-v1`, then AES-256-GCM with a random 12-byte
  nonce.
- `generate(None)`: raw private key bytes are stored in the fields named
  `*_encrypted`.
- `save`: bincode-serialized `node_identity.bin`, written with `fs::write` then
  `set_permissions(0600)` on Unix. The data directory is created with
  `create_dir_all` and no explicit mode.
- `load`: bincode deserialization; no at-rest integrity MAC when unencrypted.

### TLS and trust

- Self-signed Ed25519 node cert; `CERT_VALIDITY_DAYS = 365 * 2`.
- TLS 1.3 only, enforced by `builder_with_protocol_versions(&[TLS13])`.
- Server requires a client cert verified against the pinned root store; client
  verifies the server against the same store.
- Pinning stores the peer cert and fingerprint; unknown/changed certs are
  rejected.
- Revocation/retirement is local; CRL/OCSP is future work.

### Enrollment

- `sf1:<hex(JSON)>` token carrying version, issuer node ID, DER cert, dataset,
  permissions, 128-bit nonce, issued-at, expiry.
- Signed over canonical JSON of claims with the issuer cert’s Ed25519 key.
- `verify` checks version, expiry, cert-node-ID binding, and signature.
- The issuer records a SHA-256 token hash in `enrollment_tokens`; a
  `claim()` function exists to atomically mark `used_at`, but no call site
  invokes it. `peer redeem` is local and never contacts the issuer.
- The token is self-contained; the issuer still separately imports/pins the
  redeemer certificate and grants permissions.

### Replication and conflicts

- Version-vector checkpoints; incremental operation stream; bounded
  `max_ops_per_response`.
- Per-operation READ/WRITE authorization, schema-version checks, idempotent
  apply, conflict recording.
- Policies: `manual`, `remote_wins`, `local_wins`, `last_write_wins` (compares
  operation timestamps), `merge_fields` (deterministic LWW-register per JSON
  field, ties by `created_at` then origin node ID).
- Single-writer designation is still on the roadmap; concurrent writes are
  detected and recorded, not fully prevented.

## Security Observations

### Positive controls

- TLS 1.3 only, mutual authentication, pinned certificates; no external CA
  required.
- Ed25519 identity and X25519 key material; AES-256-GCM for passphrase-protected
  keys, with random 96-bit nonces.
- Per-dataset ACLs and per-operation READ/WRITE checks.
- Token signature verified against the embedded certificate and node ID.
- SQLite WAL, foreign keys, busy timeout, optional integrity check; checkpoint
  tied to peer+dataset.
- Identity private-key file mode 0600; TLS key mode 0600.
- No shell-outs; no network listener beyond the mTLS peering port and optional
  mDNS.
- Structured logging; no secrets intentionally logged.

### Risk areas

1. **Private keys are plaintext when no passphrase is supplied.** The CLI
   `--passphrase` is optional. Without it, `generate(None)` stores raw Ed25519
   and X25519 private key bytes in `node_identity.bin`, despite field names
   ending in `*_encrypted`. `SECURITY.md` and the threat model claim keys are
   “encrypted at rest” and “never persisted in plaintext.” File mode is 0600,
   but the file is written with `fs::write` and only then chmodded, leaving a
   brief umask-dependent window; the data directory itself has no explicit
   restrictive mode. Backups, snapshots, or copied data directories can expose
   the keys.
2. **Enrollment tokens are not single-use in practice.** `storage::enrollment::claim`
   atomically marks `used_at`, but no code calls it. `peer redeem` verifies and
   pins locally and never contacts the issuer; the issuer’s stored token hash is
   never consulted at redemption. The same token can be redeemed repeatedly
   until expiry. The issuer still must pin/authorize each redeemer, so this is
   not an immediate authorization bypass, but it violates the documented
   “single-use” contract and allows unlimited enrollment attempts from a leaked
   token.
3. **Passphrase KDF uses a global static salt and differs from documentation.**
   `PASSPHRASE_SALT` is a hard-coded constant, so the same passphrase derives
   the same key on every node/deployment. `SECURITY.md` claims Argon2id and
   HKDF; the implementation uses PBKDF2-HMAC-SHA256 600k. PBKDF2 is a
   reasonable work factor, but a per-identity random salt is standard practice
   and prevents cross-node precomputation.
4. **Ten configuration fields are dead.** None of the following are read
   outside `src/config/mod.rs` (some only validated to be nonzero):
   `storage.journal_max_size_gb`, `storage.snapshot_interval_hours`,
   `storage.compaction_interval_hours`, `replication.max_concurrent_peers`,
   `replication.max_in_flight`, `replication.snapshot_bootstrap_enabled`,
   `replication.max_snapshot_size_mb`, `security.require_enrollment_token`,
   `security.auto_renew_peer_certs`, and the entire `metrics` section. Operators
   can set limits/features that are not enforced; docs describe snapshots,
   metrics, auto-renewal, and concurrency bounds as if they exist.
5. **The “no wall-clock dependency” claim is false for LWW.** The threat model
   says correctness has no wall-clock dependency, but `last_write_wins`
   compares operation timestamps, and `merge_fields` uses `created_at` as a
   tie-breaker. Clock skew can change which write wins. The README is honest
   that LWW compares timestamps; the threat model is not.
6. **Malformed enrollment tokens can panic the CLI.** `verify` copies the
   decoded signature into `[u8; 64]` with `copy_from_slice`, which panics if the
   hex decodes to any other length. A malformed token is untrusted input to
   `peer redeem`. There is also no size bound before hex/JSON decoding, so a
   very large token can allocate heavily.
7. **SQLite and data-directory permissions are not explicitly restricted.**
   `Storage::open` calls `create_dir_all` and SQLite creates `syncfleet.db`,
   `-wal`, and `-shm` under the process umask. Only `node_identity.bin` and the
   TLS key are chmodded. A 0755/0644 default can expose the replicated dataset
   and journal to local users even when the identity key is 0600.
8. **Dependency audit/deny do not block CI.** The `audit` and `deny` jobs use
   `continue-on-error: true`, and the `release` job needs `[build, audit, deny]`
   but those jobs can be red without failing the workflow. Security docs and
   threat-model checkboxes imply scanning is enforced.
9. **Status documentation is stale.** README says daemon stop/restart is “not
   implemented yet,” but `DaemonAction::Stop`/`Restart` and PID management exist.
   `KNOWN_ISSUES.md` still lists daemon stop/restart as remaining. It also says
   metrics are exposed via `/metrics`, but no endpoint is wired. TLS certificate
   docs say 1-year node/90-day peer certs, while code uses a 2-year constant and
   the validity/renewal config fields are unused.

## Tests and CI

- CI (`/.github/workflows/ci.yml`):
  - `fmt` (`cargo fmt --check --all`)
  - `clippy` (`--all-targets --all-features -D warnings`)
  - `test` (`cargo test --workspace --all-features`)
  - `test-doc` (`cargo test --doc --workspace`)
  - `audit` (`cargo audit`, `continue-on-error: true`)
  - `deny` (`cargo deny check`, `continue-on-error: true`)
  - `build` release + `syncfleet --help`
  - `coverage` via `cargo llvm-cov` + Codecov (`fail_ci_if_error: false`)
  - `msrv` (`cargo check --workspace` on 1.89)
  - `release` on tags, needs `[build, audit, deny]`
- 66 `#[test]`/`#[tokio::test]` attributes observed across `tests/` and `src/`
  (cli, identity, net, replication, storage, tls).
- Tests cover identity generation/load, TLS config, replication/checkpoints,
  storage/journal behavior, CLI flows, and network framing.
- No test asserts single-use token rejection, plaintext-key absence, config
  enforcement, file modes, or malformed-signature handling.
- All CI runners are Ubuntu; no macOS/Windows jobs.

## Findings

| Severity | Finding | Evidence | Impact | Root Cause Hypothesis | Recommendation |
|---|---|---|---|---|---|
| Medium | Private keys are stored plaintext when no passphrase is supplied, despite “encrypted at rest” claims | `src/identity/mod.rs:50-57` stores raw key bytes when `passphrase` is `None`; fields are named `*_encrypted`; CLI `--passphrase` is optional (`src/cli/mod.rs:73-74,505-506,540-543`); `docs/SECURITY.md:128-134`, `docs/THREAT_MODEL.md:58,128,145` claim encrypted key storage; `save` writes then chmods (`mod.rs:132-142`) | Anyone who can read `node_identity.bin` (local user during the pre-chmod window, backup, copied data dir, or host compromise) gets the node’s Ed25519 signing and X25519 static keys and can impersonate the node to pinned peers | Passphrase was made optional for convenience, but the at-rest guarantee was documented unconditionally; file creation uses `fs::write` before chmod | Require a passphrase for production, or store a clear “unencrypted” marker and refuse to start in hardened mode; create the file with `OpenOptions::mode(0o600)` atomically; create the data dir 0700; correct the docs and add a test that asserts encrypted-vs-plaintext behavior |
| Medium | Enrollment tokens are not enforced single-use | `src/storage/mod.rs:1362-1402` defines `claim()` to set `used_at`; no call sites; `src/cli/mod.rs:859-883` redeems locally and never consults the issuer’s `enrollment_tokens` table; README:107, `docs/SECURITY.md:83`, `docs/THREAT_MODEL.md:142,161` claim single-use | A leaked token can be redeemed by multiple parties until expiry; each still needs issuer cert pinning/authorization, so this is a contract/replay weakness rather than immediate access, but it enables repeated enrollment attempts and undermines the token’s security promise | The self-contained token design made redemption offline/local, while the single-use ledger lives only on the issuer and is never queried | Either make enrollment an online redemption against the issuer (consume atomically) or explicitly document that tokens are bearer invitations, not single-use; add a revocation/consume protocol and tests for replay |
| Medium | Ten config fields are declared but unused, including advertised bounds/metrics/snapshots | `src/config/mod.rs:98-101,124-130,154-156,172-189`; grep outside `config/mod.rs` finds no reads for `journal_max_size_gb`, `snapshot_interval_hours`, `compaction_interval_hours`, `max_concurrent_peers`, `max_in_flight`, `snapshot_bootstrap_enabled`, `max_snapshot_size_mb`, `require_enrollment_token`, `auto_renew_peer_certs`, or `metrics`; docs/OPERATIONS.md and ROADMAP mention these features | Operators may believe concurrency, journal size, snapshots, metrics, and cert auto-renewal are enforced/available when they are not; resource-exhaustion limits are illusory | Config schema was extended ahead of implementation; validation checks only a subset | Remove or mark fields unimplemented, fail validation for unsupported non-default values, or implement and test enforcement; align docs/ROADMAP/OPERATIONS with actual behavior |
| Medium | Passphrase KDF uses a static global salt; docs claim Argon2id/HKDF | `src/identity/mod.rs:13,15-22,88-97`: fixed `PASSPHRASE_SALT`, PBKDF2-HMAC-SHA256 600k; `docs/SECURITY.md:56,132` says HKDF/Argon2id; `docs/THREAT_MODEL.md:130` says HKDF | The same passphrase derives the same key on every node, enabling cross-node precomputation/rainbow tables; documentation misstates the cryptographic design, weakening audit confidence | Salt was treated as a domain separator rather than a per-identity random salt; docs were not updated after switching from HKDF | Generate a random 16–32 byte salt per identity, store it with a versioned format, include it in the KDF, and migrate old files; use Argon2id or document PBKDF2 accurately with rationale |
| Low | `last_write_wins`/`merge_fields` use wall-clock timestamps, contradicting “no wall-clock dependency” | `README.md:185` says LWW compares operation timestamps and `merge_fields` ties by `created_at`; `docs/THREAT_MODEL.md:120` says “No wall-clock dependency for correctness” | Clock skew across nodes can change conflict outcomes; a node with a fast clock wins more often, and replay/ordering assumptions are weaker than documented | LWW was chosen for simplicity; the threat model’s statement was written for version-vector correctness and not updated for LWW | Use logical versions (Lamport/version vector) as the primary order for LWW, or explicitly document clock-skew assumptions and bound acceptable skew; add skew tests |
| Low | Malformed enrollment token can panic or allocate unboundedly | `src/tls/enrollment.rs:136-140` uses `copy_from_slice` into `[u8; 64]` without a length check; `parse` decodes arbitrary-length hex/JSON with no size limit | A malformed `sf1:` token supplied to `peer redeem` can panic the CLI (DoS) or force large allocations; local/untrusted input only | Deserialization assumes a well-formed signature length; no input-size guard | Check decoded signature length before copy, return `EnrollmentInvalid`; cap token length before hex/JSON decode; add malformed-token tests |
| Low | SQLite DB/WAL and data directory permissions are not explicitly restricted | `src/storage/mod.rs:40-60` uses `create_dir_all` and SQLite defaults; only `node_identity.bin` and TLS key get 0600 (`identity/mod.rs:137-141`, `tls/cert.rs:217`) | On a 0755/0644 umask, local users can read replicated datasets, journal, checkpoints, and conflict data | Permission hardening was applied to key files only; DB files rely on umask | Create the data dir 0700 and DB/WAL/SHM 0600 (or document a required umask), and check/fix modes at startup |
| Low | CI audit/deny are non-blocking | `.github/workflows/ci.yml` `audit` and `deny` jobs use `continue-on-error: true`; `release` needs them but does not fail on their errors | Known vulnerable/denied dependencies can ship; docs/threat-model checkboxes imply enforcement | Jobs were added for visibility before being made required | Remove `continue-on-error`, or mark the checks advisory in docs; add a required dependency-policy gate before release |
| Low | Status/security docs are stale relative to code | `README.md:61-63` says daemon stop/restart “not implemented yet” while `src/cli/mod.rs:1603-1611` implements it; `docs/KNOWN_ISSUES.md:8,29` repeats stop/restart and claims a `/metrics` endpoint that is not wired; `src/tls/cert.rs:19` uses 2-year certs while `docs/SECURITY.md:136-139` says 1-year/90-day and config validity fields are unused | Operators and auditors get an incorrect picture of capabilities and certificate lifetime; release notes/status claims are unreliable | Docs evolved across phases without a status reconciliation pass | Update README/KNOWN_ISSUES/SECURITY/ROADMAP in one pass against code; add a docs consistency check or generated status table; remove dead config or implement it |

## Strengths

- TLS 1.3 only with mutual auth and pinned certs; no ambient CA trust.
- Ed25519 identity, X25519 key agreement, AES-256-GCM with random nonces.
- Per-dataset ACLs and per-operation READ/WRITE enforcement.
- Signed enrollment tokens bound to the issuer cert/node ID; TOFU pinning.
- SQLite WAL, foreign keys, busy timeout, optional integrity check.
- Idempotent operation apply, version-vector checkpoints, schema-version
  rejection, conflict recording, and compaction.
- Broad test suite (66 tests) and strict CI (fmt, clippy `-D warnings`, tests,
  doc tests, release build, MSRV).
- Python wrapper is a thin launcher, not a reimplementation.

## Open Questions / Not Verified

- Whether deployments always supply `--passphrase`; the CLI warns when they do
  not, but there is no hard requirement.
- Whether the issuer’s local `enrollment_tokens` ledger has any future network
  redemption path; none exists today.
- Whether any external deployment depends on the unused config fields; changing
  them to hard errors could be breaking.
- Certificate renewal/reissue behavior: only manual `peer reissue` exists; the
  auto-renew config is unused.
- Live multi-node replication behavior and clock-skew outcomes were not tested;
  no tests were run during this audit.

## References

- `README.md`
- `Cargo.toml`
- `.github/workflows/ci.yml`
- `src/identity/mod.rs`
- `src/config/mod.rs`
- `src/storage/mod.rs`
- `src/tls/{mod,cert,store,enrollment}.rs`
- `src/net/mod.rs`
- `src/replication/mod.rs`
- `src/cli/mod.rs`
- `src/sync_loop.rs`
- `src/discovery/mod.rs`
- `docs/{SECURITY,THREAT_MODEL,ROADMAP,KNOWN_ISSUES,OPERATIONS,CONSISTENCY}.md`
- `tests/{cli,identity,net,replication,storage,tls}.rs`
