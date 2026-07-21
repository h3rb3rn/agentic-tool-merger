//! Cursor-driven Codex file ingestion and deterministic watcher primitives.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter, Write};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sessionmesh_core::event::CanonicalEvent;
use sessionmesh_storage::{
    CursorRepository, IngestionBatch, IngestionCursor, RawObject, Storage, StorageError,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader};

use super::parser::{CodexParseContext, ParseIssue, parse_rollout};

/// Immutable metadata needed to prepare one scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IncrementalInput {
    /// Stable identity that survives a path rename.
    pub source_id: String,
    /// Current runtime path.
    pub source_path: PathBuf,
    /// Host-facing provenance path.
    pub original_path: Option<String>,
    /// Parser version.
    pub adapter_version: String,
    /// Fallback session identity.
    pub fallback_session_id: String,
    /// Explicit timestamp supplied by the daemon clock.
    pub imported_at: String,
}

/// Why a prior cursor could not continue.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResetReason {
    /// No cursor existed.
    Initial,
    /// File identity changed.
    Replaced,
    /// File size moved before the committed boundary.
    Truncated,
    /// Retained partial bytes no longer match.
    PartialMismatch,
}

/// Prepared atomic storage unit and isolated parser diagnostics.
#[derive(Clone, Debug)]
pub struct PreparedIncremental {
    /// Raw records, events, and next cursor.
    pub batch: IngestionBatch,
    /// Record-level issues that did not block safe progress.
    pub issues: Vec<ParseIssue>,
    /// Reset reason when scanning restarted from zero.
    pub reset: Option<ResetReason>,
}

/// Prepares only newly available complete records.
///
/// # Errors
///
/// Returns an I/O error if metadata or read-only access fails.
pub async fn prepare_incremental(
    input: &IncrementalInput,
    prior: Option<&IngestionCursor>,
) -> Result<PreparedIncremental, std::io::Error> {
    let metadata = tokio::fs::metadata(&input.source_path).await?;
    let generation = source_generation(&input.source_path, &metadata).await?;
    let size = metadata.len();
    let (mut start, mut next_sequence, mut reset) = resume_position(prior, &generation, size);
    let mut bytes = read_after(&input.source_path, start).await?;

    if let Some(cursor) = prior
        && reset.is_none()
        && !cursor.partial_line.is_empty()
        && !bytes.starts_with(&cursor.partial_line)
    {
        start = 0;
        next_sequence = 0;
        reset = Some(ResetReason::PartialMismatch);
        bytes = read_after(&input.source_path, 0).await?;
    }

    let complete_length = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    let complete = &bytes[..complete_length];
    let partial_line = bytes[complete_length..].to_vec();
    let parser_context = CodexParseContext {
        source_path: input.source_path.to_string_lossy().into_owned(),
        original_path: input.original_path.clone(),
        source_generation: generation.clone(),
        base_offset: start,
        first_native_sequence: next_sequence,
        adapter_version: input.adapter_version.clone(),
        fallback_session_id: input.fallback_session_id.clone(),
        first_ingestion_sequence: next_sequence,
    };
    let parsed = parse_rollout(complete, &parser_context);
    let completed_records = u64::try_from(parsed.len()).unwrap_or(u64::MAX);
    let confirmed_bytes = u64::try_from(complete_length).unwrap_or(u64::MAX);
    let modified = modified_label(&metadata);
    let permissions = unix_permissions(&metadata);
    let mut raw_objects = Vec::with_capacity(parsed.len());
    let mut events = Vec::new();
    let mut issues = Vec::new();
    for record in parsed {
        raw_objects.push(RawObject {
            id: raw_identity(&generation, record.raw.source_offset, &record.raw.bytes),
            bytes: record.raw.bytes,
            source_path: parser_context.source_path.clone(),
            original_path: parser_context.original_path.clone(),
            source_offset: record.raw.source_offset,
            source_size: size,
            source_modified_at: modified.clone(),
            source_permissions: permissions,
            source_generation: generation.clone(),
            parser_version: input.adapter_version.clone(),
            imported_at: input.imported_at.clone(),
        });
        events.extend(record.event);
        issues.extend(record.issue);
    }
    Ok(PreparedIncremental {
        batch: IngestionBatch {
            raw_objects,
            events,
            cursor: IngestionCursor {
                source_id: input.source_id.clone(),
                source_generation: generation,
                byte_offset: start.saturating_add(confirmed_bytes),
                next_sequence: next_sequence.saturating_add(completed_records),
                partial_line,
                updated_at: input.imported_at.clone(),
            },
        },
        issues,
        reset,
    })
}

/// Prepares and atomically commits one scan.
///
/// # Errors
///
/// Returns filesystem or storage failures. Preparation never changes a
/// committed cursor; storage commits it with the accepted event set.
pub async fn ingest_file(
    storage: &Storage,
    input: &IncrementalInput,
) -> Result<PreparedIncremental, IngestError> {
    let prior = storage.get_cursor(&input.source_id).await?;
    let prepared = prepare_incremental(input, prior.as_ref()).await?;
    storage.commit_batch(&prepared.batch).await?;
    Ok(prepared)
}

/// Incremental ingestion failure.
#[derive(Debug)]
pub enum IngestError {
    /// Native source read failed.
    Io(std::io::Error),
    /// Atomic persistence failed.
    Storage(StorageError),
}

impl Display for IngestError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "source read failed: {error}"),
            Self::Storage(error) => write!(formatter, "storage commit failed: {error}"),
        }
    }
}

impl std::error::Error for IngestError {}

impl From<std::io::Error> for IngestError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<StorageError> for IngestError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

/// Deterministic cross-source display key from ADR-009.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct OrderingKey {
    /// RFC 3339 instant normalized to nanoseconds.
    pub effective_timestamp_nanos: i64,
    /// Source-local native sequence.
    pub native_sequence: u64,
    /// Stable source generation.
    pub source_generation: String,
    /// Source byte offset.
    pub source_byte_offset: u64,
    /// Durable ingestion order.
    pub ingestion_sequence: u64,
    /// Final deterministic tie break.
    pub event_id: String,
}

/// Builds the deterministic ordering key for a canonical event.
///
/// # Errors
///
/// Returns an error for an unrepresentable timestamp.
pub fn ordering_key(event: &CanonicalEvent) -> Result<OrderingKey, String> {
    let timestamp = chrono::DateTime::parse_from_rfc3339(event.timestamp.as_str())
        .map_err(|error| error.to_string())?;
    let effective_timestamp_nanos = timestamp
        .timestamp_nanos_opt()
        .ok_or_else(|| "timestamp is outside nanosecond ordering range".to_owned())?;
    Ok(OrderingKey {
        effective_timestamp_nanos,
        native_sequence: event.sequence,
        source_generation: event.provenance.source_generation.clone(),
        source_byte_offset: event.provenance.source_offset,
        ingestion_sequence: event.provenance.ingestion_sequence,
        event_id: event.event_id.as_str().to_owned(),
    })
}

/// Bounded retry classification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetryPolicy {
    /// Retries after the initial attempt.
    pub max_retries: u32,
    /// Initial delay.
    pub initial_delay: Duration,
    /// Maximum delay.
    pub maximum_delay: Duration,
}

/// Retry decision retained for diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryDecision {
    /// Retry after a bounded delay.
    RetryAfter(Duration),
    /// Failure is permanent or exhausted.
    Stop,
}

impl RetryPolicy {
    /// Classifies a filesystem failure without sleeping.
    #[must_use]
    pub fn decide(&self, error: &std::io::Error, retries: u32) -> RetryDecision {
        let transient = matches!(
            error.kind(),
            ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut
        );
        if !transient || retries >= self.max_retries {
            return RetryDecision::Stop;
        }
        let multiplier = 2_u32.saturating_pow(retries);
        RetryDecision::RetryAfter(
            self.initial_delay
                .saturating_mul(multiplier)
                .min(self.maximum_delay),
        )
    }
}

/// Event-storm coalescer driven by caller-supplied monotonic time.
#[derive(Clone, Debug)]
pub struct WatchDebouncer {
    debounce: Duration,
    pending: BTreeMap<PathBuf, Duration>,
}

impl WatchDebouncer {
    /// Creates a coalescer.
    #[must_use]
    pub fn new(debounce: Duration) -> Self {
        Self {
            debounce,
            pending: BTreeMap::new(),
        }
    }

    /// Records or refreshes one path notification.
    pub fn notify(&mut self, path: PathBuf, now: Duration) {
        self.pending.insert(path, now);
    }

    /// Drains stable paths in lexical order.
    #[must_use]
    pub fn drain_ready(&mut self, now: Duration) -> Vec<PathBuf> {
        let ready = self
            .pending
            .iter()
            .filter(|(_, observed)| now.saturating_sub(**observed) >= self.debounce)
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        for path in &ready {
            self.pending.remove(path);
        }
        ready
    }
}

fn resume_position(
    prior: Option<&IngestionCursor>,
    generation: &str,
    size: u64,
) -> (u64, u64, Option<ResetReason>) {
    match prior {
        None => (0, 0, Some(ResetReason::Initial)),
        Some(cursor) if cursor.source_generation != generation => {
            (0, 0, Some(ResetReason::Replaced))
        }
        Some(cursor) if size < cursor.byte_offset => (0, 0, Some(ResetReason::Truncated)),
        Some(cursor) => (cursor.byte_offset, cursor.next_sequence, None),
    }
}

async fn read_after(path: &Path, offset: u64) -> Result<Vec<u8>, std::io::Error> {
    let mut file = tokio::fs::File::open(path).await?;
    file.seek(std::io::SeekFrom::Start(offset)).await?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).await?;
    Ok(bytes)
}

async fn source_generation(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> Result<String, std::io::Error> {
    let mut reader = BufReader::new(tokio::fs::File::open(path).await?);
    let mut first_record = Vec::new();
    reader.read_until(b'\n', &mut first_record).await?;
    if first_record.last() != Some(&b'\n') {
        first_record.clear();
    }
    let mut hasher = Sha256::new();
    hasher.update(metadata_identity(metadata));
    hasher.update(first_record.len().to_be_bytes());
    hasher.update(&first_record);
    Ok(prefixed_digest(hasher.finalize()))
}

#[cfg(unix)]
fn metadata_identity(metadata: &std::fs::Metadata) -> Vec<u8> {
    use std::os::unix::fs::MetadataExt;

    [metadata.dev().to_be_bytes(), metadata.ino().to_be_bytes()].concat()
}

#[cfg(not(unix))]
fn metadata_identity(metadata: &std::fs::Metadata) -> Vec<u8> {
    metadata
        .created()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(Vec::new, |duration| {
            duration.as_nanos().to_be_bytes().to_vec()
        })
}

fn raw_identity(generation: &str, offset: u64, bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(generation.as_bytes());
    hasher.update(offset.to_be_bytes());
    hasher.update(bytes);
    prefixed_digest(hasher.finalize())
}

fn prefixed_digest(digest: impl AsRef<[u8]>) -> String {
    let mut value = String::with_capacity(71);
    value.push_str("sha256:");
    for byte in digest.as_ref() {
        write!(&mut value, "{byte:02x}").expect("writing to a String cannot fail");
    }
    value
}

fn modified_label(metadata: &std::fs::Metadata) -> Option<String> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| format!("unix:{}.{}", duration.as_secs(), duration.subsec_nanos()))
}

#[cfg(unix)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "the cross-platform storage model represents unavailable Unix modes as None"
)]
fn unix_permissions(metadata: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;

    Some(metadata.permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn unix_permissions(_metadata: &std::fs::Metadata) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::Write as IoWrite;

    use sessionmesh_storage::{CursorRepository, EventRepository};
    use tempfile::TempDir;

    use super::*;

    const SESSION: &[u8] = b"{\"timestamp\":\"2026-07-20T14:30:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"session-1\"}}\n";
    const MESSAGE: &[u8] = b"{\"timestamp\":\"2026-07-20T14:30:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"next\"}}\n";
    const LATE: &[u8] = b"{\"timestamp\":\"2026-07-20T14:29:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"late\"}}\n";

    async fn setup() -> (TempDir, Storage, IncrementalInput) {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let source = directory.path().join("rollout.jsonl");
        fs::write(&source, SESSION).expect("source fixture should write");
        let storage = Storage::open(
            directory.path().join("sessionmesh.db"),
            directory.path().join("blobs"),
        )
        .await
        .expect("storage should open");
        let input = IncrementalInput {
            source_id: "codex-source".to_owned(),
            source_path: source,
            original_path: Some("~/.codex/rollout.jsonl".to_owned()),
            adapter_version: "0.1.0".to_owned(),
            fallback_session_id: "fallback".to_owned(),
            imported_at: "2026-07-20T15:00:00Z".to_owned(),
        };
        (directory, storage, input)
    }

    fn append(path: &Path, bytes: &[u8]) {
        OpenOptions::new()
            .append(true)
            .open(path)
            .expect("source should open for fixture append")
            .write_all(bytes)
            .expect("fixture append should work");
    }

    #[tokio::test]
    async fn append_and_duplicate_scans_import_each_event_once() {
        let (_directory, storage, input) = setup().await;
        ingest_file(&storage, &input)
            .await
            .expect("initial scan should commit");
        append(&input.source_path, MESSAGE);
        let appended = ingest_file(&storage, &input)
            .await
            .expect("append scan should commit");
        let repeated = ingest_file(&storage, &input)
            .await
            .expect("repeat scan should commit no changes");

        assert_eq!(appended.batch.events.len(), 1);
        assert!(repeated.batch.events.is_empty());
        assert_eq!(storage.event_count().await.expect("count should work"), 2);
        assert_eq!(
            repeated.batch.cursor.byte_offset,
            u64::try_from(SESSION.len() + MESSAGE.len()).unwrap()
        );
    }

    #[tokio::test]
    async fn partial_line_survives_restart_without_early_parse() {
        let (_directory, storage, input) = setup().await;
        let split = MESSAGE.len() / 2;
        append(&input.source_path, &MESSAGE[..split]);
        let first = ingest_file(&storage, &input)
            .await
            .expect("partial scan should commit cursor");

        assert_eq!(first.batch.events.len(), 1);
        assert_eq!(first.batch.cursor.partial_line, MESSAGE[..split]);
        assert_eq!(
            first.batch.cursor.byte_offset,
            u64::try_from(SESSION.len()).unwrap()
        );

        append(&input.source_path, &MESSAGE[split..]);
        let second = ingest_file(&storage, &input)
            .await
            .expect("completed line should commit");

        assert_eq!(second.batch.events.len(), 1);
        assert!(second.batch.cursor.partial_line.is_empty());
        assert_eq!(second.batch.events[0].sequence, 1);
    }

    #[tokio::test]
    async fn crash_before_commit_replays_and_crash_after_commit_deduplicates() {
        let (_directory, storage, input) = setup().await;
        let prepared_before_crash = prepare_incremental(&input, None)
            .await
            .expect("preparation should work");
        assert!(
            storage
                .get_cursor(&input.source_id)
                .await
                .expect("cursor lookup should work")
                .is_none()
        );

        let replay = ingest_file(&storage, &input)
            .await
            .expect("replayed scan should commit");
        assert_eq!(
            prepared_before_crash.batch.events[0].event_id,
            replay.batch.events[0].event_id
        );
        let after_commit = ingest_file(&storage, &input)
            .await
            .expect("post-commit scan should be empty");
        assert!(after_commit.batch.events.is_empty());
    }

    #[tokio::test]
    async fn truncate_and_replacement_reset_without_data_loss() {
        let (_directory, storage, input) = setup().await;
        append(&input.source_path, MESSAGE);
        ingest_file(&storage, &input)
            .await
            .expect("initial content should commit");
        fs::write(&input.source_path, SESSION).expect("fixture should truncate");
        let truncated = ingest_file(&storage, &input)
            .await
            .expect("truncated source should recover");
        assert_eq!(truncated.reset, Some(ResetReason::Truncated));

        fs::remove_file(&input.source_path).expect("old generation should remove");
        fs::write(&input.source_path, LATE).expect("replacement should write");
        let replaced = ingest_file(&storage, &input)
            .await
            .expect("replacement should ingest");
        assert_eq!(replaced.reset, Some(ResetReason::Replaced));
        assert_eq!(storage.event_count().await.expect("count should work"), 3);
    }

    #[tokio::test]
    async fn rename_preserves_generation_and_continues_offset() {
        let (directory, storage, mut input) = setup().await;
        let first = ingest_file(&storage, &input)
            .await
            .expect("initial source should commit");
        let renamed = directory.path().join("renamed.jsonl");
        fs::rename(&input.source_path, &renamed).expect("source should rename");
        input.source_path = renamed;
        append(&input.source_path, MESSAGE);
        let second = ingest_file(&storage, &input)
            .await
            .expect("renamed source should continue");

        assert_eq!(
            first.batch.cursor.source_generation,
            second.batch.cursor.source_generation
        );
        assert_eq!(second.reset, None);
        assert_eq!(second.batch.events[0].sequence, 1);
    }

    #[test]
    fn event_storms_coalesce_and_retry_is_bounded() {
        let mut debouncer = WatchDebouncer::new(Duration::from_millis(750));
        let path = PathBuf::from("/sources/codex/rollout.jsonl");
        debouncer.notify(path.clone(), Duration::from_millis(0));
        debouncer.notify(path.clone(), Duration::from_millis(100));
        debouncer.notify(path.clone(), Duration::from_millis(200));
        assert!(debouncer.drain_ready(Duration::from_millis(900)).is_empty());
        assert_eq!(
            debouncer.drain_ready(Duration::from_millis(950)),
            vec![path]
        );

        let policy = RetryPolicy {
            max_retries: 2,
            initial_delay: Duration::from_millis(100),
            maximum_delay: Duration::from_millis(150),
        };
        let transient = std::io::Error::from(ErrorKind::WouldBlock);
        assert_eq!(
            policy.decide(&transient, 0),
            RetryDecision::RetryAfter(Duration::from_millis(100))
        );
        assert_eq!(
            policy.decide(&transient, 1),
            RetryDecision::RetryAfter(Duration::from_millis(150))
        );
        assert_eq!(policy.decide(&transient, 2), RetryDecision::Stop);
        assert_eq!(
            policy.decide(&std::io::Error::from(ErrorKind::PermissionDenied), 0),
            RetryDecision::Stop
        );
    }

    #[tokio::test]
    async fn ordering_is_deterministic_for_late_and_equal_timestamps() {
        let (_directory, _storage, input) = setup().await;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(SESSION);
        bytes.extend_from_slice(MESSAGE);
        bytes.extend_from_slice(
            b"{\"timestamp\":\"2026-07-20T14:30:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"same time\"}}\n",
        );
        fs::write(&input.source_path, bytes).expect("ordering fixture should write");
        let prepared = prepare_incremental(&input, None)
            .await
            .expect("ordering fixture should parse");
        let mut first = prepared
            .batch
            .events
            .iter()
            .map(ordering_key)
            .collect::<Result<Vec<_>, _>>()
            .expect("keys should build");
        let mut second = first.clone();
        first.sort();
        second.reverse();
        second.sort();

        assert_eq!(first, second);
        assert!(first[1] < first[2]);
    }
}
