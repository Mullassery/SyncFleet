# SyncFleet Operations Guide

## Deployment

### System Requirements
- Linux (primary), macOS (development)
- Rust 1.75+ (MSRV)
- SQLite 3.35+ (WAL mode)
- 50MB RAM minimum, 200MB recommended
- 1GB disk for journal (configurable)

### Installation

#### From Release (Future)
```bash
# Linux
curl -L https://github.com/Mullassery/SyncFleet/releases/latest/download/syncfleet-x86_64-unknown-linux-gnu.tar.gz | tar xz
sudo mv syncfleet /usr/local/bin/

# macOS
brew install mullassery/tap/syncfleet
```

#### From Source
```bash
git clone https://github.com/Mullassery/SyncFleet
cd SyncFleet
cargo install --path . --locked
```

### Directory Structure
```
/etc/syncfleet/
  config.toml          # Main configuration
  certs/
    ca.pem             # CA certificate
    node.pem           # Node certificate
    node-key.pem       # Node private key (0600)
  peers/
    <peer-id>.pem      # Peer certificates

/var/lib/syncfleet/
  syncfleet.db         # SQLite metadata + journal
  snapshots/           # Snapshot files

/var/log/syncfleet/
  syncfleet.log        # Structured logs (JSON)
```

### systemd Service
```ini
# /etc/systemd/system/syncfleet.service
[Unit]
Description=SyncFleet Synchronization Daemon
After=network-online.target
Wants=network-online.target

[Service]
Type=notify
User=syncfleet
Group=syncfleet
ExecStart=/usr/local/bin/syncfleet daemon start --config /etc/syncfleet/config.toml
ExecReload=/bin/kill -HUP $MAINPID
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal
SyslogIdentifier=syncfleet

# Security
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ReadWritePaths=/var/lib/syncfleet /var/log/syncfleet /etc/syncfleet
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes

[Install]
WantedBy=multi-user.target
```

Enable: `sudo systemctl enable --now syncfleet`

### Docker Compose
```yaml
# docker-compose.yml
version: '3.8'
services:
  syncfleet:
    image: ghcr.io/mullassery/syncfleet:latest
    volumes:
      - ./config:/etc/syncfleet:ro
      - ./data:/var/lib/syncfleet
      - ./logs:/var/log/syncfleet
    ports:
      - "8443:8443"   # Sync port
      - "9090:9090"   # Metrics (optional)
    environment:
      - RUST_LOG=info
      - SYNCFLEET_CONFIG=/etc/syncfleet/config.toml
    restart: unless-stopped
    cap_drop:
      - ALL
    cap_add:
      - NET_BIND_SERVICE
    read_only: true
    tmpfs:
      - /run
      - /tmp
```

## Configuration

See `config/syncfleet.example.toml` for complete reference.

### Key Settings
```toml
[node]
node_name = "store-a"
data_dir = "/var/lib/syncfleet"
log_level = "info"

[network]
listen_addr = "0.0.0.0:8443"
advertise_addr = "auto"  # or explicit "host:port"
tls_cert = "/etc/syncfleet/certs/node.pem"
tls_key = "/etc/syncfleet/certs/node-key.pem"
ca_cert = "/etc/syncfleet/certs/ca.pem"

[discovery]
enabled = true
interface = "eth0"
port = 5353

[storage]
journal_retention_days = 30
journal_max_size_gb = 1
snapshot_interval_hours = 24
compaction_interval_hours = 6

[replication]
max_concurrent_peers = 32
max_in_flight = 32
max_message_size_mb = 1
sync_interval_sec = 30

[metrics]
enabled = true
listen_addr = "127.0.0.1:9090"
```

## CLI Reference

### Node Management
```bash
# Initialize node (generates keys, creates config)
syncfleet init --node store-a [--config /path/to/config.toml]

# Show node status
syncfleet status

# Validate configuration
syncfleet config validate

# Show effective configuration
syncfleet config show
```

### Dataset Management
```bash
# List datasets
syncfleet dataset list

# Register dataset
syncfleet dataset register --name orders --tables orders,order_items --policy single_writer

# Show dataset details
syncfleet dataset show orders

# Remove dataset (local only, doesn't affect peers)
syncfleet dataset remove orders
```

### Peer Management
```bash
# List known peers
syncfleet peer list

# Pair with peer (enrollment)
syncfleet peer pair --address 192.168.1.100:8443 --token <enrollment-token>

# Show peer details
syncfleet peer show <peer-id>

# Revoke peer
syncfleet peer revoke <peer-id>

# Retire peer (for GC)
syncfleet peer retire <peer-id>
```

### Synchronization
```bash
# Show sync status for all datasets
syncfleet sync status

# Show sync status for specific dataset
syncfleet sync status --dataset orders

# Force sync now
syncfleet sync now --dataset orders
```

### Conflict Management
```bash
# List conflicts
syncfleet conflicts list

# Show conflict details
syncfleet conflicts show <conflict-id>

# Resolve conflict (admin)
syncfleet conflicts resolve <conflict-id> --resolution keep_local|keep_remote|merge
```

### Diagnostics
```bash
# Run health checks
syncfleet doctor

# Show metrics snapshot
syncfleet metrics

# Export Prometheus metrics
curl http://localhost:9090/metrics
```

### Daemon Control
```bash
# Start daemon (foreground)
syncfleet daemon start

# Start daemon (background, systemd-style)
syncfleet daemon start --background

# Stop daemon gracefully
syncfleet daemon stop

# Restart daemon
syncfleet daemon restart
```

## Monitoring

### Key Metrics

| Metric | Type | Description | Alert Threshold |
|--------|------|-------------|-----------------|
| `syncfleet_peer_connected` | Gauge | 1 if peer connected | < expected peers |
| `syncfleet_replication_lag_seconds` | Gauge | Time since last successful sync | > 300s |
| `syncfleet_pending_outgoing_ops` | Gauge | Operations waiting to send | > 10000 |
| `syncfleet_pending_incoming_ops` | Gauge | Operations received not applied | > 1000 |
| `syncfleet_sync_errors_total` | Counter | Total sync errors | Rate > 0.1/s |
| `syncfleet_auth_failures_total` | Counter | Authentication failures | Rate > 0.01/s |
| `syncfleet_conflicts_total` | Counter | Conflicts detected | Any increase |
| `syncfleet_journal_size_bytes` | Gauge | Journal disk usage | > 80% of max |
| `syncfleet_db_size_bytes` | Gauge | SQLite database size | Monitor trend |

### Health Checks
```bash
# Liveness (process running)
systemctl is-active syncfleet

# Readiness (can serve sync)
syncfleet status --format json | jq '.ready'

# Deep health
syncfleet doctor --format json
```

### Log Format
JSON structured logs:
```json
{
  "timestamp": "2024-01-15T10:30:45.123Z",
  "level": "INFO",
  "target": "syncfleet::replication",
  "message": "Sync completed",
  "fields": {
    "peer_id": "a1b2c3d4...",
    "dataset_id": "orders",
    "operations_synced": 42,
    "duration_ms": 156
  }
}
```

Sensitive fields (keys, tokens, payloads) are **never** logged by default.

## Backup and Recovery

### Automated Backup
```bash
#!/bin/bash
# /usr/local/bin/syncfleet-backup.sh
set -euo pipefail

BACKUP_DIR="/backup/syncfleet/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$BACKUP_DIR"

# Snapshot database (requires read-only access)
sqlite3 /var/lib/syncfleet/syncfleet.db ".backup '$BACKUP_DIR/syncfleet.db'"

# Backup configs and certs
cp -r /etc/syncfleet "$BACKUP_DIR/config"

# Verify
sqlite3 "$BACKUP_DIR/syncfleet.db" "PRAGMA integrity_check;"

# Retain 30 days
find /backup/syncfleet -type d -mtime +30 -exec rm -rf {} \;
```

### Recovery Procedures

#### Scenario 1: Node Failure, Data Intact
1. Provision new node
2. Restore `/etc/syncfleet` and `/var/lib/syncfleet`
3. Start service: `systemctl start syncfleet`
4. Verify: `syncfleet status`

#### Scenario 2: Database Corruption
1. Stop service
2. Run integrity check: `sqlite3 syncfleet.db "PRAGMA integrity_check;"`
3. If failed: restore from latest backup
4. If passed but sync issues: `syncfleet doctor --repair-journal`

#### Scenario 3: Full Disaster Recovery
1. Provision new node
2. Run `syncfleet init --node <original-name>`
3. Restore configs and certs from backup
4. Start daemon
5. Peers will detect new node and push missing operations
6. Verify convergence: `syncfleet sync status`

#### Scenario 4: Peer Compromise
1. `syncfleet peer revoke <compromised-peer-id>`
2. Rotate node keys: `syncfleet key rotate`
3. Re-enroll legitimate peers
4. Audit replicated data for unauthorized changes

## Certificate Management

### Initial CA Setup
```bash
# Generate CA
openssl req -x509 -newkey ed25519 -keyout ca-key.pem -out ca.pem -days 3650 -subj "/CN=SyncFleet CA"

# Generate node cert
openssl req -newkey ed25519 -keyout node-key.pem -out node.csr -subj "/CN=store-a"
openssl x509 -req -in node.csr -CA ca.pem -CAkey ca-key.pem -CAcreateserial -out node.pem -days 365

# Set permissions
chmod 600 ca-key.pem node-key.pem
chmod 644 ca.pem node.pem
```

### Rotation
```bash
# Rotate node identity (generates new keys, preserves old for verification)
syncfleet key rotate

# Rotate peer certificate (admin on peer node)
syncfleet peer reissue <peer-id>
```

## Troubleshooting

### Common Issues

| Symptom | Likely Cause | Resolution |
|---------|--------------|------------|
| Peer connection refused | Wrong port/firewall | Check `listen_addr`, firewall rules |
| TLS handshake failure | Cert mismatch, expired | Verify certs, check expiry, regenerate |
| Sync stuck at 0% | No authorized datasets | Check `peer list`, `dataset list`, ACLs |
| High replication lag | Network, large backlog | Check network, increase `sync_interval` |
| Conflicts increasing | Multi-writer, clock issues | Verify single-writer, check policy |
| Disk usage growing | Retention not working | Check compaction job, retention config |

### Debug Mode
```bash
# Increase log level
syncfleet daemon start --log-level debug

# Or via config
RUST_LOG=syncfleet=debug,tls=trace syncfleet daemon start
```

### Diagnostic Commands
```bash
# Full diagnostic report
syncfleet doctor --full --output report.json

# Database analysis
sqlite3 /var/lib/syncfleet/syncfleet.db <<'EOF'
-- Journal size
SELECT COUNT(*), SUM(LENGTH(payload)) FROM operation_journal;
-- Checkpoints
SELECT * FROM replication_checkpoints;
-- Conflicts
SELECT * FROM conflicts ORDER BY detected_at DESC LIMIT 10;
EOF
```

## Upgrades

### Rolling Upgrade (Multi-Node)
1. Upgrade one node at a time
2. Verify sync works with older peers
3. Protocol version negotiation handles compatibility
4. No simultaneous schema changes

### Breaking Changes
- Announced 2 releases in advance
- Migration guide in CHANGELOG
- `syncfleet migrate` command for schema changes

## Security Hardening Checklist

- [ ] Run as dedicated user (`syncfleet`)
- [ ] Restrict file permissions (0600 for keys, 0700 for cert dir)
- [ ] Enable firewall (only 8443/tcp + 5353/udp for mDNS)
- [ ] Disable discovery if not needed
- [ ] Use hardware-backed keys (TPM/HSM) if available
- [ ] Regular `cargo audit` in CI/CD
- [ ] Monitor auth failures
- [ ] Rotate certificates annually
- [ ] Audit peer authorizations quarterly
- [ ] Test disaster recovery annually