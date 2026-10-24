# Competitive intelligence in roadmap and upgrade work

**Policy date:** 2026-10-19

Competitive findings are an input to product decisions, not a one-off report.
Every pending roadmap item, new feature, and routine dependency upgrade should
check the repository's [`AUDIT_FINDINGS.md`](AUDIT_FINDINGS.md) before work
starts, and record the decision in the PR or changelog.

## Required check before building

1. **Read `AUDIT_FINDINGS.md`.** Find the relevant competitor row and the
   ranked next actions. If the finding is stale, update it with a dated source.
2. **Name the competitor and the user problem.** A feature is not justified by
   "competitor has it". State the workflow it unlocks, who needs it, and how
   often. Mark unverified demand as a hypothesis.
3. **Prefer reuse over new code.** Check whether an existing Mullassery repo
   already provides the capability (see `BUNDLES.md` in the profile repo and
   the bundle role file in this repo, when present). Typical reuse:
   - crawling/fetching/robots/SSRF → **MudFish**
   - architecture maps / PR diffs → **CodeAtlas**
   - data-quality contracts / validation → **StatGuardian**
   - token counting / cost tables → **PyTokenCalc**
   - dependency graph / advisories / SBOM → **PyDeepMesh**
   - segmentation / CLV / churn → **ClusterAudienceKit**
   - activation / lineage / PII → **PyReverseETL**
   - fleet state sync → **SyncFleet** (only after conflict resolution)
4. **Choose one disposition:** build, integrate, defer, or reject. Write it
   down. A rejection with a reason is a successful outcome.
5. **For routine upgrades**, check whether the upgrade closes a known
   competitor gap or breaks a differentiated capability. Run the tests that
   cover the affected path; do not merge a green build that silently changes a
   user-visible contract.

## PR / changelog checklist

- [ ] Competitor and source URL/date recorded (or "no direct competitor").
- [ ] User impact and frequency stated; unverified demand labelled.
- [ ] Reuse check done against sibling repos/bundles.
- [ ] Failure mode and rollback considered (especially auth, PII, cost).
- [ ] Docs/roadmap status updated honestly.
- [ ] Tests or a manual verification command included.

## Review cadence

- Refresh the competitor row when a release changes the capability.
- Re-check the full `AUDIT_FINDINGS.md` at least quarterly and before a major
  release.
- If a finding is wrong or stale, correct it in the same PR that relies on it.

## Non-goals

- Copying features for parity.
- Adding a dependency just because a competitor uses it.
- Treating star counts or download counts as proof of user need.

## OSS gap analysis — 2026-10-24

**What matters most here:** project operations into tables and ship Linux wheels. Replicated journals that are not materialized make automatic sync unreliable, and macOS-only wheels block the Linux edge gateways this product targets; fix both before CRDT work.

Scope: SyncFleet 0.7.0 compared with four actively maintained OSS replication and sync systems on topology, conflict handling, partial replication, security, and operability. Claims about SyncFleet come from this repo's README/architecture docs; competitor claims come from their repository documentation.

### Comparable OSS projects

| Project | GitHub | License | Last activity | Why it is the comparison |
| --- | --- | --- | --- | --- |
| Apache CouchDB | https://github.com/apache/couchdb | Apache-2.0 | pushed 2026-10-23 (~7.0k stars) | Mature multi-master document replication with a well-defined conflict model |
| Automerge | https://github.com/automerge/automerge | MIT | pushed 2026-10-24 (~6.7k stars) | CRDT documents plus a sync protocol for offline, concurrent edits |
| Electric | https://github.com/electric-sql/electric | Apache-2.0 | pushed 2026-10-06 (~10.4k stars) | Postgres-to-client sync with partial replication ("Shapes") over HTTP |
| rqlite | https://github.com/rqlite/rqlite | MIT | pushed 2026-10-24 (~17.9k stars) | Distributed SQLite with fault-tolerant clustering and simple operations |

### Capability comparison

| Capability | Ours | CouchDB | Automerge | Electric | rqlite | Gap |
| --- | --- | --- | --- | --- | --- | --- |
| Peer-to-peer replication | Yes | Yes | Partial | Partial | No | CouchDB and ours are true multi-node replication; Electric is read-path sync |
| SQLite table-level replication | Yes | No | No | No | Partial | Ours is table/dataset scoped; rqlite replicates the whole database |
| Incremental by version vector | Yes | Partial | Yes | Yes | Partial | Ours and Automerge are strong; others use checkpoints, offsets, or logs |
| Multi-writer CRDT merge | No | No | Yes | No | No | Real gap |
| Built-in conflict policies | Yes | Partial | Yes | No writes | No | Ours offers five policies; CouchDB leaves resolution to the app |
| Per-dataset read/write ACL | Yes | Partial | No | Partial | Partial | Ours is explicit per dataset and per peer |
| Mutual TLS with pinned certs | Yes | Partial | No | Partial | Partial | Ours is a differentiator for operator-controlled fleets |
| Offline operation | Yes | Yes | Yes | Partial | Yes | None of the local-first options require a cloud service |
| Relay or NAT traversal | No | Partial | No | Yes | Partial | Real gap; ours assumes a trusted LAN or VPN |
| Materialize replicated operations into tables | No | Yes | Yes | Yes | Yes | Real gap; ours replicates the journal, not the projected state |
| Automatic schema migration | No | Partial | N/A | Partial | Partial | Real gap for long-lived fleets |
| Cross-language client SDK | No | Yes | Yes | Yes | Yes | Real gap; the Python package is only a launcher |
| Published scale evidence | No | Yes | Partial | Yes | Yes | Real gap |

Nuance: CouchDB's "peer-to-peer" means HTTP replication between CouchDB servers, not a brokerless embedded engine; its TLS and ACL capabilities exist but are administered at the server/database layer. Electric is explicitly a read-path sync engine: it syncs Postgres changes to clients and does not accept arbitrary client writes itself. rqlite uses a leader-based consensus cluster for high availability, so it is not a multi-master or brokerless mesh. Automerge is a library and sync protocol, not a deployable database with certificates, ACLs, or a daemon.

### Gaps that matter (ranked)

1. **Replicated operations are not projected into the allowlisted user tables.** The README records this as a remaining gap in the PyTerrainMap bridge. In practice a peer can receive the journal while queries against the user's tables still return stale data, which undercuts the "automatic replication" claim. This is the highest-value functional gap. Effort M.
2. **No full multi-writer CRDT.** `merge_fields` is an LWW-register merge with deterministic tie-breaking, not an intention-preserving CRDT. Two offline edits to the same field can lose one value; Automerge preserves concurrent edits. For append-only fleet observations this is tolerable, for general application state it is not. Effort L.
3. **No relay or NAT traversal.** SyncFleet requires directly reachable peers on a trusted LAN or VPN. Electric's HTTP/CDN delivery and CouchDB's HTTP replication work through more network shapes. This limits deployments to the environments the README already names. Effort L.
4. **Linux and Windows distribution is unpublished.** PyPI 0.7.0 ships a macOS universal2 wheel plus sdist; the workflow defines Linux/Windows wheels but does not publish them. The stated primary users are Linux edge gateways and Raspberry Pi-class devices, so this is an adoption blocker disproportionate to its engineering cost. Effort M.
5. **Schema evolution is manual.** An operation with a mismatched `schema_version` is rejected as a conflict and the operator must run `syncfleet dataset schema` after a migration. CouchDB and rqlite have more mature upgrade stories. Effort M.
6. **No cross-language SDK and no published scale evidence.** The Python package forwards to the native binary, so embedding SyncFleet in a Go/JS/C++ edge agent means shelling out. There are also no published results for peer count, dataset size, or conflict rate, which makes the "modest datasets on modest hardware" claim unquantified. Effort L and M respectively.

### Where we are differentiated

- **Brokerless, operator-owned trust:** mutual TLS 1.3 with pinned self-signed certificates and single-use signed enrollment tokens, with no cloud account, control plane, or message broker.
- **Explicit dataset-scoped authorization:** per-dataset read/write grants enforced on the wire, rather than database-wide replication.
- **Offline-first journal with auditability:** local operation journaling, per-origin checkpoints, conflict records, and five documented resolution policies including field-level merge.
- **Small Rust + SQLite surface:** easy to run on a gateway, with mDNS discovery that never bypasses enrollment.

### Disposition

- **Build:** operation projection into user tables (P1, M); Linux/Windows wheels (P1, M); automated schema migration (P2, M); cross-language client bindings for the embedded/edge case (P2, L).
- **Integrate/reuse:** use `cr-sqlite` behind the existing dataset abstraction instead of writing a CRDT from scratch; reuse PyTerrainMap's append-only observations as the reference workload for projection and scale tests.
- **Defer/reject:** a cloud control plane, message broker, and general-purpose distributed SQL. Those are explicitly out of scope and should remain so; the wedge is trusted, local-first fleet sync, not a rqlite or CouchDB replacement.

### Sources

- https://github.com/apache/couchdb (Apache-2.0, pushed 2026-10-23, accessed 2026-10-24)
- https://github.com/automerge/automerge (MIT, pushed 2026-10-24, accessed 2026-10-24)
- https://github.com/electric-sql/electric (Apache-2.0, pushed 2026-10-06, accessed 2026-10-24)
- https://github.com/rqlite/rqlite (MIT, pushed 2026-10-24, accessed 2026-10-24)
- Repo files inspected: `README.md`, `COMPETITIVE_INTELLIGENCE.md`, `docs/ARCHITECTURE.md`, `docs/CONSISTENCY.md`, `docs/ROADMAP.md`, `docs/KNOWN_ISSUES.md`
