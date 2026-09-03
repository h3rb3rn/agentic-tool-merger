//! Read-only snapshot adapters for JSON/JSONL agent session stores.

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

/// Supported read-only external session formats.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalTool {
    /// Claude Code JSONL transcripts.
    ClaudeCode,
    /// Continue JSON session snapshots.
    Continue,
    /// Antigravity CLI prompt history.
    Agy,
}

impl ExternalTool {
    fn family(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Continue => "continue",
            Self::Agy => "agy",
        }
    }

    fn surface(self) -> &'static str {
        match self {
            Self::ClaudeCode | Self::Agy => "cli",
            Self::Continue => "ide",
        }
    }
}

/// One external source discovered below an explicitly configured tool home.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalSource {
    /// Tool-specific format.
    pub tool: ExternalTool,
    /// Collector-readable source path.
    pub path: PathBuf,
    /// Host-facing provenance label.
    pub original_path: PathBuf,
}

/// Result of an external snapshot scan.
#[derive(Clone, Debug)]
pub struct ExternalImport {
    /// Whether a changed snapshot committed.
    pub changed: bool,
    /// Newly interpreted events, including idempotent replays.
    pub events: Vec<CanonicalEvent>,
}

/// Discovers Claude Code transcript files without following unrelated data.
#[must_use]
pub fn discover_claude(home: &Path, original_home: &Path) -> Vec<ExternalSource> {
    let root = home.join("projects");
    discover_files(&root, "jsonl")
        .into_iter()
        .map(|path| ExternalSource {
            tool: ExternalTool::ClaudeCode,
            original_path: original_home.join(path.strip_prefix(home).unwrap_or(&path)),
            path,
        })
        .collect()
}

/// Discovers Continue session snapshots and excludes its aggregate index.
#[must_use]
pub fn discover_continue(home: &Path, original_home: &Path) -> Vec<ExternalSource> {
    let root = home.join("sessions");
    discover_files(&root, "json")
        .into_iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) != Some("sessions.json"))
        .map(|path| ExternalSource {
            tool: ExternalTool::Continue,
            original_path: original_home.join(path.strip_prefix(home).unwrap_or(&path)),
            path,
        })
        .collect()
}

/// Discovers the Antigravity CLI conversation-aware prompt history.
#[must_use]
pub fn discover_agy(home: &Path, original_home: &Path) -> Vec<ExternalSource> {
    let path = home.join("history.jsonl");
    path.is_file()
        .then(|| ExternalSource {
            tool: ExternalTool::Agy,
            original_path: original_home.join("history.jsonl"),
            path,
        })
        .into_iter()
        .collect()
}

/// Stable source identity used to key the ingestion cursor, before any
/// server-side collector scoping.
#[must_use]
pub fn external_source_id(source: &ExternalSource) -> String {
    format!("{}:{}", source.tool.family(), source.path.to_string_lossy())
}

/// Imports a changed snapshot atomically while preserving immutable raw bytes.
///
/// Unchanged metadata fingerprints avoid reparsing. Changed snapshots are
/// fully interpreted so append-only JSONL and rewritten JSON arrays share one
/// idempotent storage boundary.
///
/// # Errors
///
/// Returns [`ExternalError`] when the source cannot be read, parsed, or stored
/// atomically.
pub async fn ingest_external(
    storage: &Storage,
    source: &ExternalSource,
    imported_at: &str,
) -> Result<ExternalImport, ExternalError> {
    let source_id = external_source_id(source);
    let prior = storage.get_cursor(&source_id).await?;
    let Some(batch) = prepare_external(source, imported_at, prior.as_ref()).await? else {
        return Ok(ExternalImport {
            changed: false,
            events: Vec::new(),
        });
    };
    let events = batch.events.clone();
    storage.commit_batch(&batch, None).await?;
    Ok(ExternalImport {
        changed: true,
        events,
    })
}

/// Prepares a changed external snapshot for atomic commit, or `None` when
/// `prior_cursor`'s fingerprint already matches the current file and
/// nothing changed.
///
/// Performs no storage access, so it is reusable by both local ingestion
/// (which commits the result directly) and a network collector (which POSTs
/// it to the daemon's ingestion API instead).
///
/// # Errors
///
/// Returns [`ExternalError`] when the source cannot be read or parsed.
pub async fn prepare_external(
    source: &ExternalSource,
    imported_at: &str,
    prior_cursor: Option<&IngestionCursor>,
) -> Result<Option<IngestionBatch>, ExternalError> {
    let metadata = tokio::fs::metadata(&source.path).await?;
    let source_id = external_source_id(source);
    let fingerprint = metadata_fingerprint(&source.path, &metadata);
    if prior_cursor.is_some_and(|cursor| cursor.source_generation == fingerprint) {
        return Ok(None);
    }
    let bytes = tokio::fs::read(&source.path).await?;
    let stable_generation = stable_generation(&source.path);
    let fallback_timestamp = metadata
        .modified()
        .map(chrono::DateTime::<chrono::Utc>::from)
        .map_or_else(
            |_| imported_at.to_owned(),
            |timestamp| timestamp.to_rfc3339(),
        );
    let parsed = match source.tool {
        ExternalTool::ClaudeCode => {
            parse_claude(&bytes, source, &stable_generation, &fallback_timestamp)?
        }
        ExternalTool::Continue => {
            parse_continue(&bytes, source, &stable_generation, &fallback_timestamp)?
        }
        ExternalTool::Agy => parse_agy(&bytes, source, &stable_generation, &fallback_timestamp)?,
    };
    let events = parsed
        .iter()
        .map(|record| record.event.clone())
        .collect::<Vec<_>>();
    let raw_objects = parsed
        .into_iter()
        .map(|record| RawObject {
            id: raw_id(&stable_generation, record.offset, &record.raw),
            bytes: record.raw,
            source_path: source.path.to_string_lossy().into_owned(),
            original_path: Some(source.original_path.to_string_lossy().into_owned()),
            source_offset: record.offset,
            source_size: metadata.len(),
            source_modified_at: Some(fallback_timestamp.clone()),
            source_permissions: permissions(&metadata),
            source_generation: stable_generation.clone(),
            parser_version: env!("CARGO_PKG_VERSION").to_owned(),
            imported_at: imported_at.to_owned(),
        })
        .collect();
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

#[derive(Clone, Debug)]
struct ParsedRecord {
    offset: u64,
    raw: Vec<u8>,
    event: CanonicalEvent,
}

fn parse_claude(
    bytes: &[u8],
    source: &ExternalSource,
    generation: &str,
    fallback_timestamp: &str,
) -> Result<Vec<ParsedRecord>, ExternalError> {
    let mut records = Vec::new();
    let mut offset = 0_u64;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let start = offset;
        offset = offset.saturating_add(u64::try_from(line.len()).unwrap_or(u64::MAX));
        let trimmed = line.strip_suffix(b"\n").unwrap_or(line);
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(trimmed) else {
            continue;
        };
        let native_type = value.get("type").and_then(serde_json::Value::as_str);
        let kind = match native_type {
            Some("user") => EventKind::UserMessage,
            Some("assistant") => EventKind::AssistantMessage,
            _ => continue,
        };
        let native_session_id = value
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .or_else(|| source.path.file_stem().and_then(|stem| stem.to_str()))
            .unwrap_or("claude-session");
        let session_id = format!("claude:{native_session_id}");
        let timestamp = value
            .get("timestamp")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(fallback_timestamp);
        let text = message_text(
            value
                .get("message")
                .and_then(|message| message.get("content")),
        );
        let mut payload = BTreeMap::new();
        payload.insert("text".to_owned(), serde_json::Value::String(text));
        let workspace = value
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .map(|cwd| WorkspaceContext {
                cwd: Some(cwd.to_owned()),
                repository_id: None,
                branch: value
                    .get("gitBranch")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                head: None,
            });
        records.push(ParsedRecord {
            offset: start,
            raw: line.to_vec(),
            event: event(
                source,
                &session_id,
                u64::try_from(records.len()).unwrap_or(u64::MAX),
                timestamp,
                kind,
                workspace,
                payload,
                generation,
                start,
            )?,
        });
    }
    Ok(records)
}

fn parse_continue(
    bytes: &[u8],
    source: &ExternalSource,
    generation: &str,
    fallback_timestamp: &str,
) -> Result<Vec<ParsedRecord>, ExternalError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)?;
    let native_session_id = value
        .get("sessionId")
        .and_then(serde_json::Value::as_str)
        .or_else(|| source.path.file_stem().and_then(|stem| stem.to_str()))
        .unwrap_or("continue-session");
    let session_id = format!("continue:{native_session_id}");
    let workspace = value
        .get("workspaceDirectory")
        .and_then(serde_json::Value::as_str)
        .map(|cwd| WorkspaceContext {
            cwd: Some(cwd.to_owned()),
            repository_id: None,
            branch: None,
            head: None,
        });
    let history = value
        .get("history")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    history
        .into_iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let message = item.get("message")?;
            let role = message.get("role")?.as_str()?;
            let kind = match role {
                "user" => EventKind::UserMessage,
                "assistant" => EventKind::AssistantMessage,
                "system" => EventKind::SystemMessage,
                _ => return None,
            };
            let mut payload = BTreeMap::new();
            payload.insert(
                "text".to_owned(),
                serde_json::Value::String(message_text(message.get("content"))),
            );
            let offset = u64::try_from(index).unwrap_or(u64::MAX);
            let raw = serde_json::to_vec(&item).ok()?;
            Some(
                event(
                    source,
                    &session_id,
                    offset,
                    fallback_timestamp,
                    kind,
                    workspace.clone(),
                    payload,
                    generation,
                    offset,
                )
                .map(|event| ParsedRecord { offset, raw, event }),
            )
        })
        .collect::<Result<Vec<_>, _>>()
}

fn parse_agy(
    bytes: &[u8],
    source: &ExternalSource,
    generation: &str,
    fallback_timestamp: &str,
) -> Result<Vec<ParsedRecord>, ExternalError> {
    let mut records = Vec::new();
    let mut offset = 0_u64;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let start = offset;
        offset = offset.saturating_add(u64::try_from(line.len()).unwrap_or(u64::MAX));
        let trimmed = line.strip_suffix(b"\n").unwrap_or(line);
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(trimmed) else {
            continue;
        };
        let Some(conversation_id) = value
            .get("conversationId")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let text = value
            .get("display")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if text.is_empty() {
            continue;
        }
        let timestamp = value
            .get("timestamp")
            .and_then(agy_timestamp)
            .unwrap_or_else(|| fallback_timestamp.to_owned());
        let workspace = value
            .get("workspace")
            .and_then(serde_json::Value::as_str)
            .filter(|workspace| !workspace.is_empty())
            .map(|cwd| WorkspaceContext {
                cwd: Some(cwd.to_owned()),
                repository_id: None,
                branch: None,
                head: None,
            });
        let payload = BTreeMap::from([(
            "text".to_owned(),
            serde_json::Value::String(text.to_owned()),
        )]);
        records.push(ParsedRecord {
            offset: start,
            raw: line.to_vec(),
            event: event(
                source,
                &format!("agy:{conversation_id}"),
                u64::try_from(records.len()).unwrap_or(u64::MAX),
                &timestamp,
                EventKind::UserMessage,
                workspace,
                payload,
                generation,
                start,
            )?,
        });
    }
    Ok(records)
}

fn agy_timestamp(value: &serde_json::Value) -> Option<String> {
    if let Some(timestamp) = value.as_str() {
        return Some(timestamp.to_owned());
    }
    let milliseconds = value.as_i64()?;
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(milliseconds)
        .map(|timestamp| timestamp.to_rfc3339())
}

#[allow(clippy::too_many_arguments)]
fn event(
    source: &ExternalSource,
    session_id: &str,
    sequence: u64,
    timestamp: &str,
    kind: EventKind,
    workspace: Option<WorkspaceContext>,
    payload: BTreeMap<String, serde_json::Value>,
    generation: &str,
    offset: u64,
) -> Result<CanonicalEvent, ExternalError> {
    Ok(CanonicalEvent::from_draft(EventDraft {
        tool: ToolIdentity {
            family: source.tool.family().to_owned(),
            surface: source.tool.surface().to_owned(),
            profile: "default".to_owned(),
        },
        native_session_id: session_id.to_owned(),
        sequence,
        timestamp: EventTimestamp::parse(timestamp.to_owned())?,
        timestamp_precision: TimestampPrecision::Unknown,
        kind,
        workspace,
        payload,
        provenance: EventProvenance {
            source_path: source.path.to_string_lossy().into_owned(),
            original_path: Some(source.original_path.to_string_lossy().into_owned()),
            source_offset: offset,
            source_generation: generation.to_owned(),
            adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
            ingestion_sequence: sequence,
            ordering_confidence: Some(0.8),
        },
    })?)
}

fn message_text(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn discover_files(root: &Path, extension: &str) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn stable_generation(path: &Path) -> String {
    digest(path.to_string_lossy().as_bytes())
}

fn metadata_fingerprint(path: &Path, metadata: &std::fs::Metadata) -> String {
    let modified = metadata.modified().ok().and_then(|value| {
        value
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_nanos())
    });
    digest(format!("{}:{}:{modified:?}", path.to_string_lossy(), metadata.len()).as_bytes())
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

#[cfg(unix)]
#[allow(
    clippy::unnecessary_wraps,
    reason = "the cross-platform call site stores nullable permissions"
)]
fn permissions(metadata: &std::fs::Metadata) -> Option<u32> {
    Some(unix_permissions(metadata))
}

#[cfg(unix)]
fn unix_permissions(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn permissions(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

/// External adapter failure.
#[derive(Debug)]
pub enum ExternalError {
    /// Source read or metadata failure.
    Io(std::io::Error),
    /// Storage transaction failure.
    Storage(StorageError),
    /// JSON snapshot failure.
    Json(serde_json::Error),
    /// Canonical event validation failure.
    Event(sessionmesh_core::event::EventError),
}

impl std::fmt::Display for ExternalError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "source read failed: {error}"),
            Self::Storage(error) => write!(formatter, "storage failed: {error}"),
            Self::Json(error) => write!(formatter, "snapshot JSON failed: {error}"),
            Self::Event(error) => write!(formatter, "canonical event failed: {error}"),
        }
    }
}

impl std::error::Error for ExternalError {}

impl From<std::io::Error> for ExternalError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<StorageError> for ExternalError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}
impl From<serde_json::Error> for ExternalError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
impl From<sessionmesh_core::event::EventError> for ExternalError {
    fn from(error: sessionmesh_core::event::EventError) -> Self {
        Self::Event(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn imports_claude_and_continue_idempotently_with_workspace_context() {
        let directory = tempfile::tempdir().unwrap();
        let claude = directory.path().join("claude/projects/repo/session.jsonl");
        let continue_path = directory.path().join("continue/sessions/session.json");
        std::fs::create_dir_all(claude.parent().unwrap()).unwrap();
        std::fs::create_dir_all(continue_path.parent().unwrap()).unwrap();
        std::fs::write(&claude, concat!(
            "{\"type\":\"user\",\"sessionId\":\"claude-1\",\"timestamp\":\"2026-07-21T10:00:00Z\",\"cwd\":\"/repo\",\"message\":{\"content\":\"Build it\"}}\n",
            "{\"type\":\"assistant\",\"sessionId\":\"claude-1\",\"timestamp\":\"2026-07-21T10:01:00Z\",\"cwd\":\"/repo\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Done\"}]}}\n"
        )).unwrap();
        std::fs::write(&continue_path, r#"{"sessionId":"continue-1","workspaceDirectory":"/repo","history":[{"message":{"role":"user","content":"Continue it"}},{"message":{"role":"assistant","content":"Working"}}]}"#).unwrap();
        let storage = Storage::open(
            directory.path().join("state.db"),
            directory.path().join("blobs"),
        )
        .await
        .unwrap();
        let claude_source =
            discover_claude(&directory.path().join("claude"), Path::new("~/.claude"))
                .pop()
                .unwrap();
        let continue_source =
            discover_continue(&directory.path().join("continue"), Path::new("~/.continue"))
                .pop()
                .unwrap();

        let first = ingest_external(&storage, &claude_source, "2026-07-21T11:00:00Z")
            .await
            .unwrap();
        let second = ingest_external(&storage, &claude_source, "2026-07-21T11:01:00Z")
            .await
            .unwrap();
        let third = ingest_external(&storage, &continue_source, "2026-07-21T11:02:00Z")
            .await
            .unwrap();

        assert_eq!(first.events.len(), 2);
        assert!(!second.changed);
        assert_eq!(third.events.len(), 2);
        assert!(first.events.iter().chain(&third.events).all(|event| {
            event
                .workspace
                .as_ref()
                .and_then(|workspace| workspace.cwd.as_deref())
                == Some("/repo")
        }));
        assert_eq!(storage.list_native_sessions().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn imports_agy_history_by_conversation_without_reading_oauth_state() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("antigravity-cli");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("history.jsonl"),
            concat!(
                "{\"conversationId\":\"agy-1\",\"display\":\"Design correlation\",\"timestamp\":1784678400000,\"workspace\":\"/workspace/repo\"}\n",
                "{\"conversationId\":\"agy-1\",\"display\":\"Implement evidence\",\"timestamp\":1784678460000,\"workspace\":\"/workspace/repo\"}\n"
            ),
        )
        .unwrap();
        std::fs::write(home.join("antigravity-oauth-token"), "must-not-be-read").unwrap();
        let storage = Storage::open(
            directory.path().join("state.db"),
            directory.path().join("blobs"),
        )
        .await
        .unwrap();
        let source = discover_agy(&home, Path::new("~/.gemini/antigravity-cli"))
            .pop()
            .unwrap();

        let imported = ingest_external(&storage, &source, "2026-07-22T00:00:00Z")
            .await
            .unwrap();

        assert_eq!(imported.events.len(), 2);
        assert!(
            imported
                .events
                .iter()
                .all(|event| event.native_session_id == "agy:agy-1")
        );
        assert_eq!(imported.events[0].tool.family, "agy");
        assert_eq!(
            imported.events[0]
                .workspace
                .as_ref()
                .unwrap()
                .cwd
                .as_deref(),
            Some("/workspace/repo")
        );
    }
}
