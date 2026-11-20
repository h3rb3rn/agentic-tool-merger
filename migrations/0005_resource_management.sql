-- Persistent policy is shared by daemon, MCP processes, and collectors.
CREATE TABLE storage_settings (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);
INSERT INTO storage_settings VALUES ('conversation_only', 0), ('overview_backfill_complete', 0);
CREATE TABLE blob_cleanup_queue (blob_hash TEXT PRIMARY KEY);
CREATE INDEX raw_objects_blob_hash ON raw_objects(blob_hash);
CREATE INDEX handoffs_latest ON handoffs(global_session_id, created_at DESC, id DESC);

-- An explicit integer primary key keeps ingestion positions stable through VACUUM.
CREATE TABLE native_events_retained (
    ingestion_id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL UNIQUE,
    native_session_id TEXT NOT NULL REFERENCES native_sessions(id),
    sequence INTEGER NOT NULL CHECK(sequence >= 0),
    timestamp TEXT NOT NULL,
    kind TEXT NOT NULL,
    canonical_json TEXT NOT NULL,
    raw_object_id TEXT REFERENCES raw_objects(id),
    source_offset INTEGER NOT NULL CHECK(source_offset >= 0)
);
INSERT INTO native_events_retained
    SELECT rowid, event_id, native_session_id, sequence, timestamp, kind,
           canonical_json, raw_object_id, source_offset FROM native_events;
DROP TABLE native_events;
ALTER TABLE native_events_retained RENAME TO native_events;
CREATE INDEX native_events_session_sequence ON native_events(native_session_id,sequence,event_id);

-- Payload-free projections plus an exact, deterministic chronology index.
CREATE TABLE event_overviews (
    event_id TEXT PRIMARY KEY REFERENCES native_events(event_id) ON DELETE CASCADE,
    native_session_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    timestamp TEXT NOT NULL,
    timestamp_seconds INTEGER NOT NULL,
    timestamp_nanos INTEGER NOT NULL,
    kind TEXT NOT NULL,
    cwd TEXT,
    branch TEXT,
    source_generation TEXT NOT NULL,
    source_offset INTEGER NOT NULL,
    ingestion_sequence INTEGER NOT NULL
);
CREATE INDEX event_overviews_order ON event_overviews(
    timestamp_seconds, timestamp_nanos, sequence, source_generation, source_offset, ingestion_sequence, event_id
);
CREATE INDEX event_overviews_session ON event_overviews(native_session_id, timestamp_seconds, timestamp_nanos, sequence, source_generation, source_offset, ingestion_sequence, event_id);

CREATE INDEX native_events_raw_object ON native_events(raw_object_id);
-- Index content only once, using native rowids for O(1) maintenance deletes.
DROP TABLE native_events_fts;
CREATE VIRTUAL TABLE native_events_fts USING fts5(event_id UNINDEXED, searchable_text);

CREATE INDEX native_events_kind ON native_events(kind);

-- Session list reads must not parse or sum the accumulated transcript on each poll.
CREATE TABLE session_statistics (
    native_session_id TEXT PRIMARY KEY REFERENCES native_sessions(id),
    event_count INTEGER NOT NULL,
    content_bytes INTEGER NOT NULL,
    title TEXT,
    title_priority INTEGER NOT NULL,
    title_sequence INTEGER NOT NULL,
    title_event_id TEXT NOT NULL
);

-- Small metadata tombstones prevent old parsers from resurrecting known legacy tool dumps.
CREATE TABLE legacy_discarded_events (event_id TEXT PRIMARY KEY);

CREATE INDEX event_overviews_workspace_order ON event_overviews(
    rtrim(cwd, '/'), timestamp_seconds DESC, timestamp_nanos DESC, sequence DESC, source_generation DESC,
    source_offset DESC, ingestion_sequence DESC, event_id DESC
);
CREATE INDEX global_session_members_native ON global_session_members(native_session_id,global_session_id);

CREATE INDEX native_sessions_family ON native_sessions(tool_family,id);
