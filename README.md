# SyncFleet

[![Graphify](https://img.shields.io/badge/Graphify-knowledge%20graph-blue)](https://github.com/Graphify-Labs/graphify)

A secure, local-first, peer-to-peer edge database synchronization engine.

## Vision

SyncFleet enables authorized devices to replicate selected application state across independent nodes without requiring a continuously available cloud synchronization service or mandatory central message broker.

## Target Environments

- Raspberry Pi and Linux edge gateways
- Industrial IoT gateways and sensor aggregation systems
- Retail branches and geographically distributed stores
- Local development environments and small computing clusters
- Private LANs and WireGuard-connected networks
- Applications that must continue working during internet outages
- Developer tools that exchange selected local state and test results

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

## Current Status

**Phase 1: Project Foundation and CLI** - In Progress

See [ROADMAP.md](docs/ROADMAP.md) for detailed implementation status.

## Quick Start

```bash
# Build
cargo build --release

# Initialize a node
syncfleet init --node store-a

# Start daemon
syncfleet daemon start

# Check status
syncfleet status
```

## Install

### From PyPI (Python wrapper)

A thin, dependency-free Python wrapper is published to PyPI. It forwards to the
native `syncfleet` executable, which it finds in this order:

1. `SYNCFLEET_BIN` environment variable (path to the executable),
2. a binary bundled inside the installed wheel,
3. `syncfleet` on your `PATH`.

```bash
pip install syncfleet
syncfleet --help
```

Wheels published by the release workflow bundle the native binary for their
platform, so `pip install syncfleet` works with no extra setup. If you install
the pure-Python wheel without a bundled binary, install the CLI separately and
point `SYNCFLEET_BIN` at it.


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

## License

MIT License - see [LICENSE](LICENSE) for details.
