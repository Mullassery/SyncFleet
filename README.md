# SyncFleet

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
  pin certificates, and inspect the journal. **Automated data replication, the
  background daemon, conflict resolution, and mDNS discovery are on the
  [roadmap](docs/ROADMAP.md) and are not wired up yet** (see *Current Status*).
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

Requires a recent stable Rust toolchain (MSRV 1.75).

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

### 5. Inspect and maintain

```bash
syncfleet journal stats
syncfleet journal compact --dry-run
syncfleet doctor --full
```

> **Not yet available:** `sync status`, `sync now`, and `daemon start` exist in
> the CLI but currently report that they are not implemented. They will be
> enabled as replication lands (see [ROADMAP.md](docs/ROADMAP.md)).

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

**Phase 3 — Secure peering (in progress).** Working today:

- Node identity and encrypted keystore (Ed25519 / X25519)
- Configuration loading, validation, and `config show`
- SQLite storage engine with migrations, WAL, journal, and idempotent
  remote-operation application
- Datasets and peer authorization (grant, revoke, retire)
- Self-signed TLS 1.3 certificates and a pinned (TOFU) trust store
- Mutually-authenticated peering handshake
- Single-use, signed enrollment tokens (issue / redeem)
- `doctor` diagnostics and journal inspection/compaction
- Python wrapper + PyPI packaging

Not implemented yet: incremental replication, long-running daemon, conflict
resolution runtime, and mDNS auto-discovery.

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

## Related projects

Part of the Mullassery open-source family:

- [PyTerrainMap](https://github.com/Mullassery/PyTerrainMap) — multi-robot terrain intelligence and mapping
- [PyRoboSimulator](https://github.com/Mullassery/PyRoboSimulator) — simulation environment for autonomous systems
- [PyRoboFrames](https://github.com/Mullassery/PyRoboFrames) — high-performance robotics dataloader
- [TinyBridge](https://github.com/Mullassery/tinybridge) — macOS-native Linux VM runtime for local edge testing
- [PyDeepMesh](https://github.com/Mullassery/PyDeepMesh) — dependency-graph and vulnerability intelligence
- [Browse all projects →](https://github.com/Mullassery?tab=repositories)

## License

MIT License - see [LICENSE](LICENSE) for details.
