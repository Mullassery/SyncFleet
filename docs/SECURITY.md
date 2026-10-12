# SyncFleet Security Model

## Assets

1. **Node Identity Keys** - Ed25519 private keys
2. **Replicated Data** - Application state in operation journal
3. **Enrollment Tokens** - Single-use peer authorization secrets
4. **Peer Certificates** - TLS certificates for mutual authentication
5. **Configuration** - Dataset policies, peer authorizations

## Trust Boundaries

```
┌─────────────────────────────────────────────────────────────┐
│                      Trusted Node                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐   │
│  │  Application │  │  SyncFleet   │  │  Local Storage   │   │
│  │   (Writer)   │──│    Core      │──│   (SQLite)       │   │
│  └──────────────┘  └──────┬───────┘  └──────────────────┘   │
│                           │                                  │
│                    ┌──────▼───────┐                           │
│                    │  Transport   │                           │
│                    │   (TLS 1.3)  │                           │
│                    └──────┬───────┘                           │
└───────────────────────────┼───────────────────────────────────┘
                            │
              ┌─────────────┼─────────────┐
              ▼             ▼             ▼
        ┌──────────┐ ┌──────────┐ ┌──────────┐
        │  Peer A  │ │  Peer B  │ │  Peer C  │
        │(Trusted) │ │(Trusted) │ │(Untrusted│
        └──────────┘ └──────────┘ │  until   │
                                  │ verified)│
                                  └──────────┘
```

## Attacker Capabilities

| Capability | Mitigation |
|------------|------------|
| Network eavesdropping | TLS 1.3 encryption |
| Message injection/modification | TLS 1.3 integrity + application checksums |
| Replay attacks | Operation IDs + deduplication + sequence numbers |
| Peer impersonation | Mutual TLS + certificate pinning |
| Enrollment token theft | Single-use, 24h expiry, out-of-band delivery |
| Compromised node | Peer revocation, per-dataset ACLs |
| Resource exhaustion | Bounded queues, message sizes, connection limits |
| Disk exhaustion | Journal retention, compaction, quotas |

## Cryptographic Primitives

| Purpose | Algorithm | Library |
|---------|-----------|---------|
| Node identity | Ed25519 | `ed25519-dalek` |
| TLS certificates | X.25519 / Ed25519 | `rustls` + `rcgen` |
| Key derivation | HKDF-SHA256 | `hkdf` |
| Token generation | CSPRNG (ChaCha20) | `rand` |
| Operation checksums | SHA-256 | `sha2` |
| Encryption at rest | AES-256-GCM | `aes-gcm` |

## Enrollment Protocol

```
Admin                          New Node
  │                                │
  │  Generate token (256-bit)      │
  │  ──────────────────────────►   │
  │         (out-of-band)          │
  │                                │
  │         TLS Handshake          │
  │  ◄──────────────────────────   │
  │  ClientHello + Token           │
  │                                │
  │  Validate token                │
  │  Issue peer certificate        │
  │  ──────────────────────────►   │
  │                                │
  │  Invalidate token              │
```

### Token Properties
- 256 bits entropy (32 bytes from CSPRNG)
- Single-use: invalidated after successful enrollment
- Time-limited: expires after 24 hours
- Never logged or persisted in plaintext
- Delivered via secure channel (QR code, encrypted file, SSH)

## Authorization Model

### Dataset-Level ACLs
```
PeerAuthorization {
    peer_id: NodeId,
    dataset_id: String,
    permissions: Permissions,  // READ=1, WRITE=2, ADMIN=4
    granted_at: DateTime,
    revoked_at: Option<DateTime>,
}
```

### Enforcement Points
1. **Connection**: Verify peer identity via TLS
2. **Hello**: Intersect authorized datasets
3. **SyncRequest**: Check READ permission
4. **SyncResponse**: Check WRITE permission
5. **Operation Apply**: Validate origin peer has WRITE

## Key Management

### Private Key Storage
- Encrypted at rest using AES-256-GCM
- Key derived from passphrase (Argon2id) or hardware-backed (TPM/HSM)
- File permissions: 0600
- Never written to logs

### Certificate Lifecycle
- Self-signed CA per deployment (or external PKI)
- Node certificates: 1 year validity
- Peer certificates: 90 days, auto-renewal
- Revocation: CRL or OCSP (future), immediate local reject list

### Rotation
- Manual `syncfleet key rotate` command
- Generates new identity, preserves old for verification
- Re-enrollment required for peers

## Threat Model Summary

| Threat | Likelihood | Impact | Mitigation |
|--------|------------|--------|------------|
| MITM on LAN | Medium | High | Mutual TLS, cert pinning |
| Stolen enrollment token | Low | High | Single-use, expiry, OOB delivery |
| Compromised peer | Low | Medium | Revocation, per-dataset ACLs |
| Malicious peer DoS | Medium | Medium | Rate limits, bounds |
| Journal tampering | Low | High | Checksums, append-only, fsync |
| Schema injection | Low | High | Schema validation, versioning |
| Rollback attack | Low | High | Monotonic sequences, checkpoints |

## Residual Risks

1. **Physical access** - Full disk encryption recommended
2. **Insider threat** - Audit logging, separation of duties
3. **Supply chain** - Dependency auditing, locked Cargo.lock
4. **Side channels** - Not addressed in MVP
5. **Quantum** - Ed25519 not post-quantum; monitor NIST PQC

## Vulnerability Reporting

Report security issues to: security@syncfleet.example.com (placeholder)

Do not file public issues for suspected vulnerabilities.