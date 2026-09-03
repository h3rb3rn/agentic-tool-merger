//! Transactional `SQLite` storage and content-addressed blob persistence.

use std::{
    error::Error,
    fmt::{self, Display, Formatter, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use async_trait::async_trait;
use sessionmesh_core::event::CanonicalEvent;
use sha2::{Digest, Sha256};
use sqlx::{
    Row, Sqlite, SqlitePool, Transaction,
    migrate::MigrateError,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use tokio::io::AsyncWriteExt;

/// Embedded database migrations for every supported `SessionMesh` installation.
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");
static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Metadata and immutable bytes for one imported native source object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawObject {
    /// Deterministic raw-object identity supplied by the ingestion boundary.
    pub id: String,
    /// Native bytes stored in the content-addressed blob store.
    pub bytes: Vec<u8>,
    /// Collector-readable path.
    pub source_path: String,
    /// Original host path shown in provenance.
    pub original_path: Option<String>,
    /// Source-local byte offset.
    pub source_offset: u64,
    /// Observed complete source size.
    pub source_size: u64,
    /// Native modification timestamp when available.
    pub source_modified_at: Option<String>,
    /// Native permission bits when available.
    pub source_permissions: Option<u32>,
    /// Stable source generation.
    pub source_generation: String,
    /// Parser version used for this import.
    pub parser_version: String,
    /// Import timestamp.
    pub imported_at: String,
}

/// Durable incremental-ingestion position.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestionCursor {
    /// Stable collector source ID.
    pub source_id: String,
    /// Source generation associated with this offset.
    pub source_generation: String,
    /// Last committed byte boundary.
    pub byte_offset: u64,
    /// Native sequence assigned to the next complete physical record.
    pub next_sequence: u64,
    /// Incomplete final bytes retained until a complete record arrives.
    pub partial_line: Vec<u8>,
    /// Cursor update timestamp.
    pub updated_at: String,
}

/// One atomic ingestion unit.
#[derive(Clone, Debug)]
pub struct IngestionBatch {
    /// Immutable native objects referenced by the events.
    pub raw_objects: Vec<RawObject>,
    /// Validated canonical events.
    pub events: Vec<CanonicalEvent>,
    /// Cursor committed only if all metadata and events commit.
    pub cursor: IngestionCursor,
}

/// Metadata returned for an immutable raw object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredRawObject {
    /// Raw-object identity.
    pub id: String,
    /// Content-addressed blob hash.
    pub blob_hash: String,
    /// Collector-readable source path.
    pub source_path: String,
}

/// Native-session row returned to read APIs without event payloads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredNativeSession {
    /// Native session identity.
    pub id: String,
    /// Tool family.
    pub tool_family: String,
    /// Tool surface.
    pub surface: String,
    /// Discovery profile.
    pub profile: String,
    /// Earliest observed lifecycle timestamp.
    pub started_at: Option<String>,
    /// Latest observed end timestamp.
    pub ended_at: Option<String>,
    /// Remote collector this session was ingested from, if not local.
    pub origin_collector_id: Option<String>,
}

/// Global work context referencing native sessions without copying them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredGlobalSession {
    /// Global identity.
    pub id: String,
    /// Current human objective.
    pub objective: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Latest update timestamp.
    pub updated_at: String,
}

/// Auditable reference from a global session to a native session.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredSessionMember {
    /// Global session identity.
    pub global_session_id: String,
    /// Referenced native session identity.
    pub native_session_id: String,
    /// Explainable correlation confidence.
    pub confidence: f64,
    /// Correlation algorithm version.
    pub correlation_version: String,
    /// Explicit state, if manually decided.
    pub manual_state: Option<String>,
}

/// A registered remote ingestion collector's non-secret identity.
///
/// The bearer token itself is never stored or returned after issuance; only
/// its salted hash is retained for authentication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestionCollector {
    /// Collector identity, used to namespace and audit its ingested data.
    pub id: String,
    /// Human-readable label (e.g. the dedicated system's hostname).
    pub label: String,
    /// Issuance timestamp.
    pub created_at: String,
    /// Revocation timestamp, once revoked.
    pub revoked_at: Option<String>,
}

/// Outcome of one network ingestion attempt, for the audit log.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngestionAuditOutcome {
    /// The batch was authenticated, validated, and committed.
    Accepted,
    /// The batch was rejected before or during commit.
    Rejected,
}

impl IngestionAuditOutcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
        }
    }
}

/// One recorded network ingestion attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestionAuditEntry {
    /// Authenticated collector, when a token was successfully resolved.
    pub collector_id: Option<String>,
    /// When the attempt was handled.
    pub occurred_at: String,
    /// Accepted or rejected.
    pub outcome: IngestionAuditOutcome,
    /// Number of canonical events in the attempted batch.
    pub event_count: u64,
    /// Wire size of the attempted payload in bytes.
    pub byte_size: u64,
    /// Non-secret rejection reason, when rejected.
    pub reason: Option<String>,
}

/// Repository contract for issuing, authenticating, and auditing remote
/// ingestion collectors.
///
/// Kept separate from [`CursorRepository`] because collector identity is an
/// authentication and audit concern, not an ingestion-progress concern: a
/// local daemon scan never touches this trait.
#[async_trait]
pub trait IngestionTokenRepository {
    /// Issues a new collector and its one-time bearer token.
    ///
    /// The returned token is the only time the raw secret is available; only
    /// its hash is persisted.
    ///
    /// # Errors
    ///
    /// Returns a storage error if persistence or entropy generation fails.
    async fn issue_collector(
        &self,
        label: &str,
        created_at: &str,
    ) -> Result<(IngestionCollector, String), StorageError>;

    /// Resolves a bearer token to its non-revoked collector, if any.
    ///
    /// # Errors
    ///
    /// Returns a storage error if the lookup fails.
    async fn authenticate_collector(
        &self,
        token: &str,
    ) -> Result<Option<IngestionCollector>, StorageError>;

    /// Revokes a collector so its token no longer authenticates.
    ///
    /// Returns `false` when no matching, not-yet-revoked collector exists.
    ///
    /// # Errors
    ///
    /// Returns a storage error if persistence fails.
    async fn revoke_collector(&self, id: &str, revoked_at: &str) -> Result<bool, StorageError>;

    /// Lists all registered collectors, revoked or not.
    ///
    /// # Errors
    ///
    /// Returns a storage error if the query fails.
    async fn list_collectors(&self) -> Result<Vec<IngestionCollector>, StorageError>;

    /// Records one network ingestion attempt, accepted or rejected.
    ///
    /// # Errors
    ///
    /// Returns a storage error if persistence fails.
    async fn record_ingestion_audit(&self, entry: &IngestionAuditEntry)
    -> Result<(), StorageError>;
}

/// Explainable proposed relationship between two native sessions.
#[derive(Clone, Debug, PartialEq)]
pub struct StoredCorrelationCandidate {
    /// Deterministic candidate identity.
    pub id: String,
    /// Session that is not yet assigned by this candidate.
    pub left_native_session_id: String,
    /// Existing session providing the target global context.
    pub right_native_session_id: String,
    /// Combined deterministic and content score.
    pub score: f64,
    /// Pending, accepted, or rejected review state.
    pub status: String,
    /// Versioned, human-readable evidence JSON objects.
    pub evidence: Vec<String>,
}

/// One immutable manual membership decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MembershipAuditEntry {
    /// Monotonic audit identity.
    pub id: i64,
    /// Global session identity.
    pub global_session_id: String,
    /// Native session identity.
    pub native_session_id: String,
    /// Decision action.
    pub action: String,
    /// Authenticated local actor class.
    pub actor: String,
    /// Optional non-secret explanation.
    pub reason: Option<String>,
    /// Decision timestamp.
    pub created_at: String,
}

/// Durable versioned handoff JSON and its deterministic snapshot reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredHandoff {
    /// Handoff identity.
    pub id: String,
    /// Global session scope.
    pub global_session_id: String,
    /// Deterministic snapshot identity.
    pub snapshot_id: String,
    /// Schema version.
    pub schema_version: String,
    /// Validated handoff JSON owned by the handoff layer.
    pub handoff_json: String,
    /// Creation timestamp.
    pub created_at: String,
}

/// Shared audit metadata for one membership mutation.
pub struct MembershipDecision<'a> {
    /// Global session identity.
    pub global_session_id: &'a str,
    /// Native session identity.
    pub native_session_id: &'a str,
    /// Authenticated actor class.
    pub actor: &'a str,
    /// Optional safe rationale.
    pub reason: Option<&'a str>,
    /// Decision timestamp.
    pub created_at: &'a str,
}

/// Typed user observation recorded through a controlled integration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRecord {
    /// Stable record identity.
    pub id: String,
    /// Idempotency identity from the caller.
    pub request_id: String,
    /// Global session scope.
    pub global_session_id: String,
    /// `decision` or `task`.
    pub record_type: String,
    /// Observation content; never interpreted as system instructions.
    pub content: String,
    /// Integration origin.
    pub origin: String,
    /// Creation timestamp.
    pub created_at: String,
}

/// Storage failure with corruption and immutability conflicts distinguished.
#[derive(Debug)]
pub enum StorageError {
    /// Database operation failed.
    Database(sqlx::Error),
    /// Migration failed.
    Migration(MigrateError),
    /// Filesystem operation failed.
    Io(std::io::Error),
    /// JSON serialization failed.
    Serialization(String),
    /// Existing immutable metadata conflicts with a repeated identity.
    ImmutableConflict {
        /// Conflicting object kind.
        object: &'static str,
        /// Stable identity that was reused with different content.
        id: String,
    },
    /// Blob content did not match its content address.
    BlobIntegrity {
        /// Expected hash derived from the blob path.
        expected: String,
        /// Hash calculated from the stored bytes.
        actual: String,
    },
    /// Numeric value cannot be represented safely by `SQLite`.
    NumericRange {
        /// Field that exceeded `SQLite`'s signed integer range.
        field: &'static str,
    },
    /// Required identity or path input was empty.
    InvalidInput {
        /// Invalid field.
        field: &'static str,
        /// Safe explanation.
        message: &'static str,
    },
}

impl Display for StorageError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "database error: {error}"),
            Self::Migration(error) => write!(formatter, "migration error: {error}"),
            Self::Io(error) => write!(formatter, "filesystem error: {error}"),
            Self::Serialization(error) => write!(formatter, "serialization error: {error}"),
            Self::ImmutableConflict { object, id } => {
                write!(formatter, "immutable {object} conflict for {id}")
            }
            Self::BlobIntegrity { expected, actual } => {
                write!(
                    formatter,
                    "blob integrity mismatch: expected {expected}, got {actual}"
                )
            }
            Self::NumericRange { field } => {
                write!(formatter, "{field} exceeds SQLite integer range")
            }
            Self::InvalidInput { field, message } => write!(formatter, "{field}: {message}"),
        }
    }
}

impl Error for StorageError {}

impl From<sqlx::Error> for StorageError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl From<MigrateError> for StorageError {
    fn from(error: MigrateError) -> Self {
        Self::Migration(error)
    }
}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Content-addressed filesystem store for large or sensitive native payloads.
#[derive(Clone, Debug)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    /// Creates or opens a blob root with user-only directory permissions.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] if the directory cannot be created or secured.
    pub async fn open(root: impl AsRef<Path>) -> Result<Self, StorageError> {
        let root = root.as_ref().to_path_buf();
        tokio::fs::create_dir_all(&root).await?;
        set_directory_permissions(&root).await?;
        Ok(Self { root })
    }

    /// Writes bytes atomically and returns their SHA-256 content address.
    ///
    /// Existing content is verified before reuse.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] for filesystem failures or an existing corrupt
    /// blob.
    pub async fn put(&self, bytes: &[u8]) -> Result<String, StorageError> {
        let hash = content_hash(bytes);
        let path = self.path_for(&hash);
        if tokio::fs::try_exists(&path).await? {
            self.verify(&hash).await?;
            return Ok(hash);
        }

        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = self
            .root
            .join(format!(".{hash}.{}.{sequence}.tmp", std::process::id()));
        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .await?;
        file.write_all(bytes).await?;
        file.flush().await?;
        set_file_permissions(&temporary).await?;
        file.sync_all().await?;
        drop(file);

        match tokio::fs::rename(&temporary, &path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                tokio::fs::remove_file(&temporary).await?;
            }
            Err(error) => return Err(error.into()),
        }
        self.verify(&hash).await?;
        Ok(hash)
    }

    /// Reads and verifies a blob.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when the blob is missing, unreadable, or does
    /// not match its content address.
    pub async fn get(&self, hash: &str) -> Result<Vec<u8>, StorageError> {
        let bytes = tokio::fs::read(self.path_for(hash)).await?;
        verify_hash(hash, &bytes)?;
        Ok(bytes)
    }

    async fn verify(&self, hash: &str) -> Result<(), StorageError> {
        self.get(hash).await.map(|_| ())
    }

    fn path_for(&self, hash: &str) -> PathBuf {
        self.root.join(hash)
    }
}

/// Concrete local storage composed of `SQLite` and a content-addressed blob root.
#[derive(Clone, Debug)]
pub struct Storage {
    pool: SqlitePool,
    blobs: BlobStore,
}

impl Storage {
    /// Opens storage, enforces `SQLite` safety settings, and runs migrations.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError`] when paths are invalid, permissions cannot be
    /// enforced, the database cannot open, or a migration fails.
    pub async fn open(
        database_path: impl AsRef<Path>,
        blob_root: impl AsRef<Path>,
    ) -> Result<Self, StorageError> {
        let database_path = database_path.as_ref();
        if database_path.as_os_str().is_empty() {
            return Err(StorageError::InvalidInput {
                field: "database_path",
                message: "must not be empty",
            });
        }
        if let Some(parent) = database_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
            set_directory_permissions(parent).await?;
        }

        let options = SqliteConnectOptions::from_str(&format!(
            "sqlite://{}",
            database_path.to_string_lossy()
        ))?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        set_file_permissions(database_path).await?;
        MIGRATOR.run(&pool).await?;
        let blobs = BlobStore::open(blob_root).await?;
        Ok(Self { pool, blobs })
    }

    /// Returns the pool for read-only diagnostics and later repository modules.
    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Returns the content-addressed blob store.
    #[must_use]
    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    /// Lists native sessions in stable identity order.
    ///
    /// # Errors
    ///
    /// Returns a database error when the query fails.
    pub async fn list_native_sessions(&self) -> Result<Vec<StoredNativeSession>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, tool_family, surface, profile, started_at, ended_at, origin_collector_id
             FROM native_sessions ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| StoredNativeSession {
                id: row.get("id"),
                tool_family: row.get("tool_family"),
                surface: row.get("surface"),
                profile: row.get("profile"),
                started_at: row.get("started_at"),
                ended_at: row.get("ended_at"),
                origin_collector_id: row.get("origin_collector_id"),
            })
            .collect())
    }

    /// Loads one native session without secret-bearing event payloads.
    ///
    /// # Errors
    ///
    /// Returns a database error when the query fails.
    pub async fn get_native_session(
        &self,
        id: &str,
    ) -> Result<Option<StoredNativeSession>, StorageError> {
        let row = sqlx::query(
            "SELECT id, tool_family, surface, profile, started_at, ended_at, origin_collector_id
             FROM native_sessions WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| StoredNativeSession {
            id: row.get("id"),
            tool_family: row.get("tool_family"),
            surface: row.get("surface"),
            profile: row.get("profile"),
            started_at: row.get("started_at"),
            ended_at: row.get("ended_at"),
            origin_collector_id: row.get("origin_collector_id"),
        }))
    }

    /// Loads canonical events for internal API projection.
    ///
    /// Callers must explicitly project safe fields before serialization.
    ///
    /// # Errors
    ///
    /// Returns a storage or canonical JSON validation error.
    pub async fn list_canonical_events(&self) -> Result<Vec<CanonicalEvent>, StorageError> {
        let rows = sqlx::query("SELECT canonical_json FROM native_events")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                let json: String = row.get("canonical_json");
                CanonicalEvent::from_json(&json)
                    .map_err(|error| StorageError::Serialization(error.to_string()))
            })
            .collect()
    }

    /// Loads canonical events restricted to the given native session identifiers.
    ///
    /// Uses the `native_events_session_sequence` index instead of a full table
    /// scan, keeping cost proportional to the requested sessions rather than
    /// the entire event history.
    ///
    /// # Errors
    ///
    /// Returns a storage or canonical JSON validation error.
    pub async fn list_canonical_events_for_sessions(
        &self,
        native_session_ids: &[String],
    ) -> Result<Vec<CanonicalEvent>, StorageError> {
        if native_session_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", native_session_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT canonical_json FROM native_events WHERE native_session_id IN ({placeholders})"
        );
        let mut query = sqlx::query(&sql);
        for id in native_session_ids {
            query = query.bind(id);
        }
        let rows = query.fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                let json: String = row.get("canonical_json");
                CanonicalEvent::from_json(&json)
                    .map_err(|error| StorageError::Serialization(error.to_string()))
            })
            .collect()
    }

    /// Loads one canonical event for an explicitly requested detail view.
    ///
    /// Unlike list projections, the returned event includes its payload and
    /// must only be exposed through an authenticated, deliberate reveal.
    ///
    /// # Errors
    ///
    /// Returns a storage or canonical JSON validation error.
    pub async fn get_canonical_event(
        &self,
        id: &str,
    ) -> Result<Option<CanonicalEvent>, StorageError> {
        let json: Option<String> =
            sqlx::query_scalar("SELECT canonical_json FROM native_events WHERE event_id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        json.map(|json| {
            CanonicalEvent::from_json(&json)
                .map_err(|error| StorageError::Serialization(error.to_string()))
        })
        .transpose()
    }

    /// Creates a global session idempotently.
    ///
    /// # Errors
    ///
    /// Returns an immutable conflict when the identity already has a different
    /// objective, or a database error.
    pub async fn create_global_session(
        &self,
        session: &StoredGlobalSession,
    ) -> Result<(), StorageError> {
        validate_non_empty("global_session.id", &session.id)?;
        validate_non_empty("global_session.objective", &session.objective)?;
        let existing: Option<String> =
            sqlx::query_scalar("SELECT objective FROM global_sessions WHERE id = ?")
                .bind(&session.id)
                .fetch_optional(&self.pool)
                .await?;
        if let Some(objective) = existing {
            if objective != session.objective {
                return Err(StorageError::ImmutableConflict {
                    object: "global_session",
                    id: session.id.clone(),
                });
            }
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO global_sessions (id, objective, created_at, updated_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&session.id)
        .bind(&session.objective)
        .bind(&session.created_at)
        .bind(&session.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lists global sessions in stable identity order.
    ///
    /// # Errors
    ///
    /// Returns a database error when sessions cannot be read.
    pub async fn list_global_sessions(&self) -> Result<Vec<StoredGlobalSession>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, objective, created_at, updated_at FROM global_sessions ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| StoredGlobalSession {
                id: row.get("id"),
                objective: row.get("objective"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            })
            .collect())
    }

    /// Lists membership references without native event or transcript copies.
    ///
    /// # Errors
    ///
    /// Returns a database error when members cannot be read.
    pub async fn list_session_members(
        &self,
        global_session_id: &str,
    ) -> Result<Vec<StoredSessionMember>, StorageError> {
        let rows = sqlx::query(
            "SELECT global_session_id, native_session_id, confidence,
                    correlation_version, manual_state
             FROM global_session_members WHERE global_session_id = ?
             ORDER BY native_session_id",
        )
        .bind(global_session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| StoredSessionMember {
                global_session_id: row.get("global_session_id"),
                native_session_id: row.get("native_session_id"),
                confidence: row.get("confidence"),
                correlation_version: row.get("correlation_version"),
                manual_state: row.get("manual_state"),
            })
            .collect())
    }

    /// Lists every global-session membership for deterministic reconciliation.
    ///
    /// # Errors
    ///
    /// Returns a database error when memberships cannot be read.
    pub async fn list_all_session_members(&self) -> Result<Vec<StoredSessionMember>, StorageError> {
        let rows = sqlx::query(
            "SELECT global_session_id, native_session_id, confidence,
                    correlation_version, manual_state
             FROM global_session_members ORDER BY global_session_id, native_session_id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| StoredSessionMember {
                global_session_id: row.get("global_session_id"),
                native_session_id: row.get("native_session_id"),
                confidence: row.get("confidence"),
                correlation_version: row.get("correlation_version"),
                manual_state: row.get("manual_state"),
            })
            .collect())
    }

    /// Advances the global-session freshness boundary after member ingestion.
    ///
    /// # Errors
    ///
    /// Returns a database error when the session cannot be updated.
    pub async fn touch_global_session(
        &self,
        global_session_id: &str,
        updated_at: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "UPDATE global_sessions SET updated_at = ?
             WHERE id = ? AND updated_at < ?",
        )
        .bind(updated_at)
        .bind(global_session_id)
        .bind(updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Stores a correlation candidate and replaces its versioned evidence.
    ///
    /// # Errors
    ///
    /// Returns an input or database error.
    pub async fn upsert_correlation_candidate(
        &self,
        candidate: &StoredCorrelationCandidate,
    ) -> Result<(), StorageError> {
        if !(0.0..=1.0).contains(&candidate.score) {
            return Err(StorageError::InvalidInput {
                field: "candidate.score",
                message: "must be between zero and one",
            });
        }
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO correlation_candidates
                (id, left_native_session_id, right_native_session_id, score, status)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET score = excluded.score,
                status = CASE WHEN correlation_candidates.status = 'rejected'
                              THEN 'rejected' ELSE excluded.status END",
        )
        .bind(&candidate.id)
        .bind(&candidate.left_native_session_id)
        .bind(&candidate.right_native_session_id)
        .bind(candidate.score)
        .bind(&candidate.status)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM correlation_evidence WHERE candidate_id = ?")
            .bind(&candidate.id)
            .execute(&mut *transaction)
            .await?;
        for (index, evidence) in candidate.evidence.iter().enumerate() {
            sqlx::query(
                "INSERT INTO correlation_evidence
                    (id, candidate_id, evidence_type, weight, evidence_json)
                 VALUES (?, ?, 'explainable-signal', 1.0, ?)",
            )
            .bind(format!("{}:{index}", candidate.id))
            .bind(&candidate.id)
            .bind(evidence)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Lists correlation candidates and their evidence for review.
    ///
    /// # Errors
    ///
    /// Returns a database error.
    pub async fn list_correlation_candidates(
        &self,
    ) -> Result<Vec<StoredCorrelationCandidate>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, left_native_session_id, right_native_session_id, score, status
             FROM correlation_candidates ORDER BY score DESC, id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut candidates = Vec::with_capacity(rows.len());
        for row in rows {
            let id: String = row.get("id");
            let evidence = sqlx::query_scalar(
                "SELECT evidence_json FROM correlation_evidence
                 WHERE candidate_id = ? ORDER BY id",
            )
            .bind(&id)
            .fetch_all(&self.pool)
            .await?;
            candidates.push(StoredCorrelationCandidate {
                id,
                left_native_session_id: row.get("left_native_session_id"),
                right_native_session_id: row.get("right_native_session_id"),
                score: row.get("score"),
                status: row.get("status"),
                evidence,
            });
        }
        Ok(candidates)
    }

    /// Persists an explicit candidate review decision.
    ///
    /// # Errors
    ///
    /// Returns an input or database error.
    pub async fn set_correlation_candidate_status(
        &self,
        id: &str,
        status: &str,
    ) -> Result<(), StorageError> {
        if !matches!(status, "accepted" | "rejected") {
            return Err(StorageError::InvalidInput {
                field: "candidate.status",
                message: "must be accepted or rejected",
            });
        }
        sqlx::query("UPDATE correlation_candidates SET status = ? WHERE id = ?")
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Applies and audits a manual membership decision transactionally.
    ///
    /// Rejected pairs cannot be linked until `reverse_rejection` is called.
    ///
    /// # Errors
    ///
    /// Returns an input, conflict, or database error.
    pub async fn link_session(
        &self,
        decision: &MembershipDecision<'_>,
        confidence: f64,
        correlation_version: &str,
    ) -> Result<(), StorageError> {
        if !(0.0..=1.0).contains(&confidence) {
            return Err(StorageError::InvalidInput {
                field: "confidence",
                message: "must be between zero and one",
            });
        }
        let target = membership_target(decision.global_session_id, decision.native_session_id);
        let rejected: Option<String> = sqlx::query_scalar(
            "SELECT value_json FROM user_overrides
             WHERE override_type = 'membership_rejection' AND target_id = ?",
        )
        .bind(&target)
        .fetch_optional(&self.pool)
        .await?;
        if rejected.is_some() {
            return Err(StorageError::InvalidInput {
                field: "membership",
                message: "manual rejection must be explicitly reversed",
            });
        }
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO global_session_members (
                global_session_id, native_session_id, confidence,
                correlation_version, manual_state
             ) VALUES (?, ?, ?, ?, 'accepted')
             ON CONFLICT(global_session_id, native_session_id) DO UPDATE SET
                confidence = excluded.confidence,
                correlation_version = excluded.correlation_version,
                manual_state = 'accepted'",
        )
        .bind(decision.global_session_id)
        .bind(decision.native_session_id)
        .bind(confidence)
        .bind(correlation_version)
        .execute(&mut *transaction)
        .await?;
        insert_audit(
            &mut transaction,
            decision.global_session_id,
            decision.native_session_id,
            "link",
            decision.actor,
            decision.reason,
            decision.created_at,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Removes a membership reference and records the decision.
    ///
    /// # Errors
    ///
    /// Returns a database error if the transactional decision fails.
    pub async fn unlink_session(
        &self,
        decision: &MembershipDecision<'_>,
    ) -> Result<(), StorageError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM global_session_members
             WHERE global_session_id = ? AND native_session_id = ?",
        )
        .bind(decision.global_session_id)
        .bind(decision.native_session_id)
        .execute(&mut *transaction)
        .await?;
        insert_audit(
            &mut transaction,
            decision.global_session_id,
            decision.native_session_id,
            "unlink",
            decision.actor,
            decision.reason,
            decision.created_at,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Persists a rejection that blocks automatic and manual silent relinking.
    ///
    /// # Errors
    ///
    /// Returns a database error if the transactional override fails.
    pub async fn reject_membership(
        &self,
        decision: &MembershipDecision<'_>,
    ) -> Result<(), StorageError> {
        let target = membership_target(decision.global_session_id, decision.native_session_id);
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM global_session_members
             WHERE global_session_id = ? AND native_session_id = ?",
        )
        .bind(decision.global_session_id)
        .bind(decision.native_session_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO user_overrides (
                id, override_type, target_id, value_json, created_at
             ) VALUES (?, 'membership_rejection', ?, '{\"rejected\":true}', ?)
             ON CONFLICT(override_type, target_id) DO UPDATE SET
                value_json = excluded.value_json,
                created_at = excluded.created_at",
        )
        .bind(format!("reject:{target}"))
        .bind(&target)
        .bind(decision.created_at)
        .execute(&mut *transaction)
        .await?;
        insert_audit(
            &mut transaction,
            decision.global_session_id,
            decision.native_session_id,
            "reject",
            decision.actor,
            decision.reason,
            decision.created_at,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Explicitly reverses a prior rejection.
    ///
    /// # Errors
    ///
    /// Returns a database error if the transactional reversal fails.
    pub async fn reverse_rejection(
        &self,
        global_session_id: &str,
        native_session_id: &str,
        actor: &str,
        created_at: &str,
    ) -> Result<(), StorageError> {
        let target = membership_target(global_session_id, native_session_id);
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "DELETE FROM user_overrides
             WHERE override_type = 'membership_rejection' AND target_id = ?",
        )
        .bind(&target)
        .execute(&mut *transaction)
        .await?;
        insert_audit(
            &mut transaction,
            global_session_id,
            native_session_id,
            "reverse_rejection",
            actor,
            None,
            created_at,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Returns immutable membership audit entries.
    ///
    /// # Errors
    ///
    /// Returns a database error when audit entries cannot be read.
    pub async fn membership_audit(
        &self,
        global_session_id: &str,
    ) -> Result<Vec<MembershipAuditEntry>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, global_session_id, native_session_id, action, actor,
                    reason, created_at
             FROM membership_audit_log WHERE global_session_id = ? ORDER BY id",
        )
        .bind(global_session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| MembershipAuditEntry {
                id: row.get("id"),
                global_session_id: row.get("global_session_id"),
                native_session_id: row.get("native_session_id"),
                action: row.get("action"),
                actor: row.get("actor"),
                reason: row.get("reason"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    /// Stores an immutable state snapshot and handoff atomically.
    ///
    /// # Errors
    ///
    /// Returns a database error when the transaction cannot commit.
    pub async fn store_handoff(
        &self,
        handoff: &StoredHandoff,
        snapshot_json: &str,
    ) -> Result<(), StorageError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO state_snapshots (id, global_session_id, created_at, snapshot_json)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(&handoff.snapshot_id)
        .bind(&handoff.global_session_id)
        .bind(&handoff.created_at)
        .bind(snapshot_json)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO handoffs (
                id, global_session_id, snapshot_id, schema_version,
                handoff_json, created_at
             ) VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(&handoff.id)
        .bind(&handoff.global_session_id)
        .bind(&handoff.snapshot_id)
        .bind(&handoff.schema_version)
        .bind(&handoff.handoff_json)
        .bind(&handoff.created_at)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Loads the latest handoff for one global session.
    ///
    /// # Errors
    ///
    /// Returns a database error when the handoff cannot be read.
    pub async fn latest_handoff(
        &self,
        global_session_id: &str,
    ) -> Result<Option<StoredHandoff>, StorageError> {
        let row = sqlx::query(
            "SELECT id, global_session_id, snapshot_id, schema_version,
                    handoff_json, created_at
             FROM handoffs WHERE global_session_id = ?
             ORDER BY created_at DESC, id DESC LIMIT 1",
        )
        .bind(global_session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| StoredHandoff {
            id: row.get("id"),
            global_session_id: row.get("global_session_id"),
            snapshot_id: row.get("snapshot_id"),
            schema_version: row.get("schema_version"),
            handoff_json: row.get("handoff_json"),
            created_at: row.get("created_at"),
        }))
    }

    /// Stores a typed session observation idempotently by request ID.
    ///
    /// # Errors
    ///
    /// Returns an input, immutable conflict, or database error.
    pub async fn store_session_record(&self, record: &SessionRecord) -> Result<bool, StorageError> {
        if !matches!(record.record_type.as_str(), "decision" | "task") {
            return Err(StorageError::InvalidInput {
                field: "record_type",
                message: "must be decision or task",
            });
        }
        let existing: Option<(String, String, String)> = sqlx::query_as(
            "SELECT global_session_id, record_type, content
             FROM session_records WHERE request_id = ?",
        )
        .bind(&record.request_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(existing) = existing {
            if existing
                != (
                    record.global_session_id.clone(),
                    record.record_type.clone(),
                    record.content.clone(),
                )
            {
                return Err(StorageError::ImmutableConflict {
                    object: "session_record",
                    id: record.request_id.clone(),
                });
            }
            return Ok(false);
        }
        let result = sqlx::query(
            "INSERT INTO session_records (
                id, request_id, global_session_id, record_type,
                content, origin, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&record.id)
        .bind(&record.request_id)
        .bind(&record.global_session_id)
        .bind(&record.record_type)
        .bind(&record.content)
        .bind(&record.origin)
        .bind(&record.created_at)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Lists typed observations for handoff generation.
    ///
    /// # Errors
    ///
    /// Returns a database error when records cannot be read.
    pub async fn list_session_records(
        &self,
        global_session_id: &str,
    ) -> Result<Vec<SessionRecord>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, request_id, global_session_id, record_type,
                    content, origin, created_at
             FROM session_records WHERE global_session_id = ?
             ORDER BY created_at, id",
        )
        .bind(global_session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| SessionRecord {
                id: row.get("id"),
                request_id: row.get("request_id"),
                global_session_id: row.get("global_session_id"),
                record_type: row.get("record_type"),
                content: row.get("content"),
                origin: row.get("origin"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    /// Searches canonical events with bounded offset pagination.
    ///
    /// # Errors
    ///
    /// Returns a database or canonical validation error.
    pub async fn search_events(
        &self,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<CanonicalEvent>, StorageError> {
        let limit = i64::try_from(limit).map_err(|_| StorageError::NumericRange {
            field: "search_limit",
        })?;
        let offset = i64::try_from(offset).map_err(|_| StorageError::NumericRange {
            field: "search_offset",
        })?;
        let rows = sqlx::query(
            "SELECT e.canonical_json
             FROM native_events_fts f
             JOIN native_events e ON e.event_id = f.event_id
             WHERE native_events_fts MATCH ?
             ORDER BY e.timestamp, e.sequence, e.event_id
             LIMIT ? OFFSET ?",
        )
        .bind(query)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let json: String = row.get("canonical_json");
                CanonicalEvent::from_json(&json)
                    .map_err(|error| StorageError::Serialization(error.to_string()))
            })
            .collect()
    }
}

async fn insert_audit(
    transaction: &mut Transaction<'_, Sqlite>,
    global_session_id: &str,
    native_session_id: &str,
    action: &str,
    actor: &str,
    reason: Option<&str>,
    created_at: &str,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO membership_audit_log (
            global_session_id, native_session_id, action, actor, reason, created_at
         ) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(global_session_id)
    .bind(native_session_id)
    .bind(action)
    .bind(actor)
    .bind(reason)
    .bind(created_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn membership_target(global_session_id: &str, native_session_id: &str) -> String {
    format!("{global_session_id}:{native_session_id}")
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), StorageError> {
    if value.trim().is_empty() {
        Err(StorageError::InvalidInput {
            field,
            message: "must not be empty",
        })
    } else {
        Ok(())
    }
}

/// Repository contract for immutable raw imports.
#[async_trait]
pub trait RawObjectRepository {
    /// Stores an object once or confirms an identical prior insert.
    async fn store_raw_object(&self, raw: &RawObject) -> Result<StoredRawObject, StorageError>;

    /// Loads immutable metadata by identity.
    async fn get_raw_object(&self, id: &str) -> Result<Option<StoredRawObject>, StorageError>;
}

/// Repository contract for idempotent normalized events.
#[async_trait]
pub trait EventRepository {
    /// Stores a canonical event and returns whether it was newly inserted.
    async fn store_event(&self, event: &CanonicalEvent) -> Result<bool, StorageError>;

    /// Counts stored canonical events.
    async fn event_count(&self) -> Result<u64, StorageError>;
}

/// Repository contract for atomic ingestion progress.
#[async_trait]
pub trait CursorRepository {
    /// Loads the last committed cursor for one source.
    async fn get_cursor(&self, source_id: &str) -> Result<Option<IngestionCursor>, StorageError>;

    /// Commits raw objects, events, and the new cursor atomically.
    ///
    /// `origin_collector_id` attributes newly created native sessions to a
    /// remote [`IngestionCollector`]; pass `None` for local daemon ingestion.
    async fn commit_batch(
        &self,
        batch: &IngestionBatch,
        origin_collector_id: Option<&str>,
    ) -> Result<(), StorageError>;
}

#[async_trait]
impl RawObjectRepository for Storage {
    async fn store_raw_object(&self, raw: &RawObject) -> Result<StoredRawObject, StorageError> {
        let blob_hash = self.blobs.put(&raw.bytes).await?;
        let mut transaction = self.pool.begin().await?;
        let stored = insert_raw(&mut transaction, raw, &blob_hash).await?;
        transaction.commit().await?;
        Ok(stored)
    }

    async fn get_raw_object(&self, id: &str) -> Result<Option<StoredRawObject>, StorageError> {
        let row = sqlx::query("SELECT id, blob_hash, source_path FROM raw_objects WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| StoredRawObject {
            id: row.get("id"),
            blob_hash: row.get("blob_hash"),
            source_path: row.get("source_path"),
        }))
    }
}

#[async_trait]
impl EventRepository for Storage {
    async fn store_event(&self, event: &CanonicalEvent) -> Result<bool, StorageError> {
        let mut transaction = self.pool.begin().await?;
        let inserted = insert_event(&mut transaction, event, None, None).await?;
        transaction.commit().await?;
        Ok(inserted)
    }

    async fn event_count(&self) -> Result<u64, StorageError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM native_events")
            .fetch_one(&self.pool)
            .await?;
        u64::try_from(count).map_err(|_| StorageError::NumericRange {
            field: "event_count",
        })
    }
}

#[async_trait]
impl CursorRepository for Storage {
    async fn get_cursor(&self, source_id: &str) -> Result<Option<IngestionCursor>, StorageError> {
        let row = sqlx::query(
            "SELECT source_id, source_generation, byte_offset, next_sequence, partial_line, updated_at
             FROM ingestion_cursors WHERE source_id = ?",
        )
        .bind(source_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(IngestionCursor {
                source_id: row.get("source_id"),
                source_generation: row.get("source_generation"),
                byte_offset: to_u64(row.get("byte_offset"), "byte_offset")?,
                next_sequence: to_u64(row.get("next_sequence"), "next_sequence")?,
                partial_line: row.get("partial_line"),
                updated_at: row.get("updated_at"),
            })
        })
        .transpose()
    }

    async fn commit_batch(
        &self,
        batch: &IngestionBatch,
        origin_collector_id: Option<&str>,
    ) -> Result<(), StorageError> {
        validate_cursor(&batch.cursor)?;
        let mut blob_hashes = Vec::with_capacity(batch.raw_objects.len());
        for raw in &batch.raw_objects {
            blob_hashes.push(self.blobs.put(&raw.bytes).await?);
        }

        let mut transaction = self.pool.begin().await?;
        for (raw, blob_hash) in batch.raw_objects.iter().zip(blob_hashes.iter()) {
            insert_raw(&mut transaction, raw, blob_hash).await?;
        }
        for event in &batch.events {
            let raw_object_id = batch
                .raw_objects
                .iter()
                .find(|raw| {
                    raw.source_generation == event.provenance.source_generation
                        && raw.source_offset == event.provenance.source_offset
                })
                .map(|raw| raw.id.as_str());
            insert_event(&mut transaction, event, raw_object_id, origin_collector_id).await?;
        }
        upsert_cursor(&mut transaction, &batch.cursor).await?;
        transaction.commit().await?;
        Ok(())
    }
}

#[async_trait]
impl IngestionTokenRepository for Storage {
    async fn issue_collector(
        &self,
        label: &str,
        created_at: &str,
    ) -> Result<(IngestionCollector, String), StorageError> {
        validate_non_empty("collector.label", label)?;
        validate_non_empty("collector.created_at", created_at)?;
        let id = format!("collector_{}", random_hex(16)?);
        let token = random_hex(32)?;
        let token_hash = hex_digest(token.as_bytes());
        sqlx::query(
            "INSERT INTO ingestion_collectors (id, label, token_hash, created_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(label)
        .bind(&token_hash)
        .bind(created_at)
        .execute(&self.pool)
        .await?;
        Ok((
            IngestionCollector {
                id,
                label: label.to_owned(),
                created_at: created_at.to_owned(),
                revoked_at: None,
            },
            token,
        ))
    }

    async fn authenticate_collector(
        &self,
        token: &str,
    ) -> Result<Option<IngestionCollector>, StorageError> {
        let token_hash = hex_digest(token.as_bytes());
        let row = sqlx::query(
            "SELECT id, label, created_at, revoked_at FROM ingestion_collectors
             WHERE token_hash = ? AND revoked_at IS NULL",
        )
        .bind(&token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| IngestionCollector {
            id: row.get("id"),
            label: row.get("label"),
            created_at: row.get("created_at"),
            revoked_at: row.get("revoked_at"),
        }))
    }

    async fn revoke_collector(&self, id: &str, revoked_at: &str) -> Result<bool, StorageError> {
        let result = sqlx::query(
            "UPDATE ingestion_collectors SET revoked_at = ?
             WHERE id = ? AND revoked_at IS NULL",
        )
        .bind(revoked_at)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    async fn list_collectors(&self) -> Result<Vec<IngestionCollector>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, label, created_at, revoked_at FROM ingestion_collectors ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| IngestionCollector {
                id: row.get("id"),
                label: row.get("label"),
                created_at: row.get("created_at"),
                revoked_at: row.get("revoked_at"),
            })
            .collect())
    }

    async fn record_ingestion_audit(
        &self,
        entry: &IngestionAuditEntry,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO ingestion_audit_log (
                collector_id, occurred_at, outcome, event_count, byte_size, reason
             ) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.collector_id)
        .bind(entry.occurred_at.as_str())
        .bind(entry.outcome.as_str())
        .bind(to_i64(entry.event_count, "event_count")?)
        .bind(to_i64(entry.byte_size, "byte_size")?)
        .bind(entry.reason.as_deref())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

/// Generates `byte_count` random bytes encoded as lowercase hex.
fn random_hex(byte_count: usize) -> Result<String, StorageError> {
    let mut bytes = vec![0_u8; byte_count];
    getrandom::fill(&mut bytes)
        .map_err(|error| StorageError::Io(std::io::Error::other(format!("{error}"))))?;
    let mut value = String::with_capacity(byte_count * 2);
    for byte in bytes {
        write!(&mut value, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(value)
}

/// Plain lowercase-hex SHA-256 digest, without the `sha256:` content-address
/// prefix used for blob hashes.
fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut value = String::with_capacity(64);
    for byte in digest {
        write!(&mut value, "{byte:02x}").expect("writing to a String cannot fail");
    }
    value
}

async fn insert_raw(
    transaction: &mut Transaction<'_, Sqlite>,
    raw: &RawObject,
    blob_hash: &str,
) -> Result<StoredRawObject, StorageError> {
    validate_raw(raw)?;
    let metadata_hash = raw_metadata_hash(raw);
    let existing: Option<(String, String, String)> = sqlx::query_as(
        "SELECT blob_hash, metadata_hash, source_path FROM raw_objects WHERE id = ?",
    )
    .bind(&raw.id)
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some((existing_hash, existing_metadata_hash, existing_path)) = existing {
        if existing_hash != blob_hash || existing_metadata_hash != metadata_hash {
            return Err(StorageError::ImmutableConflict {
                object: "raw_object",
                id: raw.id.clone(),
            });
        }
        return Ok(StoredRawObject {
            id: raw.id.clone(),
            blob_hash: existing_hash,
            source_path: existing_path,
        });
    }

    sqlx::query(
        "INSERT INTO raw_objects (
            id, blob_hash, metadata_hash, source_path, original_path, source_offset, source_size,
            source_modified_at, source_permissions, source_generation,
            parser_version, imported_at
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&raw.id)
    .bind(blob_hash)
    .bind(metadata_hash)
    .bind(&raw.source_path)
    .bind(&raw.original_path)
    .bind(to_i64(raw.source_offset, "source_offset")?)
    .bind(to_i64(raw.source_size, "source_size")?)
    .bind(&raw.source_modified_at)
    .bind(raw.source_permissions.map(i64::from))
    .bind(&raw.source_generation)
    .bind(&raw.parser_version)
    .bind(&raw.imported_at)
    .execute(&mut **transaction)
    .await?;
    Ok(StoredRawObject {
        id: raw.id.clone(),
        blob_hash: blob_hash.to_owned(),
        source_path: raw.source_path.clone(),
    })
}

async fn insert_event(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &CanonicalEvent,
    raw_object_id: Option<&str>,
    origin_collector_id: Option<&str>,
) -> Result<bool, StorageError> {
    let canonical_json = event
        .to_json()
        .map_err(|error| StorageError::Serialization(error.to_string()))?;
    sqlx::query(
        "INSERT INTO native_sessions (id, tool_family, surface, profile, origin_collector_id)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(&event.native_session_id)
    .bind(&event.tool.family)
    .bind(&event.tool.surface)
    .bind(&event.tool.profile)
    .bind(origin_collector_id)
    .execute(&mut **transaction)
    .await?;
    if event.kind == sessionmesh_core::event::EventKind::SessionStart {
        sqlx::query(
            "UPDATE native_sessions
             SET started_at = COALESCE(started_at, ?)
             WHERE id = ?",
        )
        .bind(event.timestamp.as_str())
        .bind(&event.native_session_id)
        .execute(&mut **transaction)
        .await?;
    } else if event.kind == sessionmesh_core::event::EventKind::SessionEnd {
        sqlx::query("UPDATE native_sessions SET ended_at = ? WHERE id = ?")
            .bind(event.timestamp.as_str())
            .bind(&event.native_session_id)
            .execute(&mut **transaction)
            .await?;
    }
    let result = sqlx::query(
        "INSERT INTO native_events (
            event_id, native_session_id, sequence, timestamp, kind,
            canonical_json, raw_object_id, source_offset
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(event_id) DO NOTHING",
    )
    .bind(event.event_id.as_str())
    .bind(&event.native_session_id)
    .bind(to_i64(event.sequence, "sequence")?)
    .bind(event.timestamp.as_str())
    .bind(format!("{:?}", event.kind))
    .bind(&canonical_json)
    .bind(raw_object_id)
    .bind(to_i64(event.provenance.source_offset, "source_offset")?)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() == 1 {
        sqlx::query(
            "INSERT INTO native_events_fts (event_id, searchable_text)
             VALUES (?, ?)",
        )
        .bind(event.event_id.as_str())
        .bind(&canonical_json)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(result.rows_affected() == 1)
}

async fn upsert_cursor(
    transaction: &mut Transaction<'_, Sqlite>,
    cursor: &IngestionCursor,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO ingestion_cursors (
            source_id, source_generation, byte_offset, next_sequence, partial_line, updated_at
         ) VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(source_id) DO UPDATE SET
            source_generation = excluded.source_generation,
            byte_offset = excluded.byte_offset,
            next_sequence = excluded.next_sequence,
            partial_line = excluded.partial_line,
            updated_at = excluded.updated_at",
    )
    .bind(&cursor.source_id)
    .bind(&cursor.source_generation)
    .bind(to_i64(cursor.byte_offset, "byte_offset")?)
    .bind(to_i64(cursor.next_sequence, "next_sequence")?)
    .bind(&cursor.partial_line)
    .bind(&cursor.updated_at)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn validate_raw(raw: &RawObject) -> Result<(), StorageError> {
    for (field, value) in [
        ("raw_object.id", raw.id.as_str()),
        ("raw_object.source_path", raw.source_path.as_str()),
        (
            "raw_object.source_generation",
            raw.source_generation.as_str(),
        ),
        ("raw_object.parser_version", raw.parser_version.as_str()),
        ("raw_object.imported_at", raw.imported_at.as_str()),
    ] {
        if value.is_empty() {
            return Err(StorageError::InvalidInput {
                field,
                message: "must not be empty",
            });
        }
    }
    Ok(())
}

fn validate_cursor(cursor: &IngestionCursor) -> Result<(), StorageError> {
    for (field, value) in [
        ("cursor.source_id", cursor.source_id.as_str()),
        (
            "cursor.source_generation",
            cursor.source_generation.as_str(),
        ),
        ("cursor.updated_at", cursor.updated_at.as_str()),
    ] {
        if value.is_empty() {
            return Err(StorageError::InvalidInput {
                field,
                message: "must not be empty",
            });
        }
    }
    Ok(())
}

fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut value = String::with_capacity(71);
    value.push_str("sha256:");
    for byte in digest {
        write!(&mut value, "{byte:02x}").expect("writing to a String cannot fail");
    }
    value
}

fn raw_metadata_hash(raw: &RawObject) -> String {
    let mut hasher = Sha256::new();
    update_hash_part(&mut hasher, &raw.source_offset.to_be_bytes());
    update_hash_part(&mut hasher, raw.source_generation.as_bytes());
    update_hash_part(&mut hasher, raw.parser_version.as_bytes());
    prefixed_digest(hasher.finalize())
}

fn update_hash_part(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(value.len().to_be_bytes());
    hasher.update(value);
}

fn prefixed_digest(digest: impl IntoIterator<Item = u8>) -> String {
    let mut value = String::with_capacity(71);
    value.push_str("sha256:");
    for byte in digest {
        write!(&mut value, "{byte:02x}").expect("writing to a String cannot fail");
    }
    value
}

fn verify_hash(expected: &str, bytes: &[u8]) -> Result<(), StorageError> {
    let actual = content_hash(bytes);
    if expected == actual {
        Ok(())
    } else {
        Err(StorageError::BlobIntegrity {
            expected: expected.to_owned(),
            actual,
        })
    }
}

fn to_i64(value: u64, field: &'static str) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::NumericRange { field })
}

fn to_u64(value: i64, field: &'static str) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::NumericRange { field })
}

#[cfg(unix)]
async fn set_file_permissions(path: &Path) -> Result<(), StorageError> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn set_file_permissions(_path: &Path) -> Result<(), StorageError> {
    Ok(())
}

#[cfg(unix)]
async fn set_directory_permissions(path: &Path) -> Result<(), StorageError> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
    Ok(())
}

#[cfg(not(unix))]
async fn set_directory_permissions(_path: &Path) -> Result<(), StorageError> {
    Ok(())
}

pub use sessionmesh_core::bootstrap_stage;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sessionmesh_core::event::{
        CanonicalEvent, EventDraft, EventKind, EventProvenance, EventTimestamp, TimestampPrecision,
        ToolIdentity,
    };
    use tempfile::TempDir;

    use super::*;

    async fn test_storage() -> (TempDir, Storage) {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let storage = Storage::open(
            directory.path().join("sessionmesh.db"),
            directory.path().join("blobs"),
        )
        .await
        .expect("test storage should open");
        (directory, storage)
    }

    fn raw(id: &str, bytes: &[u8]) -> RawObject {
        RawObject {
            id: id.to_owned(),
            bytes: bytes.to_vec(),
            source_path: "/sources/codex/rollout.jsonl".to_owned(),
            original_path: Some("~/.codex/rollout.jsonl".to_owned()),
            source_offset: 0,
            source_size: u64::try_from(bytes.len()).expect("fixture length fits u64"),
            source_modified_at: Some("2026-07-20T14:31:22Z".to_owned()),
            source_permissions: Some(0o600),
            source_generation: "device:inode".to_owned(),
            parser_version: "0.1.0".to_owned(),
            imported_at: "2026-07-20T14:31:23Z".to_owned(),
        }
    }

    fn event(sequence: u64) -> CanonicalEvent {
        CanonicalEvent::from_draft(EventDraft {
            tool: ToolIdentity {
                family: "codex".to_owned(),
                surface: "cli".to_owned(),
                profile: "default".to_owned(),
            },
            native_session_id: "native-session".to_owned(),
            sequence,
            timestamp: EventTimestamp::parse("2026-07-20T14:31:22Z")
                .expect("fixture timestamp is valid"),
            timestamp_precision: TimestampPrecision::Second,
            kind: EventKind::UserMessage,
            workspace: None,
            payload: BTreeMap::from([("text".to_owned(), "hello".into())]),
            provenance: EventProvenance {
                source_path: "/sources/codex/rollout.jsonl".to_owned(),
                original_path: Some("~/.codex/rollout.jsonl".to_owned()),
                source_offset: sequence,
                source_generation: "device:inode".to_owned(),
                adapter_version: "0.1.0".to_owned(),
                ingestion_sequence: sequence,
                ordering_confidence: Some(1.0),
            },
        })
        .expect("fixture event should be valid")
    }

    fn cursor(offset: u64) -> IngestionCursor {
        IngestionCursor {
            source_id: "codex-rollout".to_owned(),
            source_generation: "device:inode".to_owned(),
            byte_offset: offset,
            next_sequence: offset,
            partial_line: Vec::new(),
            updated_at: "2026-07-20T14:31:24Z".to_owned(),
        }
    }

    #[tokio::test]
    async fn enables_wal_and_foreign_keys() {
        let (_directory, storage) = test_storage().await;
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(storage.pool())
            .await
            .expect("journal mode should be queryable");
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(storage.pool())
            .await
            .expect("foreign key mode should be queryable");

        assert_eq!(journal_mode.to_lowercase(), "wal");
        assert_eq!(foreign_keys, 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn creates_database_with_user_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let (directory, _storage) = test_storage().await;
        let mode = tokio::fs::metadata(directory.path().join("sessionmesh.db"))
            .await
            .expect("database metadata should exist")
            .permissions()
            .mode()
            & 0o777;

        assert_eq!(mode, 0o600);
    }

    #[tokio::test]
    async fn repeated_event_insert_is_idempotent() {
        let (_directory, storage) = test_storage().await;
        let event = event(1);

        assert!(
            storage
                .store_event(&event)
                .await
                .expect("first insert works")
        );
        assert!(
            !storage
                .store_event(&event)
                .await
                .expect("repeat insert works")
        );
        assert_eq!(storage.event_count().await.expect("count works"), 1);
    }

    #[tokio::test]
    async fn raw_objects_are_immutable() {
        let (_directory, storage) = test_storage().await;
        storage
            .store_raw_object(&raw("raw-1", b"first"))
            .await
            .expect("first raw object should store");

        let error = storage
            .store_raw_object(&raw("raw-1", b"changed"))
            .await
            .expect_err("changed content must conflict");

        assert!(matches!(error, StorageError::ImmutableConflict { .. }));
        let stored = storage
            .get_raw_object("raw-1")
            .await
            .expect("raw lookup should work")
            .expect("raw object should remain");
        assert_eq!(
            storage
                .blobs()
                .get(&stored.blob_hash)
                .await
                .expect("original blob remains"),
            b"first"
        );
    }

    #[tokio::test]
    async fn raw_metadata_is_part_of_the_immutable_identity() {
        let (_directory, storage) = test_storage().await;
        let original = raw("raw-1", b"same bytes");
        storage
            .store_raw_object(&original)
            .await
            .expect("first raw object should store");

        let mut changed = original;
        changed.parser_version = "0.2.0".to_owned();
        let error = storage
            .store_raw_object(&changed)
            .await
            .expect_err("changed provenance metadata must conflict");

        assert!(matches!(error, StorageError::ImmutableConflict { .. }));
    }

    #[tokio::test]
    async fn repeated_raw_record_tolerates_changed_observation_metadata() {
        let (_directory, storage) = test_storage().await;
        let original = raw("raw-1", b"same bytes");
        storage
            .store_raw_object(&original)
            .await
            .expect("first observation should store");

        let mut repeated = original;
        repeated.source_path = "/renamed/rollout.jsonl".to_owned();
        repeated.source_size = 4_096;
        repeated.source_modified_at = Some("2026-07-20T15:00:00Z".to_owned());
        repeated.source_permissions = Some(0o400);
        repeated.imported_at = "2026-07-20T15:00:01Z".to_owned();

        storage
            .store_raw_object(&repeated)
            .await
            .expect("same native record should remain idempotent");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_blob_writes_are_idempotent() {
        let (_directory, storage) = test_storage().await;
        let first_store = storage.blobs().clone();
        let second_store = storage.blobs().clone();

        let first = tokio::spawn(async move { first_store.put(b"shared content").await });
        let second = tokio::spawn(async move { second_store.put(b"shared content").await });
        let first_hash = first
            .await
            .expect("first writer should join")
            .expect("first writer should store");
        let second_hash = second
            .await
            .expect("second writer should join")
            .expect("second writer should store");

        assert_eq!(first_hash, second_hash);
        assert_eq!(
            storage
                .blobs()
                .get(&first_hash)
                .await
                .expect("stored blob should remain valid"),
            b"shared content"
        );
    }

    #[tokio::test]
    async fn blob_reads_detect_tampering() {
        let (directory, storage) = test_storage().await;
        let hash = storage
            .blobs()
            .put(b"trusted")
            .await
            .expect("blob should store");
        tokio::fs::write(directory.path().join("blobs").join(&hash), b"tampered")
            .await
            .expect("test should tamper with blob");

        let error = storage
            .blobs()
            .get(&hash)
            .await
            .expect_err("tampering must be detected");
        assert!(matches!(error, StorageError::BlobIntegrity { .. }));
    }

    #[tokio::test]
    async fn batch_failure_rolls_back_raw_metadata_events_and_cursor() {
        let (_directory, storage) = test_storage().await;
        let mut conflicting = raw("raw-1", b"different");
        conflicting.source_path = "/other/source".to_owned();
        let batch = IngestionBatch {
            raw_objects: vec![raw("raw-1", b"first"), conflicting],
            events: vec![event(1)],
            cursor: cursor(100),
        };

        let error = storage
            .commit_batch(&batch, None)
            .await
            .expect_err("immutable conflict should abort batch");
        assert!(matches!(error, StorageError::ImmutableConflict { .. }));
        assert!(
            storage
                .get_raw_object("raw-1")
                .await
                .expect("lookup should work")
                .is_none()
        );
        assert_eq!(storage.event_count().await.expect("count works"), 0);
        assert!(
            storage
                .get_cursor("codex-rollout")
                .await
                .expect("cursor lookup works")
                .is_none()
        );
    }

    #[tokio::test]
    async fn successful_batch_commits_event_and_cursor_together() {
        let (_directory, storage) = test_storage().await;
        let batch = IngestionBatch {
            raw_objects: vec![raw("raw-1", b"line")],
            events: vec![event(1)],
            cursor: IngestionCursor {
                partial_line: b"partial".to_vec(),
                ..cursor(100)
            },
        };

        storage
            .commit_batch(&batch, None)
            .await
            .expect("valid batch should commit");

        assert_eq!(storage.event_count().await.expect("count works"), 1);
        assert_eq!(
            storage
                .get_cursor("codex-rollout")
                .await
                .expect("cursor lookup works"),
            Some(batch.cursor)
        );
    }

    #[tokio::test]
    async fn commit_batch_attributes_new_sessions_to_the_origin_collector() {
        let (_directory, storage) = test_storage().await;
        let batch = IngestionBatch {
            raw_objects: vec![raw("raw-1", b"line")],
            events: vec![event(1)],
            cursor: cursor(100),
        };

        storage
            .commit_batch(&batch, Some("collector_abc"))
            .await
            .expect("valid batch should commit");

        let session = storage
            .get_native_session(&batch.events[0].native_session_id)
            .await
            .expect("lookup should work")
            .expect("session should exist");
        assert_eq!(
            session.origin_collector_id.as_deref(),
            Some("collector_abc")
        );
    }

    #[tokio::test]
    async fn collector_token_issuance_authentication_and_revocation() {
        let (_directory, storage) = test_storage().await;
        let (collector, token) = storage
            .issue_collector("dedicated-host-1", "2026-07-20T14:30:00Z")
            .await
            .expect("collector should be issued");
        assert_eq!(token.len(), 64);
        assert!(
            token
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );

        let authenticated = storage
            .authenticate_collector(&token)
            .await
            .expect("authentication should not fail")
            .expect("token should resolve to the issued collector");
        assert_eq!(authenticated.id, collector.id);
        assert!(authenticated.revoked_at.is_none());

        assert!(
            storage
                .authenticate_collector(
                    "0000000000000000000000000000000000000000000000000000000000000000"
                )
                .await
                .expect("authentication should not fail")
                .is_none(),
            "an unknown token must never authenticate"
        );

        let revoked = storage
            .revoke_collector(&collector.id, "2026-07-20T15:00:00Z")
            .await
            .expect("revocation should not fail");
        assert!(revoked);
        assert!(
            !storage
                .revoke_collector(&collector.id, "2026-07-20T15:01:00Z")
                .await
                .expect("revocation should not fail"),
            "revoking an already-revoked collector reports no change"
        );

        assert!(
            storage
                .authenticate_collector(&token)
                .await
                .expect("authentication should not fail")
                .is_none(),
            "a revoked token must not authenticate"
        );

        let collectors = storage
            .list_collectors()
            .await
            .expect("listing should not fail");
        assert_eq!(collectors.len(), 1);
        assert!(collectors[0].revoked_at.is_some());
    }

    #[tokio::test]
    async fn ingestion_audit_log_records_accepted_and_rejected_attempts() {
        let (_directory, storage) = test_storage().await;
        let (collector, _token) = storage
            .issue_collector("dedicated-host-1", "2026-07-20T14:30:00Z")
            .await
            .expect("collector should be issued");

        storage
            .record_ingestion_audit(&IngestionAuditEntry {
                collector_id: Some(collector.id.clone()),
                occurred_at: "2026-07-20T14:31:00Z".to_owned(),
                outcome: IngestionAuditOutcome::Accepted,
                event_count: 3,
                byte_size: 512,
                reason: None,
            })
            .await
            .expect("accepted audit entry should persist");
        storage
            .record_ingestion_audit(&IngestionAuditEntry {
                collector_id: None,
                occurred_at: "2026-07-20T14:32:00Z".to_owned(),
                outcome: IngestionAuditOutcome::Rejected,
                event_count: 0,
                byte_size: 4096,
                reason: Some("payload exceeds max batch bytes".to_owned()),
            })
            .await
            .expect("rejected audit entry should persist");

        let rows: Vec<(Option<String>, String, i64)> = sqlx::query_as(
            "SELECT collector_id, outcome, event_count FROM ingestion_audit_log ORDER BY id",
        )
        .fetch_all(storage.pool())
        .await
        .expect("audit rows should be queryable");
        assert_eq!(
            rows,
            vec![
                (Some(collector.id), "accepted".to_owned(), 3),
                (None, "rejected".to_owned(), 0),
            ]
        );
    }

    #[tokio::test]
    async fn membership_is_reference_only_reversible_and_audited() {
        let (_directory, storage) = test_storage().await;
        storage
            .store_event(&event(0))
            .await
            .expect("native session should exist");
        storage
            .create_global_session(&StoredGlobalSession {
                id: "gs_test".to_owned(),
                objective: "Test global membership".to_owned(),
                created_at: "2026-07-20T14:30:00Z".to_owned(),
                updated_at: "2026-07-20T14:30:00Z".to_owned(),
            })
            .await
            .unwrap();
        storage
            .link_session(
                &MembershipDecision {
                    global_session_id: "gs_test",
                    native_session_id: "native-session",
                    actor: "local-user",
                    reason: None,
                    created_at: "2026-07-20T14:31:00Z",
                },
                0.9,
                "deterministic-v1",
            )
            .await
            .unwrap();
        let members = storage.list_session_members("gs_test").await.unwrap();
        assert_eq!(members.len(), 1);

        storage
            .reject_membership(&MembershipDecision {
                global_session_id: "gs_test",
                native_session_id: "native-session",
                actor: "local-user",
                reason: Some("different task"),
                created_at: "2026-07-20T14:32:00Z",
            })
            .await
            .unwrap();
        assert!(
            storage
                .link_session(
                    &MembershipDecision {
                        global_session_id: "gs_test",
                        native_session_id: "native-session",
                        actor: "local-user",
                        reason: None,
                        created_at: "2026-07-20T14:33:00Z",
                    },
                    1.0,
                    "manual-v1",
                )
                .await
                .is_err()
        );
        storage
            .reverse_rejection(
                "gs_test",
                "native-session",
                "local-user",
                "2026-07-20T14:34:00Z",
            )
            .await
            .unwrap();
        storage
            .link_session(
                &MembershipDecision {
                    global_session_id: "gs_test",
                    native_session_id: "native-session",
                    actor: "local-user",
                    reason: None,
                    created_at: "2026-07-20T14:35:00Z",
                },
                1.0,
                "manual-v1",
            )
            .await
            .unwrap();

        let audit = storage.membership_audit("gs_test").await.unwrap();
        assert_eq!(
            audit
                .iter()
                .map(|entry| entry.action.as_str())
                .collect::<Vec<_>>(),
            vec!["link", "reject", "reverse_rejection", "link"]
        );
        let membership_columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('global_session_members')")
                .fetch_all(storage.pool())
                .await
                .unwrap();
        assert!(
            !membership_columns
                .iter()
                .any(|column| column == "transcript")
        );
        assert!(!membership_columns.iter().any(|column| column == "payload"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn concurrent_reader_observes_only_committed_state() {
        let (_directory, storage) = test_storage().await;
        let writer = storage.clone();
        let reader = storage.clone();
        let event = event(1);

        let write = tokio::spawn(async move { writer.store_event(&event).await });
        let read = tokio::spawn(async move { reader.event_count().await });
        let inserted = write
            .await
            .expect("writer should join")
            .expect("write works");
        let observed = read.await.expect("reader should join").expect("read works");

        assert!(inserted);
        assert!(observed <= 1);
        assert_eq!(storage.event_count().await.expect("final count works"), 1);
    }

    #[tokio::test]
    async fn correlation_candidates_preserve_evidence_and_rejections() {
        let (_directory, storage) = test_storage().await;
        for (id, family) in [("agy:left", "agy"), ("opencode:right", "opencode")] {
            sqlx::query(
                "INSERT INTO native_sessions (id, tool_family, surface, profile)
                 VALUES (?, ?, 'cli', 'test')",
            )
            .bind(id)
            .bind(family)
            .execute(storage.pool())
            .await
            .expect("candidate fixture session should persist");
        }
        let mut candidate = StoredCorrelationCandidate {
            id: "candidate_test".to_owned(),
            left_native_session_id: "agy:left".to_owned(),
            right_native_session_id: "opencode:right".to_owned(),
            score: 0.72,
            status: "pending".to_owned(),
            evidence: vec![r#"{"signal":"content_jaccard","similarity":0.7}"#.to_owned()],
        };
        storage
            .upsert_correlation_candidate(&candidate)
            .await
            .expect("candidate should persist");
        storage
            .set_correlation_candidate_status(&candidate.id, "rejected")
            .await
            .expect("review should persist");

        candidate.score = 0.91;
        storage
            .upsert_correlation_candidate(&candidate)
            .await
            .expect("rescoring should update evidence without reversing review");
        let stored = storage.list_correlation_candidates().await.unwrap();

        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].status, "rejected");
        assert!((stored[0].score - 0.91).abs() < f64::EPSILON);
        assert_eq!(stored[0].evidence, candidate.evidence);
    }

    #[tokio::test]
    async fn reopening_preserves_data_and_database_settings() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let database_path = directory.path().join("sessionmesh.db");
        let blob_path = directory.path().join("blobs");
        let storage = Storage::open(&database_path, &blob_path)
            .await
            .expect("storage should open");
        storage
            .store_event(&event(1))
            .await
            .expect("event should store");
        storage.pool().close().await;

        let reopened = Storage::open(&database_path, &blob_path)
            .await
            .expect("storage should reopen");
        let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(reopened.pool())
            .await
            .expect("journal mode should be queryable");
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(reopened.pool())
            .await
            .expect("foreign key mode should be queryable");

        assert_eq!(reopened.event_count().await.expect("count works"), 1);
        assert_eq!(journal_mode.to_lowercase(), "wal");
        assert_eq!(foreign_keys, 1);
    }
}
