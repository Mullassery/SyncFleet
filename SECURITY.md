# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.x.x   | :white_check_mark: (current development) |

SyncFleet is pre-1.0. Security fixes will be applied to the latest main branch.

## Reporting a Vulnerability

**Do not file public issues for security vulnerabilities.**

Report security issues privately to: **security@syncfleet.example.com** (placeholder)

Include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

We will:
- Acknowledge receipt within 48 hours
- Provide initial assessment within 7 days
- Keep you informed of progress
- Credit you in the fix (if desired)

## Disclosure Timeline

- **Day 0**: Report received
- **Day 2**: Acknowledgment sent
- **Day 7**: Initial assessment
- **Day 30**: Target for fix (may vary by severity)
- **Day 30+**: Coordinated disclosure after fix released

## Security Best Practices for Users

1. **Keep Updated**: Pull latest main branch regularly
2. **Verify Downloads**: Check signatures/hashes of releases
3. **Protect Keys**: Store private keys with 0600 permissions
4. **Use TLS**: Never disable mutual authentication
5. **Restrict Network**: Firewall sync port (8443) to trusted networks
6. **Monitor Logs**: Watch for authentication failures
7. **Rotate Certificates**: Annually or per policy
8. **Audit Peers**: Review authorized peers quarterly

## Threat Model

See [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) for detailed threat analysis.

## Cryptography

- Node identity: Ed25519
- Transport: TLS 1.3 (rustls)
- Encryption at rest: AES-256-GCM
- Key derivation: HKDF-SHA256
- Random: ChaCha20 (CSPRNG)

## Known Limitations

- No HSM/TPM support (planned)
- No post-quantum cryptography (monitoring NIST)
- No formal security audit completed
- Enrollment token delivery channel not enforced by software

## Dependency Security

- `cargo audit` runs in CI
- `cargo deny` checks licenses and bans
- Dependencies pinned in Cargo.lock
- Regular dependency updates

## Bug Bounty

No formal bug bounty program at this time.