# SyncFleet — Audit Findings

Audit date: 2026-10-27
Scope: Rust source, SQLite schema/migrations, TLS/enrollment, config, CLI,
docs, CI
Method: read-only source/docs/CI inspection; tests were not run

## Summary

SyncFleet has a coherent local-first design: TLS 1.3 mutual authentication with
pinned certificates, Ed25519/X25519 identity, per-dataset ACLs, signed
enrollment tokens, version-vector replication, idempotent operation apply,
SQLite WAL, and strict CI. The main issues are security-contract and
configuration gaps rather than protocol breaks.

Without a passphrase, private keys are stored in plaintext even though docs say
“encrypted at rest.” Enrollment tokens are not single-use because the
issuer-side `claim()` function is never called. Ten configuration fields,
including advertised concurrency limits, snapshots, metrics, and auto-renewal,
are not enforced. The passphrase KDF uses a static salt and differs from the
documented Argon2id/HKDF. Several docs are stale relative to code.

| Severity | Count |
|---|---:|
| Critical | 0 |
| High | 0 |
| Medium | 4 |
| Low | 5 |

## Findings

### 1. Medium — Private keys are plaintext without a passphrase

**Evidence**

- `src/identity/mod.rs:50-57`: `generate(None)` stores raw Ed25519/X25519
  private bytes in `*_encrypted` fields.
- `src/cli/mod.rs:73-74,505-506,540-543`: `--passphrase` is optional; the CLI
  prints a warning when omitted.
- `src/identity/mod.rs:132-142`: writes `node_identity.bin`, then chmods 0600.
- `docs/SECURITY.md:128-134` and `docs/THREAT_MODEL.md:58,128,145`: claim
  encrypted key storage/“never persisted in plaintext.”

**Impact**

Anyone who reads the identity file — a local user during the pre-chmod window,
a copied data directory, a backup, or a host compromise — obtains the node’s
signing and static keys and can impersonate it to pinned peers. File mode 0600
limits but does not eliminate exposure.

**Root cause**

Passphrase protection was made optional for convenience, while the at-rest
guarantee was documented unconditionally. File creation uses `fs::write`
followed by `set_permissions`, not an atomic 0600 open.

**Recommendation**

Require a passphrase for production, or store an explicit unencrypted marker
and refuse hardened startup. Create the file with `OpenOptions::mode(0o600)`;
create the data directory 0700; correct the docs; add tests for both modes.

### 2. Medium — Enrollment tokens are not single-use

**Evidence**

- `src/storage/mod.rs:1362-1402`: `claim()` atomically sets `used_at`.
- No call site invokes `claim()`.
- `src/cli/mod.rs:859-883`: `peer redeem` verifies/pins locally and never
  queries the issuer’s `enrollment_tokens` table.
- `README.md:107`, `docs/SECURITY.md:83`, `docs/THREAT_MODEL.md:142,161`:
  claim single-use tokens.

**Impact**

A leaked token can be redeemed repeatedly until expiry. Each redeemer still
needs the issuer to pin/authorize its certificate, so this is not an immediate
authorization bypass, but it violates the documented contract and enables
repeated enrollment attempts.

**Root cause**

The token is self-contained and redemption is offline, while the single-use
ledger lives only on the issuer and is never consulted.

**Recommendation**

Make redemption an online atomic consume against the issuer, or document tokens
as bearer invitations rather than single-use. Add replay tests and a
consume/revoke protocol.

### 3. Medium — Ten config fields are unused

**Evidence**

No reads outside `src/config/mod.rs` for:

`storage.journal_max_size_gb`, `storage.snapshot_interval_hours`,
`storage.compaction_interval_hours`, `replication.max_concurrent_peers`,
`replication.max_in_flight`, `replication.snapshot_bootstrap_enabled`,
`replication.max_snapshot_size_mb`, `security.require_enrollment_token`,
`security.auto_renew_peer_certs`, and the entire `metrics` section.

**Impact**

Operators can set limits/features that are not enforced. Advertised
concurrency, journal-size, snapshot, metrics, and cert-renewal behavior is
illusory; resource-exhaustion limits do not exist.

**Root cause**

Config schema was extended ahead of implementation; only a subset is validated
or read.

**Recommendation**

Remove/mark fields unimplemented, reject unsupported non-default values, or
implement and test them. Align `docs/OPERATIONS.md`, README, and ROADMAP.

### 4. Medium — Static KDF salt and docs/implementation mismatch

**Evidence**

- `src/identity/mod.rs:13,15-22,88-97`: fixed
  `syncfleet-passphrase-salt-v1`; PBKDF2-HMAC-SHA256 600,000.
- `docs/SECURITY.md:56,132`: claims HKDF-SHA256/Argon2id.
- `docs/THREAT_MODEL.md:130`: claims HKDF.

**Impact**

The same passphrase derives the same key on every node, enabling cross-node
precomputation/rainbow tables. Documentation misstates the cryptography,
weakening audit confidence.

**Root cause**

Salt was used as a domain separator, not a per-identity random salt; docs were
not updated when the KDF changed from HKDF.

**Recommendation**

Generate a random 16–32 byte salt per identity, store a versioned format, and
migrate; use Argon2id or document PBKDF2 accurately with rationale and tests.

### 5. Low — LWW/merge_fields depend on wall-clock time

**Evidence**

- `README.md:185`: `last_write_wins` compares operation timestamps;
  `merge_fields` ties by `created_at`.
- `docs/THREAT_MODEL.md:120`: says “No wall-clock dependency for correctness.”

**Impact**

Clock skew changes conflict outcomes; a fast clock wins more often. The
documented guarantee is false for LWW policies.

**Root cause**

LWW was chosen for simplicity; the threat model statement was written for
version-vector correctness and not updated.

**Recommendation**

Use logical ordering (Lamport/version vector) for LWW, or document bounded
clock-skew assumptions and test skew behavior.

### 6. Low — Malformed enrollment tokens can panic/allocate heavily

**Evidence**

- `src/tls/enrollment.rs:136-140`: `sig.copy_from_slice` into `[u8; 64]`
  without a decoded-length check.
- `parse` hex/JSON-decodes arbitrary input with no size cap.

**Impact**

A malformed `sf1:` token can panic `peer redeem` or force large allocations;
local/untrusted input only.

**Root cause**

Deserialization assumes a 64-byte signature; no input-size guard.

**Recommendation**

Check length before copy and return `EnrollmentInvalid`; cap token size before
decode; add malformed-token tests.

### 7. Low — SQLite DB/WAL and data-dir permissions are not restricted

**Evidence**

- `src/storage/mod.rs:40-60`: `create_dir_all` + SQLite defaults.
- Only `node_identity.bin` and TLS key get 0600.

**Impact**

With a 0755/0644 umask, local users can read replicated datasets, journal,
checkpoints, and conflicts.

**Root cause**

Hardening was applied to key files only.

**Recommendation**

Create the data dir 0700 and DB/WAL/SHM 0600, or document/validate a required
umask; check modes at startup.

### 8. Low — CI audit/deny are non-blocking

**Evidence**

- `.github/workflows/ci.yml`: `audit` and `deny` jobs use
  `continue-on-error: true`.
- The `release` job needs them but does not fail the workflow on their errors.

**Impact**

Known vulnerable or policy-denied dependencies can ship; docs/threat-model
checkboxes imply enforcement.

**Root cause**

Jobs were added for visibility before being made required.

**Recommendation**

Remove `continue-on-error` or clearly mark advisory; add a required gate before
release.

### 9. Low — Status/security docs are stale

**Evidence**

- `README.md:61-63` says daemon stop/restart is not implemented;
  `src/cli/mod.rs:1603-1611` implements it.
- `docs/KNOWN_ISSUES.md:8,29` repeats stop/restart and claims a `/metrics`
  endpoint that is not wired.
- `src/tls/cert.rs:19` uses 2-year certs; `docs/SECURITY.md:136-139` says
  1-year node/90-day peer, and validity/renewal config is unused.

**Impact**

Operators/auditors get an incorrect capability and certificate-lifetime
picture.

**Root cause**

Docs evolved across phases without reconciliation.

**Recommendation**

Update README/KNOWN_ISSUES/SECURITY/ROADMAP against code; add a docs
consistency check or generated status table.

## Positive Observations

- TLS 1.3 only, mutual auth, pinned certificates.
- Ed25519/X25519 identity; AES-256-GCM with random nonces when passphrase is
  used.
- Per-dataset ACLs and per-operation READ/WRITE checks.
- Signed enrollment token bound to issuer cert/node ID; TOFU pinning.
- SQLite WAL, foreign keys, busy timeout, optional integrity check.
- Idempotent operation apply, version vectors, schema-version rejection,
  conflict recording, compaction.
- 66 tests; strict fmt/clippy/test/doc-test/build/MSRV CI.
- Thin Python launcher, no reimplementation.

## Verification Notes

- Working tree was clean; no source/config changes were made.
- Tests were not run.
- Passphrase usage in real deployments, token replay impact, unused-config
  compatibility, and live clock-skew behavior were not empirically tested.
