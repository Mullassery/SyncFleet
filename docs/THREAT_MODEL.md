# SyncFleet Threat Model

## System Boundaries

```
┌─────────────────────────────────────────────────────────────────────┐
│                        SyncFleet Node                                │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌───────────┐  │
│  │ Application │  │   SyncFleet │  │   SQLite    │  │   Keys    │  │
│  │   (Writer)  │──│    Core     │──│  (Journal)  │  │ (Encrypted)│  │
│  └─────────────┘  └──────┬──────┘  └─────────────┘  └───────────┘  │
│                         │                                            │
│                  ┌──────▼──────┐                                    │
│                  │  Transport  │                                    │
│                  │   (TLS 1.3) │                                    │
│                  └──────┬──────┘                                    │
└─────────────────────────┼───────────────────────────────────────────┘
                          │
          ┌───────────────┼───────────────┐
          ▼               ▼               ▼
    ┌───────────┐   ┌───────────┐   ┌───────────┐
    │  Peer A   │   │  Peer B   │   │  Attacker │
    │ (Trusted) │   │ (Trusted) │   │ (Network) │
    └───────────┘   └───────────┘   └───────────┘
```

## Assets and Sensitivity

| Asset | Sensitivity | Impact if Compromised |
|-------|-------------|----------------------|
| Node private key | CRITICAL | Full node impersonation, data decryption |
| Peer private keys | HIGH | Peer impersonation |
| Enrollment tokens | HIGH | Unauthorized peer enrollment |
| Operation journal | MEDIUM | Data exposure, tampering |
| Dataset content | VARIES | Application-dependent |
| Configuration | MEDIUM | Policy bypass, misconfiguration |
| TLS certificates | HIGH | MITM, impersonation |

## Threat Actors

| Actor | Motivation | Capability | Access |
|-------|------------|------------|--------|
| External network attacker | Data theft, disruption | Network control, MITM | Network only |
| Compromised peer | Data exfiltration, sabotage | Full node control | Authenticated peer |
| Malicious insider | Sabotage, espionage | Admin access | Local + network |
| Supply chain attacker | Backdoor injection | Dependency compromise | Build/deploy |
| Physical attacker | Key extraction | Hardware access | Physical |

## Attack Trees

### T1: Unauthorized Data Access
```
T1: Read replicated data without authorization
├── T1.1: Network eavesdrop on sync traffic
│   └── MITIGATED: TLS 1.3 encryption
├── T1.2: Impersonate authorized peer
│   ├── T1.2.1: Steal peer private key
│   │   └── MITIGATED: Encrypted storage, 0600 perms
│   ├── T1.2.2: Forge TLS certificate
│   │   └── MITIGATED: Mutual auth, cert pinning
│   └── T1.2.3: Replay enrollment token
│       └── MITIGATED: Single-use, expiry, OOB delivery
├── T1.3: Exploit authorization bypass
│   └── MITIGATED: Per-dataset ACLs checked on every operation
└── T1.4: Access local SQLite directly
    └── MITIGATED: File permissions, encryption at rest
```

### T2: Unauthorized Data Modification
```
T2: Write/modify replicated data without authorization
├── T2.1: Inject malicious operations
│   ├── T2.1.1: Forge operation with valid peer identity
│   │   └── MITIGATED: Mutual TLS + operation origin validation
│   ├── T2.1.2: Replay old operation
│   │   └── MITIGATED: Sequence numbers, deduplication
│   └── T2.1.3: Modify operation in transit
│       └── MITIGATED: TLS integrity + SHA-256 checksums
├── T2.2: Bypass single-writer enforcement
│   └── MITIGATED: Writer designation checked at origin + recipient
├── T2.3: Corrupt local journal
│   └── MITIGATED: Append-only, checksums, integrity check on startup
└── T2.4: Rollback to old state
    └── MITIGATED: Monotonic sequences, checkpoint validation
```

### T3: Availability / Denial of Service
```
T3: Prevent legitimate synchronization
├── T3.1: Network-level DoS
│   └── MITIGATED: Rate limits, connection bounds, backoff
├── T3.2: Resource exhaustion via protocol
│   ├── T3.2.1: Oversized messages
│   │   └── MITIGATED: 1MB message limit
│   ├── T3.2.2: Excessive connections
│   │   └── MITIGATED: Max 64 concurrent peers
│   └── T3.2.3: Rapid reconnection storms
│       └── MITIGATED: Exponential backoff + jitter
├── T3.3: Disk exhaustion
│   ├── T3.3.1: Unbounded journal growth
│   │   └── MITIGATED: Retention policy, compaction
│   └── T3.3.2: Large snapshot transfers
│       └── MITIGATED: Chunked transfer, size limits
└── T3.4: CPU exhaustion
    └── MITIGATED: Bounded parsing, async I/O
```

### T4: Data Integrity Violation
```
T4: Cause silent data corruption
├── T4.1: Bit flip in storage
│   └── MITIGATED: SQLite checksums, fsync, ZFS/btrfs recommended
├── T4.2: Malformed operation accepted
│   └── MITIGATED: Schema validation, checksum verification
├── T4.3: Schema confusion attack
│   └── MITIGATED: Explicit schema version per operation
├── T4.4: Checkpoint manipulation
│   └── MITIGATED: Checkpoint tied to peer+dataset, validated on apply
└── T4.5: Time-based attacks (clock skew)
    └── MITIGATED: No wall-clock dependency for correctness
```

## Mitigations Summary

### Cryptographic
- [x] Ed25519 for node identity
- [x] TLS 1.3 mutual authentication
- [x] AES-256-GCM for encryption at rest
- [x] SHA-256 for integrity
- [x] HKDF for key derivation
- [x] CSPRNG for tokens/secrets

### Protocol
- [x] Operation IDs (UUIDv7) for deduplication
- [x] Per-origin sequence numbers for ordering
- [x] Checkpoints for progress tracking
- [x] Schema versioning
- [x] Bounded messages, queues, connections
- [x] Exponential backoff with jitter

### Operational
- [x] Single-use enrollment tokens
- [x] Peer revocation
- [x] Per-dataset ACLs
- [x] Encrypted private key storage
- [x] File permission hardening
- [x] Structured audit logging (no secrets)

### Remaining Gaps (Post-MVP)
- [ ] Post-quantum cryptography migration path
- [ ] Hardware security module (HSM/TPM) integration
- [ ] Formal verification of protocol
- [ ] Third-party security audit
- [ ] Certificate transparency / OCSP stapling
- [ ] Secure enclave support for keys

## STRIDE Analysis

| Threat | Vectors | Mitigations |
|--------|---------|-------------|
| **S**poofing | Peer impersonation, enrollment token theft | Mutual TLS, cert pinning, single-use tokens |
| **T**ampering | Message modification, journal corruption | TLS integrity, SHA-256 checksums, append-only journal |
| **R**epudiation | Deny sending operation | Operation signatures (future), audit log |
| **I**nformation Disclosure | Eavesdropping, key extraction | TLS encryption, encrypted storage, no secret logging |
| **D**enial of Service | Resource exhaustion, network flood | Bounds, rate limits, backoff |
| **E**levation of Privilege | AuthZ bypass, writer role theft | Per-dataset ACLs, single-writer enforcement |

## Security Testing Requirements

1. **Unit**: All validation functions, crypto operations
2. **Integration**: AuthZ enforcement, revocation, token replay
3. **Fuzzing**: Protocol message parsing, malformed inputs
4. **Penetration**: Network-level attacks, privilege escalation
5. **Dependency**: `cargo audit`, `cargo deny` in CI

## Incident Response

1. **Key compromise**: `syncfleet key rotate`, re-enroll peers
2. **Peer compromise**: `syncfleet peer revoke`, audit replicated data
3. **Token leak**: Invalidate token, rotate if used
4. **Data breach**: Assess dataset sensitivity, notify stakeholders