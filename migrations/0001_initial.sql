PRAGMA foreign_keys = ON;

CREATE TABLE tool_families (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL
);

CREATE TABLE tool_profiles (
    id TEXT PRIMARY KEY,
    family_id TEXT NOT NULL REFERENCES tool_families(id),
    version TEXT NOT NULL,
    profile_json TEXT NOT NULL
);

CREATE TABLE tool_installations (
    id TEXT PRIMARY KEY,
    family_id TEXT NOT NULL REFERENCES tool_families(id),
    surface TEXT NOT NULL,
    version TEXT,
    detected_at TEXT NOT NULL
);

CREATE TABLE source_locations (
    id TEXT PRIMARY KEY,
    installation_id TEXT REFERENCES tool_installations(id),
    readable_path TEXT NOT NULL,
    original_path TEXT,
    read_only INTEGER NOT NULL CHECK (read_only = 1)
);

CREATE TABLE native_sessions (
    id TEXT PRIMARY KEY,
    tool_family TEXT NOT NULL,
    surface TEXT NOT NULL,
    profile TEXT NOT NULL,
    started_at TEXT,
    ended_at TEXT
);

CREATE TABLE raw_objects (
    id TEXT PRIMARY KEY,
    blob_hash TEXT NOT NULL,
    metadata_hash TEXT NOT NULL,
    source_path TEXT NOT NULL,
    original_path TEXT,
    source_offset INTEGER NOT NULL CHECK (source_offset >= 0),
    source_size INTEGER NOT NULL CHECK (source_size >= 0),
    source_modified_at TEXT,
    source_permissions INTEGER,
    source_generation TEXT NOT NULL,
    parser_version TEXT NOT NULL,
    imported_at TEXT NOT NULL
);

CREATE TABLE native_events (
    event_id TEXT PRIMARY KEY,
    native_session_id TEXT NOT NULL REFERENCES native_sessions(id),
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    timestamp TEXT NOT NULL,
    kind TEXT NOT NULL,
    canonical_json TEXT NOT NULL,
    raw_object_id TEXT REFERENCES raw_objects(id),
    source_offset INTEGER NOT NULL CHECK (source_offset >= 0),
    UNIQUE (native_session_id, sequence, event_id)
);

CREATE INDEX native_events_session_sequence
    ON native_events(native_session_id, sequence, event_id);

CREATE VIRTUAL TABLE native_events_fts USING fts5(
    event_id UNINDEXED,
    searchable_text
);

CREATE TABLE repositories (
    id TEXT PRIMARY KEY,
    root_path TEXT NOT NULL,
    remote_url TEXT
);

CREATE TABLE worktrees (
    id TEXT PRIMARY KEY,
    repository_id TEXT NOT NULL REFERENCES repositories(id),
    root_path TEXT NOT NULL,
    branch TEXT,
    head TEXT,
    dirty INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE global_sessions (
    id TEXT PRIMARY KEY,
    objective TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE global_session_members (
    global_session_id TEXT NOT NULL REFERENCES global_sessions(id),
    native_session_id TEXT NOT NULL REFERENCES native_sessions(id),
    confidence REAL NOT NULL CHECK (confidence BETWEEN 0 AND 1),
    correlation_version TEXT NOT NULL,
    manual_state TEXT,
    PRIMARY KEY (global_session_id, native_session_id)
);

CREATE TABLE correlation_candidates (
    id TEXT PRIMARY KEY,
    left_native_session_id TEXT NOT NULL REFERENCES native_sessions(id),
    right_native_session_id TEXT NOT NULL REFERENCES native_sessions(id),
    score REAL NOT NULL CHECK (score BETWEEN 0 AND 1),
    status TEXT NOT NULL
);

CREATE TABLE correlation_evidence (
    id TEXT PRIMARY KEY,
    candidate_id TEXT NOT NULL REFERENCES correlation_candidates(id),
    evidence_type TEXT NOT NULL,
    weight REAL NOT NULL,
    evidence_json TEXT NOT NULL
);

CREATE TABLE state_snapshots (
    id TEXT PRIMARY KEY,
    global_session_id TEXT NOT NULL REFERENCES global_sessions(id),
    created_at TEXT NOT NULL,
    snapshot_json TEXT NOT NULL
);

CREATE TABLE handoffs (
    id TEXT PRIMARY KEY,
    global_session_id TEXT NOT NULL REFERENCES global_sessions(id),
    snapshot_id TEXT NOT NULL REFERENCES state_snapshots(id),
    schema_version TEXT NOT NULL,
    handoff_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE ingestion_cursors (
    source_id TEXT PRIMARY KEY,
    source_generation TEXT NOT NULL,
    byte_offset INTEGER NOT NULL CHECK (byte_offset >= 0),
    next_sequence INTEGER NOT NULL CHECK (next_sequence >= 0),
    partial_line BLOB NOT NULL DEFAULT X'',
    updated_at TEXT NOT NULL
);

CREATE TABLE adapter_runs (
    id TEXT PRIMARY KEY,
    adapter_name TEXT NOT NULL,
    adapter_version TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT NOT NULL,
    diagnostic TEXT
);

CREATE TABLE security_findings (
    id TEXT PRIMARY KEY,
    finding_type TEXT NOT NULL,
    severity TEXT NOT NULL,
    source_reference TEXT,
    created_at TEXT NOT NULL,
    resolved_at TEXT
);

CREATE TABLE user_overrides (
    id TEXT PRIMARY KEY,
    override_type TEXT NOT NULL,
    target_id TEXT NOT NULL,
    value_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
