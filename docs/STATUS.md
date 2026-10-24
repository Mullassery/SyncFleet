# Project status

**Version:** 0.7.0 (alpha) · **Updated:** 2026-10-19

## Working today

- Ed25519 node identity and encrypted keystore (X25519).
- Self-signed TLS 1.3, pinned (TOFU) trust store, mutually authenticated
  peering handshake.
- Single-use signed enrollment tokens (issue/redeem).
- Datasets with table allowlists; per-peer read/write authorization.
- Version-vector checkpoints and incremental operation streaming.
- Local operation journaling (`dataset apply`) and per-origin `sync status`.
- `daemon start` serving loop plus a background sync task per configured peer
  with exponential backoff.
- mDNS discovery of already-authorized peers.
- Conflict handling: `manual`, `remote_wins`, `local_wins`,
  `last_write_wins`, and `merge_fields` (deterministic LWW over disjoint JSON
  fields), plus schema-version mismatch detection.
- `conflicts list|show|resolve`, `journal stats|compact`, `doctor`.
- Python wrapper on PyPI (macOS universal2 wheel bundles the native binary).

## Verified gaps

| Gap | Impact | Status |
|---|---|---|
| PyPI 0.7.0 publishes only the macOS universal2 wheel | Linux/Windows users must build the native CLI | Open |
| No GitHub releases/tags | No release feed; PyPI exists | Open |
| PyPI publishing uses `PYPI_API_TOKEN` | Not OIDC Trusted Publishing | Open |
| Full multi-writer CRDT (`cr-sqlite`) | `merge_fields` covers disjoint JSON fields only | Planned |
| mDNS interface binding | Discovery binds defaults only | Planned |
| Daemon `stop`/`restart` PID management | No process control | Implemented (2026-10-25; Unix pid file + SIGTERM) |
| Automatic schema migration | Manual `dataset schema` update only | Planned |
| Relay/NAT traversal | Peers must be mutually reachable | Planned |
| Alpha maturity | API/config may change | By design |

## Not a goal

- A message broker or high-throughput event streaming.
- A general-purpose distributed SQL database.
- A backup/archival tool.
- Trustless/Byzantine consensus.

See [Roadmap](ROADMAP.md), [Known issues](KNOWN_ISSUES.md), and
[Comparison](COMPARISON.md).
