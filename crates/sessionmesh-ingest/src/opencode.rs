//! Read-only `OpenCode` `SQLite` adapter restricted to session content tables.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sessionmesh_core::event::{
    CanonicalEvent, EventDraft, EventKind, EventProvenance, EventTimestamp, TimestampPrecision,
    ToolIdentity, WorkspaceContext,
};
use sessionmesh_storage::{
    CursorRepository, IngestionBatch, IngestionCursor, RawObject, Storage, StorageError,
};
use sha2::{Digest, Sha256};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Connection, Row, SqliteConnection};

const SQLITE_INTEGER_MAX: u64 = 0x7fff_ffff_ffff_ffff;

/// Collector-readable `OpenCode` database and its host provenance label.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenCodeSource {
    /// Database path visible to `SessionMesh`.
    pub path: PathBuf,
    /// Original host path shown in provenance.
    pub original_path: PathBuf,
}

/// Result of one `OpenCode` database scan.
#[derive(Clone, Debug)]
pub struct OpenCodeImport {
    /// Whether a changed database snapshot committed.
    pub changed: bool,
    /// Events interpreted from explicitly allowed session tables.
    pub events: Vec<CanonicalEvent>,
}

/// Discovers the documented `OpenCode` local database location.
#[must_use]
pub fn discover_opencode(home: &Path, original_home: &Path) -> Option<OpenCodeSource> {
    let path = home.join(".local/share/opencode/opencode.db");
    path.is_file().then(|| OpenCodeSource {
        path,
        original_path: original_home.join(".local/share/opencode/opencode.db"),
    })
}

/// Stable source identity used to key the ingestion cursor, before any
/// server-side collector scoping.
#[must_use]
pub fn opencode_source_id(source: &OpenCodeSource) -> String {
    format!("opencode:{}", source.path.to_string_lossy())
}

/// Imports `OpenCode` messages without selecting account, credential, or share tables.
///
/// # Errors
///
/// Returns [`OpenCodeError`] when the read-only database cannot be queried or
/// the selected session rows cannot be normalized and stored atomically.
pub async fn ingest_opencode(
    storage: &Storage,
    source: &OpenCodeSource,
    imported_at: &str,
) -> Result<OpenCodeImport, OpenCodeError> {
    let source_id = opencode_source_id(source);
    let prior = storage.get_cursor(&source_id).await?;
    let Some(batch) = prepare_opencode(source, imported_at, prior.as_ref()).await? else {
        return Ok(OpenCodeImport {
            changed: false,
            events: Vec::new(),
        });
    };
    let events = batch.events.clone();
    storage.commit_batch(&batch, None).await?;
    Ok(OpenCodeImport {
        changed: true,
        events,
    })
}

/// Prepares a changed `OpenCode` database snapshot for atomic commit, or
/// `None` when `prior_cursor`'s fingerprint already matches the current
/// database (and its WAL file) and nothing changed.
///
/// Performs no storage access, so it is reusable by both local ingestion
/// (which commits the result directly) and a network collector (which POSTs
/// it to the daemon's ingestion API instead).
///
/// # Errors
///
/// Returns [`OpenCodeError`] when the read-only database cannot be queried
/// or the selected session rows cannot be normalized.
#[allow(
    clippy::too_many_lines,
    reason = "the scan keeps source selection and normalization in one auditable boundary"
)]
pub async fn prepare_opencode(
    source: &OpenCodeSource,
    imported_at: &str,
    prior_cursor: Option<&IngestionCursor>,
) -> Result<Option<IngestionBatch>, OpenCodeError> {
    let metadata = tokio::fs::metadata(&source.path).await?;
    let fingerprint = database_fingerprint(&source.path, &metadata);
    let source_id = opencode_source_id(source);
    if prior_cursor.is_some_and(|cursor| cursor.source_generation == fingerprint) {
        return Ok(None);
    }

    let options = SqliteConnectOptions::new()
        .filename(&source.path)
        .read_only(true)
        // The agent-home bind is intentionally read-only. Immutable mode
        // prevents SQLite from attempting WAL/SHM lock writes on that mount.
        .immutable(true)
        .create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    let rows = sqlx::query(
        "SELECT p.id AS part_id, p.session_id, p.time_created,
                s.directory, s.title AS thread_title,
                m.data AS message_data, p.data AS part_data
         FROM part p
         JOIN message m ON m.id = p.message_id AND m.session_id = p.session_id
         JOIN session s ON s.id = p.session_id
         WHERE json_extract(p.data, '$.type') = 'text'
           AND json_type(p.data, '$.text') = 'text'
         ORDER BY p.session_id, p.time_created, p.id",
    )
    .fetch_all(&mut connection)
    .await?;
    connection.close().await?;

    let generation = stable_generation(&source.path);
    let mut sequences = BTreeMap::<String, u64>::new();
    let mut events = Vec::with_capacity(rows.len());
    let mut raw_objects = Vec::with_capacity(rows.len());
    for row in rows {
        let part_id: String = row.try_get("part_id")?;
        let native_id: String = row.try_get("session_id")?;
        let created: i64 = row.try_get("time_created")?;
        let directory: String = row.try_get("directory")?;
        let thread_title: String = row.try_get("thread_title")?;
        let message_data: String = row.try_get("message_data")?;
        let part_data: String = row.try_get("part_data")?;
        let message: serde_json::Value = serde_json::from_str(&message_data)?;
        let part: serde_json::Value = serde_json::from_str(&part_data)?;
        let role = message
            .get("role")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("assistant");
        let kind = match role {
            "user" => EventKind::UserMessage,
            "system" => EventKind::SystemMessage,
            _ => EventKind::AssistantMessage,
        };
        let text = part
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if text.is_empty() {
            continue;
        }
        let session_id = format!("opencode:{native_id}");
        let sequence = sequences.entry(session_id.clone()).or_default();
        let timestamp = timestamp(created, imported_at);
        let raw = serde_json::to_vec(&serde_json::json!({
            "part_id": part_id,
            "session_id": native_id,
            "time_created": created,
            "directory": directory,
            "thread_title": thread_title,
            "message_data": message_data,
            "part_data": part_data
        }))?;
        let offset = stable_offset(&part_id);
        let event = CanonicalEvent::from_draft(EventDraft {
            tool: ToolIdentity {
                family: "opencode".to_owned(),
                surface: "cli".to_owned(),
                profile: "default".to_owned(),
            },
            native_session_id: session_id,
            sequence: *sequence,
            timestamp: EventTimestamp::parse(timestamp.clone())?,
            timestamp_precision: TimestampPrecision::Millisecond,
            kind,
            workspace: Some(WorkspaceContext {
                cwd: Some(directory),
                repository_id: None,
                branch: None,
                head: None,
            }),
            payload: BTreeMap::from([
                (
                    "text".to_owned(),
                    serde_json::Value::String(text.to_owned()),
                ),
                (
                    "thread_title".to_owned(),
                    serde_json::Value::String(thread_title),
                ),
            ]),
            provenance: EventProvenance {
                source_path: source.path.to_string_lossy().into_owned(),
                original_path: Some(source.original_path.to_string_lossy().into_owned()),
                source_offset: offset,
                source_generation: generation.clone(),
                adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
                ingestion_sequence: *sequence,
                ordering_confidence: Some(0.95),
            },
        })?;
        raw_objects.push(RawObject {
            id: raw_id(&generation, offset, &raw),
            bytes: raw,
            source_path: source.path.to_string_lossy().into_owned(),
            original_path: Some(source.original_path.to_string_lossy().into_owned()),
            source_offset: offset,
            source_size: metadata.len(),
            source_modified_at: Some(timestamp),
            source_permissions: None,
            source_generation: generation.clone(),
            parser_version: env!("CARGO_PKG_VERSION").to_owned(),
            imported_at: imported_at.to_owned(),
        });
        events.push(event);
        *sequence = sequence.saturating_add(1);
    }
    let next_sequence = u64::try_from(events.len()).unwrap_or(u64::MAX);
    Ok(Some(IngestionBatch {
        raw_objects,
        events,
        cursor: IngestionCursor {
            source_id,
            source_generation: fingerprint,
            byte_offset: metadata.len(),
            next_sequence,
            partial_line: Vec::new(),
            updated_at: imported_at.to_owned(),
        },
    }))
}

fn timestamp(value: i64, fallback: &str) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(value)
        .map_or_else(|| fallback.to_owned(), |timestamp| timestamp.to_rfc3339())
}

fn database_fingerprint(path: &Path, metadata: &std::fs::Metadata) -> String {
    let wal = path.with_extension("db-wal");
    let wal_metadata = std::fs::metadata(wal).ok();
    digest(
        format!(
            "{}:{}:{:?}",
            path.to_string_lossy(),
            metadata_stamp(metadata),
            wal_metadata.as_ref().map(metadata_stamp)
        )
        .as_bytes(),
    )
}

fn metadata_stamp(metadata: &std::fs::Metadata) -> String {
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    format!("{}:{modified_nanos}", metadata.len())
}

fn stable_generation(path: &Path) -> String {
    digest(path.to_string_lossy().as_bytes())
}

fn stable_offset(id: &str) -> u64 {
    let digest = Sha256::digest(id.as_bytes());
    u64::from_le_bytes(
        digest[..8]
            .try_into()
            .expect("SHA-256 prefix has eight bytes"),
    ) & SQLITE_INTEGER_MAX
}

fn raw_id(generation: &str, offset: u64, bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(generation.as_bytes());
    hasher.update(offset.to_le_bytes());
    hasher.update(bytes);
    format_digest("raw_", hasher.finalize())
}

fn digest(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format_digest("generation_", hasher.finalize())
}

fn format_digest(prefix: &str, value: impl AsRef<[u8]>) -> String {
    let mut output = prefix.to_owned();
    for byte in value.as_ref() {
        write!(&mut output, "{byte:02x}").expect("writing to a String cannot fail");
    }
    output
}

/// `OpenCode` adapter failure.
#[derive(Debug)]
pub enum OpenCodeError {
    /// Filesystem metadata failure.
    Io(std::io::Error),
    /// External `SQLite` query failure.
    Sql(sqlx::Error),
    /// Selected JSON column failure.
    Json(serde_json::Error),
    /// Canonical event validation failure.
    Event(sessionmesh_core::event::EventError),
    /// `SessionMesh` persistence failure.
    Storage(StorageError),
}

impl std::fmt::Display for OpenCodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "OpenCode import failed: {self:?}")
    }
}

impl std::error::Error for OpenCodeError {}

macro_rules! error_from {
    ($source:ty, $variant:ident) => {
        impl From<$source> for OpenCodeError {
            fn from(error: $source) -> Self {
                Self::$variant(error)
            }
        }
    };
}

error_from!(std::io::Error, Io);
error_from!(sqlx::Error, Sql);
error_from!(serde_json::Error, Json);
error_from!(sessionmesh_core::event::EventError, Event);
error_from!(StorageError, Storage);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_row_offsets_always_fit_sqlite_integer() {
        for id in [
            "part-1",
            "part-high-bit",
            "unicode-ü",
            "z".repeat(512).as_str(),
        ] {
            assert!(stable_offset(id) <= SQLITE_INTEGER_MAX);
        }
    }

    #[tokio::test]
    async fn imports_only_session_content_and_never_requires_credential_tables() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("opencode.db");
        let mut connection = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&database)
                .create_if_missing(true),
        )
        .await
        .unwrap();
        for statement in [
            "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT NOT NULL, title TEXT NOT NULL)",
            "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, data TEXT NOT NULL)",
            "CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, data TEXT NOT NULL)",
            "CREATE TABLE credential (unexpected_secret_shape BLOB)",
            "INSERT INTO session VALUES ('s1', '/workspace/repo', 'Implement OpenCode import')",
            "INSERT INTO message VALUES ('m1', 's1', '{\"role\":\"user\"}')",
            "INSERT INTO part VALUES ('p1', 'm1', 's1', 1784678400000, '{\"type\":\"text\",\"text\":\"Build shared context\"}')",
        ] {
            sqlx::query(statement)
                .execute(&mut connection)
                .await
                .unwrap();
        }
        connection.close().await.unwrap();
        let storage = Storage::open(
            directory.path().join("sessionmesh.db"),
            directory.path().join("blobs"),
        )
        .await
        .unwrap();
        let source = OpenCodeSource {
            path: database,
            original_path: PathBuf::from("~/.local/share/opencode/opencode.db"),
        };

        let first = ingest_opencode(&storage, &source, "2026-07-22T00:00:00Z")
            .await
            .unwrap();
        let second = ingest_opencode(&storage, &source, "2026-07-22T00:01:00Z")
            .await
            .unwrap();

        assert!(first.changed);
        assert!(!second.changed);
        assert_eq!(first.events.len(), 1);
        assert_eq!(first.events[0].native_session_id, "opencode:s1");
        assert_eq!(first.events[0].tool.family, "opencode");
        assert_eq!(first.events[0].payload["text"], "Build shared context");
        assert_eq!(
            first.events[0].workspace.as_ref().unwrap().cwd.as_deref(),
            Some("/workspace/repo")
        );
    }
}
