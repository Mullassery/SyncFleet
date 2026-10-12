CREATE TABLE IF NOT EXISTS enrollment_tokens (
    token_hash TEXT PRIMARY KEY,
    issuer_node_id TEXT NOT NULL,
    dataset_id TEXT NOT NULL,
    permissions INTEGER NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    used_at TEXT
);

CREATE TABLE IF NOT EXISTS peer_certificates (
    peer_id TEXT PRIMARY KEY,
    cert_der BLOB NOT NULL,
    fingerprint TEXT NOT NULL UNIQUE,
    pinned_at TEXT NOT NULL
);

INSERT INTO schema_version (version, applied_at)
VALUES (3, datetime('now'))
ON CONFLICT(version) DO NOTHING;