# SyncFleet documentation

## Start here

| Document | What it covers |
|---|---|
| [Install](INSTALL.md) | Native CLI vs Python wrapper, platform matrix, `SYNCFLEET_BIN` |
| [README](../README.md) | Overview, quick start, enrollment, replication |
| [Project status](STATUS.md) | What works, verified gaps, deferred work |
| [Comparison](COMPARISON.md) | How this differs from Electric and PowerSync |

## Core concepts

| Document | What it covers |
|---|---|
| [Architecture](ARCHITECTURE.md) | Components, storage, transport, sync loop |
| [Consistency guarantees](CONSISTENCY.md) | Version vectors, conflict policies, ordering |
| [Security](SECURITY.md) | Identity, TLS, enrollment, authorization |
| [Threat model](THREAT_MODEL.md) | Assets, adversaries, assumptions |
| [Decisions](DECISIONS.md) | Architecture decision records |

## Operating SyncFleet

| Document | What it covers |
|---|---|
| [Operations guide](OPERATIONS.md) | Init, datasets, peers, sync, daemon, journal, doctor |
| [Testing strategy](TESTING.md) | Test layers and how to run them |
| [Known issues](KNOWN_ISSUES.md) | Known defects and limitations |
| [Roadmap](ROADMAP.md) | Shipped and planned work |

## Maintainer / policy

- [`internal/QUEUE_STATUS.md`](internal/QUEUE_STATUS.md) — shipped/blocked/rejected summary
- [`internal/AUDIT_FINDINGS.md`](internal/AUDIT_FINDINGS.md) — repository audit notes
- [`internal/BUNDLE_ROLE.md`](internal/BUNDLE_ROLE.md) — portfolio positioning
- [`internal/COMPETITIVE_INTELLIGENCE.md`](internal/COMPETITIVE_INTELLIGENCE.md) — competitive process
