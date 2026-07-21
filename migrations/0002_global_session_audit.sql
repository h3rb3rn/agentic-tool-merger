CREATE TABLE membership_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    global_session_id TEXT NOT NULL REFERENCES global_sessions(id),
    native_session_id TEXT NOT NULL REFERENCES native_sessions(id),
    action TEXT NOT NULL CHECK (action IN ('link', 'unlink', 'reject', 'reverse_rejection')),
    actor TEXT NOT NULL,
    reason TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX membership_audit_session
    ON membership_audit_log(global_session_id, native_session_id, id);

CREATE UNIQUE INDEX user_overrides_unique_target
    ON user_overrides(override_type, target_id);
