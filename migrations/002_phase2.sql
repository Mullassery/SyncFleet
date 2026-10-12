-- Phase 2: durable local sequence allocation and journal maintenance.
-- Version 2

-- Per-node monotonic sequence allocator. Local operations draw their
-- origin_seq from here inside the same transaction that inserts the journal
-- row, guaranteeing gap-free, unique ordering per origin node.
CREATE TABLE node_local_state (
    node_id TEXT PRIMARY KEY,
    last_seq INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);

INSERT INTO schema_version (version, applied_at) VALUES (2, datetime('now'));
