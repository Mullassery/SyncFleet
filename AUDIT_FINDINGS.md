# Audit findings — 2026-10-19

This file records the repository-specific findings from the portfolio audit
conducted on 2026-10-19. It is documentation only; no code or release artifacts
were changed in this pass unless explicitly noted under "Implementation status".

Sources: the repository's own README/source/workflows, GitHub metadata and
Actions runs, PyPI metadata, and official competitor documentation. Claims are
marked as verified, inferred, or open questions. "Absent" means not found in the
inspected repository/docs, not proof that it cannot exist.

## What it is

Secure, local-first, peer-to-peer SQLite synchronization for edge/robotics fleets: mTLS 1.3, pinned trust, enrollment tokens, version vectors, and incremental replication.

## Verified strengths

- Ed25519/X25519 identity; mTLS 1.3 + TOFU pinning; signed single-use enrollment; version vectors; incremental op streaming; per-op auth; WAL/idempotent apply; daemon; mDNS; background sync; 7 test files.
- Brokerless P2P + mTLS is a credible niche.

## Verified gaps and user-facing risks

- Conflict resolution runtime is not implemented; mDNS interface binding missing; no schema evolution.
- No relay/NAT traversal; no crates.io/PyPI (README claims a PyPI wrapper, 404); no example app; no multi-device scale test.
- Roadmap phases 6–8 are largely unchecked.

## Direct competitors

- cr-sqlite (CRDT SQLite), ElectricSQL (Postgres sync, 1.0), PowerSync (Postgres/MySQL/Mongo/SQL Server), Turso/libSQL.

## Ranked next actions

- P1: conflict resolution + schema evolution (L).
- P1: publish the Python wrapper/release (S).
- P2: example app + relay/NAT traversal (L).

## Do not build

- A message broker; out of scope.

## Evidence gaps / validation needed

- No 3+ device conflict test; no field deployment.

## Recommended disposition

Invest only if conflict semantics are funded; otherwise narrow to a reference P2P transport.

## Implementation status

Implemented concurrent-write conflict detection in this pass:

- `VersionVector::is_concurrent` (neither vector dominates the other).
- `apply_remote_operation` records a `concurrent_write` row in `conflicts`
  when a remote op is concurrent with the latest same-entity op from another
  origin. The op is still applied; resolution remains manual via the existing
  CLI commands.
- New storage test covers detection; full suite 55 passed; fmt/clippy clean.
- Version `0.3.0`.

Still open: automatic policy-based resolution (LWW/CRDT), schema evolution,
mDNS interface binding, and relay/NAT traversal.
