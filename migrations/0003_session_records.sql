CREATE TABLE session_records (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    global_session_id TEXT NOT NULL REFERENCES global_sessions(id),
    record_type TEXT NOT NULL CHECK (record_type IN ('decision', 'task')),
    content TEXT NOT NULL,
    origin TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX session_records_global_created
    ON session_records(global_session_id, created_at, id);
