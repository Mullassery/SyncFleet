# SyncFleet

[![PyPI](https://img.shields.io/pypi/v/syncfleet.svg)](https://pypi.org/project/syncfleet/)
[![Graphify](https://img.shields.io/badge/Graphify-knowledge%20graph-blue)](https://github.com/Graphify-Labs/graphify)

A secure, local-first, peer-to-peer edge database synchronization engine.

SyncFleet lets a small set of **authorized devices** replicate **selected**
application state between themselves over mutual-TLS 1.3 connections — without
requiring a continuously available cloud service, a central message broker, or
a trusted third party.

---

## What it is / where to use it

Use SyncFleet when you have a handful of machines that must keep a subset of
their local SQLite data consistent, often across unreliable or intermittent
links.

**Good fits**

- **Edge gateways and industrial IoT** — Raspberry Pi / Linux gateways that
  accumulate sensor or device state locally and reconcile with a regional node
  when connectivity returns.
- **Retail branches** — point-of-sale and inventory nodes that sync orders and
  stock to a headquarters node, and keep operating while the internet is down.
- **Private LANs and WireGuard meshes** — a trusted set of servers or laptops
  that exchange selected tables directly, with no broker in the middle.
- **Air-gapped / high-latency sites** — environments where a cloud round-trip is
  unavailable, expensive, or unacceptable.
- **Developer tooling** — exchanging selected local state or test results
  between developer machines on the same network.

**The core idea:** each node owns a SQLite database. You register *datasets*
(named groups of tables), authorize *peers* to read/write a dataset, and the
nodes reconcile changes directly with each other. Trust is explicit: a peer
must be enrolled and have its certificate pinned before it can connect.

---

## What it is **not**

Being explicit about scope:

- **Not a cloud database or managed service.** There is no SaaS control plane and
  no vendor-hosted synchronization. Nodes talk to each other.
- **Not a replacement for PostgreSQL/MySQL in a data center.** It is an
  edge/local-first engine for modest datasets on modest hardware, not a
  general-purpose distributed SQL database.
- **Not a message broker** (Kafka, NATS, etc.) and not built for high-throughput
  event streaming.
- **Not a blockchain or trustless ledger.** It relies on explicit, operator-driven
  peer authorization and certificate pinning (trust-on-first-use); it does not
  achieve consensus among mutually distrusting parties.
- **Not a backup or archival tool.** It is a synchronization engine; point-in-time
  backups are a separate concern.
- **Not finished.** This is an actively developed project. Today you can
  initialize nodes, manage datasets and peers, issue/redeem enrollment tokens,
  pin certificates, and inspect the journal. Automated replication and the
  background daemon are wired; concurrent-write conflicts are detected and
  recorded for manual resolution. Policy-based automatic resolution and mDNS
  interface binding remain on the [roadmap](docs/ROADMAP.md).
- **Not hardened for the public internet out of the box.** The intended
  deployments are trusted LANs/VPNs between nodes you control.

---

## How to get started

### 1. Build

```bash
git clone https://github.com/Mullassery/SyncFleet
cd SyncFleet
cargo build --release
export PATH="$PWD/target/release:$PATH"
```

Requires a recent stable Rust toolchain (MSRV 1.89).

### 2. Initialize a node

`init` creates the node identity (Ed25519), the SQLite database, and a
self-signed TLS 1.3 certificate for the node.

```bash
syncfleet init --node store-a
syncfleet status
```

### 3. Register a dataset

A dataset is a named group of tables that may be replicated.

```bash
syncfleet dataset register --name orders --tables orders,order_items
syncfleet dataset list
```

### 4. Enroll a peer

Enrollment is a two-sided handshake. The node that owns the dataset (**A**)
issues a token; the new node (**B**) redeems it.

```bash
# On node A: issue a single-use token (e.g. read permission = 1) for the dataset
syncfleet peer token --dataset orders --permissions 1 --ttl-hours 24

# On node B: redeem the token — verifies the signature and pins A's certificate
syncfleet peer redeem --token sf1:...

# On node B: hand your certificate to A
syncfleet peer export-cert --out b.pem

# On node A: pin B's certificate and authorize it for the dataset
syncfleet peer import-cert --file b.pem --dataset orders --permissions 1
```

Once both certificates are pinned, the nodes can establish a mutual-TLS 1.3
connection and exchange their peering handshake.

```bash
syncfleet peer list        # authorized peers
syncfleet peer certs       # pinned peer certificates
```

### 5. Replicate

With both certificates pinned, node **A** serves and node **B** (or A) pulls.
Replication is *incremental*: only operations the peer is missing cross the
wire, identified by a version vector.

```bash
# On node A: serve peering/replication connections (Ctrl-C to stop)
syncfleet daemon start

# On either node: stage a change locally (recorded in the journal)
syncfleet dataset apply --dataset orders --entity order-1 --op insert --data '{"total":42}'

# On node B: synchronize with A (pull + push in one session)
syncfleet sync now --peer <a-node-id> --addr 127.0.0.1:7842
syncfleet sync now --peer <a-node-id>          # uses config [[peers]] entry

# Inspect per-origin checkpoints for a dataset
syncfleet sync status --dataset-id orders
```

Add static peers so `--addr` can be omitted:

```toml
[[peers]]
node_id = "<a-node-id>"
address = "10.0.0.5:7842"
```

Or let `daemon start` find peers on the local network via mDNS:

```toml
[discovery]
enabled = true
service_name = "_syncfleet._tcp.local."
```

Discovered nodes are only dialled once they are already in the local
`peer_authorizations` table, so discovery never bypasses enrollment.

### 6. Inspect and maintain

```bash
syncfleet journal stats
syncfleet journal compact --dry-run
syncfleet doctor --full
```

> With `[[peers]]` configured, `daemon start` also keeps a background sync task
> per peer (retrying with exponential backoff), so manual `sync now` is only
> needed for one-off syncs.
>
> **Conflict handling:** concurrent writes from different origins are detected
> and recorded in the `conflicts` table during apply. Resolve them manually with
> `syncfleet conflicts list|show|resolve`; automatic policy-based resolution is
> not implemented. mDNS interface binding is also still open.

---

## Install (Python wrapper)

A thin, dependency-free Python wrapper is published to PyPI. It is a launcher:
it forwards all arguments to the native `syncfleet` executable and passes
through its exit code. There is no Python reimplementation of the engine.

```bash
pip install syncfleet
syncfleet --help
syncfleet init --node store-a
```

The wrapper locates the native executable in this order:

1. `SYNCFLEET_BIN` — explicit path to the executable (highest priority),
2. a binary **bundled inside the installed wheel** (`syncfleet/bin/`),
3. `syncfleet` found on your `PATH` (skipping the wrapper itself).

Wheels produced by the release workflow bundle the native binary for their
platform, so `pip install syncfleet` works with no extra setup. If you install a
pure-Python wheel without a bundled binary, install the native CLI separately
and point `SYNCFLEET_BIN` at it:

```bash
export SYNCFLEET_BIN=/usr/local/bin/syncfleet
```

If no binary can be found, the wrapper exits `127` with a clear message rather
than failing silently.

---

## Current Status

**Phase 4 — Incremental replication.** Working today:

- Node identity and encrypted keystore (Ed25519 / X25519)
- Configuration loading, validation, and `config show`
- SQLite storage engine with migrations, WAL, journal, and idempotent
  remote-operation application
- Datasets and peer authorization (grant, revoke, retire)
- Self-signed TLS 1.3 certificates and a pinned (TOFU) trust store
- Mutually-authenticated peering handshake
- Single-use, signed enrollment tokens (issue / redeem)
- Version-vector checkpoint exchange and bounded, incremental operation
  streaming over the peering session (`sync now --peer`)
- Per-operation authorization (read + write) enforced on the wire
- Local operation journaling via `dataset apply` (for staging offline changes)
- `sync status` checkpoint reporting per origin
- `daemon start` serving loop over mTLS, plus a background sync task per
  `[[peers]]` entry with exponential backoff on unreachable peers
- mDNS peer discovery (`discovery.enabled`): advertises the node and feeds
  discovered, authorized peers into the sync loop
- `doctor` diagnostics and journal inspection/compaction
- Python wrapper + PyPI packaging

Not implemented yet: automatic/policy conflict resolution (concurrent writes
are detected and recorded for manual resolution), mDNS interface binding, and
daemon process control (`stop`/`restart`).

See [ROADMAP.md](docs/ROADMAP.md) for details.

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                        Application Layer                        │
├─────────────────────────────────────────────────────────────────┤
│                     SyncFleet Core Daemon                       │
│  ┌──────────────┐ ┌──────────────┐ ┌────────────────────────┐  │
│  │   Storage    │ │  Replication │ │     Peer Manager       │  │
│  │   Engine     │ │   Protocol   │ │  (Discovery/Transport) │  │
│  └──────────────┘ └──────────────┘ └────────────────────────┘  │
├─────────────────────────────────────────────────────────────────┤
│                    SQLite + TLS 1.3 + mDNS                      │
└─────────────────────────────────────────────────────────────────┘
```

---

## Documentation

- [Architecture](docs/ARCHITECTURE.md)
- [Roadmap](docs/ROADMAP.md)
- [Security Model](docs/SECURITY.md)
- [Consistency Guarantees](docs/CONSISTENCY.md)
- [Threat Model](docs/THREAT_MODEL.md)
- [Testing Strategy](docs/TESTING.md)
- [Operations Guide](docs/OPERATIONS.md)
- [Architecture Decisions](docs/DECISIONS.md)
- [Known Issues](docs/KNOWN_ISSUES.md)

## Fleet sync with PyTerrainMap (planned integration)

SyncFleet and [PyTerrainMap](https://github.com/Mullassery/PyTerrainMap) are
complementary tools. PyTerrainMap is an append-only, H3-indexed observation
store for multi-robot terrain data; SyncFleet replicates selected SQLite tables
between authorized peers. Used together, they are intended to give a
multi-robot edge swarm an out-of-the-box, decentralized map-sharing protocol:
each robot accumulates terrain observations locally, and SyncFleet's P2P mesh
propagates them to the rest of the fleet over mutual-TLS 1.3 — with no cloud
service or broker in the middle.

**Why they fit together**

- PyTerrainMap observations are **append-only** (never updated or deleted through
  the normal API), so the hard part of peer-to-peer replication — conflict
  resolution — is largely sidestepped: independent robot inserts never conflict.
- A PyTerrainMap observation table maps cleanly to a SyncFleet **dataset** with a
  table allowlist, and an observation's UUID maps to an operation's `entity_id`.
- Both are Rust + Tokio + rustls 0.23 / TLS 1.3, and both target local-first,
  edge deployments.
- SyncFleet's mutual-TLS 1.3 transport, enrollment, and per-dataset ACLs supply
  the transport security that a fleet map-sharing protocol needs.

**Status: not wired up yet.** This pairing is a design goal, not a working
feature. Three gaps must be closed first:

1. **State projection.** SyncFleet currently replicates its operation *journal*
   (dedup + checkpoint + journal append) but does not yet materialize operations
   into the allowlisted user tables, so a peer's observation table is not updated
   by a received operation.
2. **Storage substrate.** SyncFleet is SQLite-only; PyTerrainMap's live
   persistence is PostgreSQL (its SQLite backend is schema-only today). The two
   need a shared, live SQLite observation table (or a Postgres backend in
   SyncFleet).
3. **Write path.** Observations written through PyTerrainMap's API are not yet
   emitted as SyncFleet operations; an outbox/CDC bridge is needed so local
   writes enter the operation journal.

Until (1)–(3) land, treat the two as independent, complementary tools that share
a vocabulary — do not rely on them for automatic fleet map sharing.

## Part of the robotics & autonomy family

SyncFleet is one of six complementary, composable tools in the Mullassery **robotics & autonomy** family. Together they form a single stack: **perceive → map → simulate → learn → debug → coordinate the fleet**.

| Tool | Role | What it does |
|---|---|---|
| [PyRoboVision](https://github.com/Mullassery/PyRoboVision) | Perception | Multi-object tracker (Kalman filter + Hungarian algorithm) and trajectory predictor for robot perception. |
| [PyTerrainMap](https://github.com/Mullassery/PyTerrainMap) | Mapping / SLAM | Fuses multi-robot terrain observations into a spatial-temporal store, with H3 spatial indexing and SLAM. |
| [PyRoboSimulator](https://github.com/Mullassery/PyRoboSimulator) | Simulation | World simulator for autonomous systems, with a FastAPI backend. |
| [PyRoboFrames](https://github.com/Mullassery/PyRoboFrames) | Learning data | Rust-backed ML dataloader for robot-learning datasets (native LeRobot format, Apple Silicon hardware video decode). |
| [PyRoboReplay](https://github.com/Mullassery/PyRoboReplay) | Mission forensics | Forensic-debugging and causal-analysis engine for autonomous robot mission replay. |
| [SyncFleet](https://github.com/Mullassery/SyncFleet) | Fleet coordination | **This project** — Rust + SQLite, peer-to-peer state sync for robotics fleets — mutual-TLS 1.3, version-vector replication, mDNS discovery; conflict resolution on the roadmap. |

Each works standalone, but they're complementary by design. The planned outbox/CDC bridge to PyTerrainMap (detailed above) will allow terrain state to sync across fleets without a central database.

[Browse all projects →](https://github.com/Mullassery?tab=repositories)

**Also from Mullassery:** [TinyBridge](https://github.com/Mullassery/TinyBridge) — macOS-native Linux VM runtime for local edge testing · [PyDeepMesh](https://github.com/Mullassery/PyDeepMesh) — dependency-graph and vulnerability intelligence

## License

MIT License - see [LICENSE](LICENSE) for details.
