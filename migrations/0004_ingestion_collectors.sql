CREATE TABLE ingestion_collectors (
    id TEXT PRIMARY KEY,
    label TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    revoked_at TEXT
);

CREATE TABLE ingestion_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    collector_id TEXT REFERENCES ingestion_collectors(id),
    occurred_at TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('accepted', 'rejected')),
    event_count INTEGER NOT NULL CHECK (event_count >= 0),
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    reason TEXT
);

CREATE INDEX ingestion_audit_log_collector
    ON ingestion_audit_log(collector_id, occurred_at, id);

-- NULL means the session was ingested by the local daemon rather than a
-- remote collector. No inline REFERENCES: a revoked/removed collector must
-- never cascade into deleting or orphaning already-ingested session history.
ALTER TABLE native_sessions ADD COLUMN origin_collector_id TEXT;
