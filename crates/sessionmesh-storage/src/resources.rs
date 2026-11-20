//! Bounded read projections and explicit retention of conversation evidence.

use super::{
    CanonicalEvent, Row, Sqlite, Storage, StorageError, StoredGlobalSession, StoredNativeSession,
    Transaction, to_i64, to_u64,
};
use sessionmesh_core::event::EventKind;
use std::{path::Path, time::Duration};

const DISPOSABLE_KINDS: &str =
    "'ToolCall','ToolResult','FileRead','FileWrite','Patch','ShellCommand','CommandResult'";

/// Payload-free event metadata used by timeline pages and SSE replay.
#[derive(Clone, Debug)]
pub struct EventOverview {
    /// Stable original identity.
    pub event_id: String,
    /// Native session identity.
    pub native_session_id: String,
    /// Original native sequence, never renumbered after cleanup.
    pub sequence: u64,
    /// Original timestamp text.
    pub timestamp: String,
    /// Canonical wire kind.
    pub kind: String,
    /// Observed workspace.
    pub cwd: Option<String>,
    /// Observed branch.
    pub branch: Option<String>,
}

/// Aggregate session information without materializing transcripts.
#[derive(Debug)]
pub struct SessionStatistics {
    /// Bounded title candidate.
    pub title: Option<String>,
    /// Number of retained events.
    pub event_count: usize,
    /// UTF-8 canonical bytes retained.
    pub content_bytes: usize,
}

/// Counts from one successful cleanup run; messages are never eligible.
#[derive(Debug, Default)]
pub struct MaintenanceReport {
    /// Separate tool/code events removed.
    pub events_removed: u64,
    /// Superseded handoffs removed.
    pub handoffs_removed: u64,
    /// Unreferenced blob files removed.
    pub blobs_removed: u64,
    /// Physical blob bytes reclaimed (database pages are reported separately).
    pub blob_bytes_removed: u64,
}

/// Identifies separate tool/code events; conversation and unknown kinds are protected.
#[must_use]
pub fn is_disposable_event(kind: EventKind) -> bool {
    matches!(
        kind,
        EventKind::ToolCall
            | EventKind::ToolResult
            | EventKind::FileRead
            | EventKind::FileWrite
            | EventKind::Patch
            | EventKind::ShellCommand
            | EventKind::CommandResult
    )
}

/// Reads the shared policy inside the same writer transaction as ingestion.
pub(super) async fn conversation_only(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<bool, StorageError> {
    let value: i64 =
        sqlx::query_scalar("SELECT value FROM storage_settings WHERE key = 'conversation_only'")
            .fetch_one(&mut **transaction)
            .await?;
    Ok(value != 0)
}

/// Protects legacy cleanup from replays produced by an older native parser.
pub(super) async fn legacy_discarded(
    transaction: &mut Transaction<'_, Sqlite>,
    id: &str,
) -> Result<bool, StorageError> {
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM legacy_discarded_events WHERE event_id=?")
            .bind(id)
            .fetch_one(&mut **transaction)
            .await?;
    Ok(count != 0)
}

/// Indexes content once, excluding duplicated canonical metadata fields.
pub(super) fn search_text(event: &CanonicalEvent) -> String {
    [
        "text",
        "title",
        "description",
        "task",
        "command",
        "stdout",
        "output",
        "content",
        "message",
    ]
    .iter()
    .filter_map(|key| event.payload.get(*key)?.as_str())
    .collect::<Vec<_>>()
    .join("\n")
}

/// Atomically maintains exact ordering and incremental session aggregates.
pub(super) async fn insert_overview(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &CanonicalEvent,
    canonical_bytes: usize,
) -> Result<(), StorageError> {
    let timestamp = chrono::DateTime::parse_from_rfc3339(event.timestamp.as_str())
        .map_err(|error| StorageError::Serialization(error.to_string()))?;
    let kind = serde_json::to_value(event.kind)
        .map_err(|error| StorageError::Serialization(error.to_string()))?;
    sqlx::query(
        "INSERT OR IGNORE INTO event_overviews VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(event.event_id.as_str())
    .bind(&event.native_session_id)
    .bind(to_i64(event.sequence, "sequence")?)
    .bind(event.timestamp.as_str())
    .bind(timestamp.timestamp())
    .bind(i64::from(timestamp.timestamp_subsec_nanos()))
    .bind(kind.as_str())
    .bind(
        event
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.cwd.as_deref()),
    )
    .bind(
        event
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.branch.as_deref()),
    )
    .bind(&event.provenance.source_generation)
    .bind(to_i64(event.provenance.source_offset, "offset")?)
    .bind(to_i64(
        event.provenance.ingestion_sequence,
        "ingestion_sequence",
    )?)
    .execute(&mut **transaction)
    .await?;
    let explicit_title = ["thread_title", "title", "name"]
        .iter()
        .find_map(|key| event.payload.get(*key)?.as_str());
    let candidate = if is_disposable_event(event.kind) {
        None
    } else {
        explicit_title.or_else(|| {
            (event.kind == EventKind::UserMessage)
                .then(|| event.payload.get("text")?.as_str())
                .flatten()
        })
    };
    let title = candidate
        .filter(|text| !text.trim().is_empty())
        .map(|text| text.chars().take(320).collect::<String>());
    let priority = if title.is_none() {
        2_i64
    } else if explicit_title.is_some() {
        0_i64
    } else if title.is_some() {
        1_i64
    } else {
        2_i64
    };
    sqlx::query("INSERT INTO session_statistics VALUES (?,1,?,?,?,?,?)
            ON CONFLICT(native_session_id)
            DO UPDATE SET event_count=event_count+1,content_bytes=content_bytes+excluded.content_bytes,
        title=CASE
            WHEN (excluded.title_priority,excluded.title_sequence,excluded.title_event_id)<(title_priority,title_sequence,title_event_id)
            THEN excluded.title
            ELSE title END,
        title_priority=MIN(title_priority,excluded.title_priority),
        title_sequence=CASE
            WHEN excluded.title_priority<title_priority OR (excluded.title_priority=title_priority
            AND (excluded.title_sequence,excluded.title_event_id)<(title_sequence,title_event_id))
            THEN excluded.title_sequence
            ELSE title_sequence END,
        title_event_id=CASE
            WHEN excluded.title_priority<title_priority OR (excluded.title_priority=title_priority
            AND (excluded.title_sequence,excluded.title_event_id)<(title_sequence,title_event_id))
            THEN excluded.title_event_id
            ELSE title_event_id END")
        .bind(&event.native_session_id).bind(i64::try_from(canonical_bytes).unwrap_or(i64::MAX)).bind(title).bind(priority).bind(to_i64(event.sequence,"sequence")?).bind(event.event_id.as_str()).execute(&mut **transaction).await?;
    Ok(())
}

impl Storage {
    /// Persists the retention policy so all writers, including MCP, honor it.
    ///
    /// # Errors
    /// Returns a database error. Disabling the policy cannot recover deleted data.
    pub async fn set_conversation_retention(&self, enabled: bool) -> Result<(), StorageError> {
        sqlx::query("UPDATE storage_settings SET value = ? WHERE key = 'conversation_only'")
            .bind(i64::from(enabled))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Backfills exact ordering metadata once, in bounded migration batches.
    pub(super) async fn backfill_overviews(&self) -> Result<(), StorageError> {
        let complete: i64 = sqlx::query_scalar(
            "SELECT value FROM storage_settings WHERE key='overview_backfill_complete'",
        )
        .fetch_one(&self.pool)
        .await?;
        if complete != 0 {
            return Ok(());
        }
        let mut after = 0_i64;
        loop {
            let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            let rows = sqlx::query(
                "SELECT n.rowid AS position, n.event_id
            FROM native_events n
            WHERE n.rowid > ?
            AND NOT EXISTS (SELECT 1
            FROM event_overviews o
            WHERE o.event_id = n.event_id)
            ORDER BY n.rowid
            LIMIT 128",
            )
            .bind(after)
            .fetch_all(&mut *transaction)
            .await?;
            if rows.is_empty() {
                sqlx::query(
                    "UPDATE storage_settings SET value=1 WHERE key='overview_backfill_complete'",
                )
                .execute(&mut *transaction)
                .await?;
                transaction.commit().await?;
                break;
            }
            for row in rows {
                after = row.get("position");
                let id: String = row.get("event_id");
                let json: String =
                    sqlx::query_scalar("SELECT canonical_json FROM native_events WHERE event_id=?")
                        .bind(id)
                        .fetch_one(&mut *transaction)
                        .await?;
                let event = CanonicalEvent::from_json(&json)
                    .map_err(|error| StorageError::Serialization(error.to_string()))?;
                insert_overview(&mut transaction, &event, json.len()).await?;
                // Do not recreate a huge derived tool index just before retention discards it.
                if !is_disposable_event(event.kind) || !conversation_only(&mut transaction).await? {
                    sqlx::query("INSERT OR IGNORE INTO native_events_fts (rowid,event_id,searchable_text) VALUES (?, ?, ?)").bind(after).bind(event.event_id.as_str()).bind(search_text(&event)).execute(&mut *transaction).await?;
                }
            }
            transaction.commit().await?;
        }
        Ok(())
    }

    /// Returns the monotonic ingestion watermark without loading changed-session IDs.
    ///
    /// # Errors
    /// Returns a database error.
    pub async fn event_watermark(&self) -> Result<i64, StorageError> {
        Ok(sqlx::query_scalar(
            "SELECT MAX(COALESCE((SELECT MAX(rowid)
            FROM native_events),0),COALESCE((SELECT value
            FROM storage_settings
            WHERE key='event_watermark'),0))",
        )
        .fetch_one(&self.pool)
        .await?)
    }

    /// Finds an event's insertion position for replay/live deduplication.
    ///
    /// # Errors
    /// Returns a database error.
    pub async fn event_rowid(&self, id: &str) -> Result<Option<i64>, StorageError> {
        Ok(
            sqlx::query_scalar("SELECT rowid FROM native_events WHERE event_id=?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Reads a bounded page with indexed, exact chronology and no event payloads.
    /// `None` means the supplied cursor no longer exists. `upper_rowid` bounds SSE replay.
    ///
    /// # Errors
    /// Returns a database or numeric-range error.
    pub async fn event_overviews(
        &self,
        after: Option<&str>,
        limit: usize,
        session: Option<&str>,
        kind: Option<&str>,
        upper_rowid: i64,
    ) -> Result<Option<Vec<EventOverview>>, StorageError> {
        if after.is_some()
            && sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM event_overviews WHERE event_id = ?",
            )
            .bind(after)
            .fetch_one(&self.pool)
            .await?
                == 0
        {
            return Ok(None);
        }
        let mut query = sqlx::QueryBuilder::<Sqlite>::new(
            "SELECT o.event_id, o.native_session_id, o.sequence, o.timestamp, o.kind, o.cwd, o.branch FROM event_overviews o JOIN native_events n ON n.event_id=o.event_id WHERE n.rowid <= ",
        );
        query.push_bind(upper_rowid);
        if let Some(after) = after {
            query.push(" AND (o.timestamp_seconds,o.timestamp_nanos,o.sequence,o.source_generation,o.source_offset,o.ingestion_sequence,o.event_id) > (SELECT timestamp_seconds,timestamp_nanos,sequence,source_generation,source_offset,ingestion_sequence,event_id FROM event_overviews WHERE event_id=").push_bind(after).push(")");
        }
        if let Some(session) = session {
            query.push(" AND o.native_session_id=").push_bind(session);
        }
        if let Some(kind) = kind {
            query.push(" AND o.kind=").push_bind(kind);
        }
        query.push(" ORDER BY o.timestamp_seconds,o.timestamp_nanos,o.sequence,o.source_generation,o.source_offset,o.ingestion_sequence,o.event_id LIMIT ")
            .push_bind(i64::try_from(limit.min(1001)).unwrap_or(1001));
        let rows = query.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(EventOverview {
                    event_id: row.get("event_id"),
                    native_session_id: row.get("native_session_id"),
                    sequence: to_u64(row.get("sequence"), "sequence")?,
                    timestamp: row.get("timestamp"),
                    kind: row.get("kind"),
                    cwd: row.get("cwd"),
                    branch: row.get("branch"),
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()
            .map(Some)
    }

    /// Reads one global context by its primary key, without collecting the global archive.
    ///
    /// # Errors
    /// Returns a database error.
    pub async fn get_global_session(
        &self,
        id: &str,
    ) -> Result<Option<StoredGlobalSession>, StorageError> {
        let row = sqlx::query(
            "SELECT id,objective,created_at,updated_at FROM global_sessions WHERE id=?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| StoredGlobalSession {
            id: row.get("id"),
            objective: row.get("objective"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }))
    }

    /// Reads a bounded identity-ordered native-session page before projecting statistics.
    ///
    /// # Errors
    /// Returns a database error.
    pub async fn native_session_page(
        &self,
        after: Option<&str>,
        limit: usize,
        family: Option<&str>,
    ) -> Result<Vec<StoredNativeSession>, StorageError> {
        let mut query = sqlx::QueryBuilder::<Sqlite>::new(
            "SELECT id,tool_family,surface,profile,started_at,ended_at,origin_collector_id FROM native_sessions WHERE 1=1",
        );
        if let Some(after) = after {
            query.push(" AND id>").push_bind(after);
        }
        if let Some(family) = family {
            query.push(" AND tool_family=").push_bind(family);
        }
        query
            .push(" ORDER BY id LIMIT ")
            .push_bind(i64::try_from(limit.min(1001)).unwrap_or(1001));
        let rows = query.build().fetch_all(&self.pool).await?;
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

    /// Resolves the latest audited workspace membership without opening transcript payloads.
    ///
    /// # Errors
    /// Returns a database error.
    pub async fn global_for_workspace(&self, cwd: &str) -> Result<Option<String>, StorageError> {
        Ok(sqlx::query_scalar("SELECT m.global_session_id
            FROM event_overviews o
            JOIN global_session_members m ON m.native_session_id=o.native_session_id
            WHERE rtrim(o.cwd,'/')=?
            ORDER BY o.timestamp_seconds DESC,o.timestamp_nanos DESC,o.sequence DESC,o.source_generation DESC,
                     o.source_offset DESC,o.ingestion_sequence DESC,o.event_id DESC,m.global_session_id DESC
            LIMIT 1").bind(cwd.trim_end_matches('/')).fetch_optional(&self.pool).await?)
    }

    /// Returns bounded text and SQL aggregates for a session list/detail view.
    ///
    /// # Errors
    /// Returns a database or numeric-range error.
    pub async fn session_statistics(&self, id: &str) -> Result<SessionStatistics, StorageError> {
        let row=sqlx::query("SELECT title,event_count,content_bytes FROM session_statistics WHERE native_session_id=?").bind(id).fetch_optional(&self.pool).await?;
        match row {
            Some(row) => Ok(SessionStatistics {
                title: row.get("title"),
                event_count: usize::try_from(row.get::<i64, _>("event_count")).map_err(|_| {
                    StorageError::NumericRange {
                        field: "event_count",
                    }
                })?,
                content_bytes: usize::try_from(row.get::<i64, _>("content_bytes")).map_err(
                    |_| StorageError::NumericRange {
                        field: "content_bytes",
                    },
                )?,
            }),
            None => Ok(SessionStatistics {
                title: None,
                event_count: 0,
                content_bytes: 0,
            }),
        }
    }

    /// Loads only a recent context window; the durable transcript stays complete.
    ///
    /// # Errors
    /// Returns a database or canonical serialization error.
    pub async fn recent_context(
        &self,
        ids: &[String],
        limit: usize,
    ) -> Result<Vec<CanonicalEvent>, StorageError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        // Fetch bounded IDs/lengths first, so 256 large messages cannot all be materialized.
        let mut metadata = sqlx::QueryBuilder::<Sqlite>::new(
            "SELECT n.event_id, length(CAST(n.canonical_json AS BLOB)) AS bytes FROM native_events n JOIN event_overviews o ON o.event_id=n.event_id WHERE o.native_session_id IN (",
        );
        let mut ids_sql = metadata.separated(",");
        for id in ids {
            ids_sql.push_bind(id);
        }
        ids_sql.push_unseparated(") ORDER BY o.timestamp_seconds DESC,o.timestamp_nanos DESC, o.sequence DESC, o.source_generation DESC, o.source_offset DESC, o.ingestion_sequence DESC, o.event_id DESC LIMIT ");
        metadata.push_bind(i64::try_from(limit.min(512)).unwrap_or(512));
        let rows = metadata.build().fetch_all(&self.pool).await?;
        let mut total_bytes = 0_i64;
        let mut events = Vec::new();
        for row in rows {
            let bytes: i64 = row.get("bytes");
            if !events.is_empty() && total_bytes.saturating_add(bytes) > 4 * 1024 * 1024 {
                break;
            }
            let id: String = row.get("event_id");
            if let Some(event) = self.get_canonical_event(&id).await? {
                events.push(event);
            }
            total_bytes = total_bytes.saturating_add(bytes);
        }
        events.reverse();
        Ok(events)
    }

    /// Returns a content-free storage report for operator inspection.
    ///
    /// # Errors
    /// Returns a database error.
    pub async fn resource_summary(&self) -> Result<String, StorageError> {
        let pages: i64 = sqlx::query_scalar("PRAGMA page_count")
            .fetch_one(&self.pool)
            .await?;
        let free: i64 = sqlx::query_scalar("PRAGMA freelist_count")
            .fetch_one(&self.pool)
            .await?;
        let size: i64 = sqlx::query_scalar("PRAGMA page_size")
            .fetch_one(&self.pool)
            .await?;
        let eligible: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM native_events WHERE kind IN ({DISPOSABLE_KINDS})"
        ))
        .fetch_one(&self.pool)
        .await?;
        let handoffs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM handoffs")
            .fetch_one(&self.pool)
            .await?;
        let messages: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM native_events WHERE kind IN ('UserMessage','AssistantMessage')",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(format!(
            "database_bytes={} reusable_database_bytes={} conversation_events={} tool_code_events={} handoffs={}",
            pages * size,
            free * size,
            messages,
            eligible,
            handoffs
        ))
    }

    /// Rebuilds the database to reclaim legacy free pages and enable incremental vacuum.
    /// Run during a maintenance window with sufficient free space for `SQLite`'s temporary copy.
    ///
    /// # Errors
    /// Returns a database error (including contention or insufficient disk space).
    pub async fn compact_database(&self) -> Result<(), StorageError> {
        let mut connection = self.pool.acquire().await?;
        sqlx::query("PRAGMA auto_vacuum=INCREMENTAL")
            .execute(&mut *connection)
            .await?;
        sqlx::query("VACUUM").execute(&mut *connection).await?;
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&mut *connection)
            .await?;
        Ok(())
    }

    /// Returns the derived-context revision changed by retention cleanup.
    ///
    /// # Errors
    /// Returns a database error.
    pub async fn context_revision(&self) -> Result<i64, StorageError> {
        Ok(sqlx::query_scalar(
            "SELECT COALESCE((SELECT value FROM storage_settings WHERE key='context_revision'),0)",
        )
        .fetch_one(&self.pool)
        .await?)
    }

    /// Invalidates summaries that may cite discarded tool events, preserving all user facts.
    async fn invalidate_context(
        transaction: &mut Transaction<'_, Sqlite>,
    ) -> Result<(), StorageError> {
        sqlx::query("INSERT INTO storage_settings VALUES ('context_revision',1) ON CONFLICT(key) DO UPDATE SET value=value+1").execute(&mut **transaction).await?;
        Ok(())
    }

    /// Reclaims legacy Claude tool-only records previously labeled as empty messages.
    /// Mixed records, reasoning, attachments, and every text block remain protected.
    async fn cleanup_legacy_claude(
        &self,
        report: &mut MaintenanceReport,
    ) -> Result<(), StorageError> {
        let mut after = 0_i64;
        loop {
            let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            if !conversation_only(&mut transaction).await? {
                break;
            }
            let rows = sqlx::query(
                "SELECT n.rowid AS position,n.event_id,n.raw_object_id,r.blob_hash
            FROM native_events n
            JOIN native_sessions s ON s.id=n.native_session_id
            JOIN raw_objects r ON r.id=n.raw_object_id
            WHERE n.rowid>?
            AND s.tool_family='claude'
            AND n.kind IN ('UserMessage','AssistantMessage')
            AND json_extract(n.canonical_json,'$.payload.text')=''
            ORDER BY n.rowid
            LIMIT 32",
            )
            .bind(after)
            .fetch_all(&mut *transaction)
            .await?;
            if rows.is_empty() {
                break;
            }
            let before = report.events_removed;
            for row in rows {
                after = row.get("position");
                let hash: String = row.get("blob_hash");
                let bytes = self.blobs.get(&hash).await?;
                let raw: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|error| StorageError::Serialization(error.to_string()))?;
                let pure_tools = raw
                    .pointer("/message/content")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|parts| {
                        !parts.is_empty()
                            && parts.iter().all(|part| {
                                matches!(
                                    part.get("type").and_then(serde_json::Value::as_str),
                                    Some("tool_use" | "tool_result")
                                )
                            })
                    });
                if pure_tools {
                    sqlx::query("INSERT OR IGNORE INTO legacy_discarded_events VALUES (?)")
                        .bind(row.get::<String, _>("event_id"))
                        .execute(&mut *transaction)
                        .await?;
                    Self::remove_event(
                        &mut transaction,
                        &row.get::<String, _>("event_id"),
                        row.get("raw_object_id"),
                        report,
                    )
                    .await?;
                }
            }
            if before != report.events_removed {
                Self::invalidate_context(&mut transaction).await?;
            }
            transaction.commit().await?;
        }
        Ok(())
    }

    /// Deletes metadata first and enqueues physical deletion for a later committed phase.
    async fn remove_event(
        transaction: &mut Transaction<'_, Sqlite>,
        id: &str,
        raw: Option<String>,
        report: &mut MaintenanceReport,
    ) -> Result<(), StorageError> {
        sqlx::query(
            "INSERT INTO storage_settings VALUES ('event_watermark',(SELECT COALESCE(MAX(rowid),0)
            FROM native_events))
            ON CONFLICT(key)
            DO UPDATE SET value=MAX(value,excluded.value)",
        )
        .execute(&mut **transaction)
        .await?;
        report.handoffs_removed += sqlx::query(
            "DELETE
            FROM handoffs
            WHERE global_session_id IN (SELECT global_session_id
            FROM global_session_members
            WHERE native_session_id=(SELECT native_session_id
            FROM native_events
            WHERE event_id=?))",
        )
        .bind(id)
        .execute(&mut **transaction)
        .await?
        .rows_affected();
        sqlx::query("UPDATE session_statistics SET event_count=event_count-1,content_bytes=content_bytes-(SELECT length(CAST(canonical_json AS BLOB))
            FROM native_events
            WHERE event_id=?)
            WHERE native_session_id=(SELECT native_session_id
            FROM native_events
            WHERE event_id=?)").bind(id).bind(id).execute(&mut **transaction).await?;
        sqlx::query("DELETE FROM native_events_fts WHERE rowid=(SELECT rowid FROM native_events WHERE event_id=?)").bind(id).execute(&mut **transaction).await?;
        sqlx::query("DELETE FROM native_events WHERE event_id=?")
            .bind(id)
            .execute(&mut **transaction)
            .await?;
        if let Some(raw) = raw {
            sqlx::query(
                "INSERT OR IGNORE INTO blob_cleanup_queue SELECT blob_hash
            FROM raw_objects
            WHERE id=?
            AND NOT EXISTS (SELECT 1
            FROM native_events
            WHERE raw_object_id=?)",
            )
            .bind(&raw)
            .bind(&raw)
            .execute(&mut **transaction)
            .await?;
            sqlx::query("DELETE FROM raw_objects WHERE id=? AND NOT EXISTS (SELECT 1 FROM native_events WHERE raw_object_id=?)").bind(&raw).bind(&raw).execute(&mut **transaction).await?;
        }
        report.events_removed += 1;
        Ok(())
    }

    /// Removes disposable tool bodies, obsolete derived snapshots, and orphan blobs.
    /// Work is committed in batches; conversations, typed records and audits survive.
    ///
    /// # Errors
    /// Returns storage/filesystem errors without silently discarding failed cleanup.
    pub async fn maintain(&self, keep_handoffs: usize) -> Result<MaintenanceReport, StorageError> {
        if keep_handoffs == 0 {
            return Err(StorageError::InvalidInput {
                field: "keep_handoffs",
                message: "must be positive",
            });
        }
        let mut report = MaintenanceReport::default();
        loop {
            let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            if !conversation_only(&mut transaction).await? {
                break;
            }
            // Preserve the insertion watermark even if the highest-rowid events are removed.
            sqlx::query("INSERT INTO storage_settings VALUES ('event_watermark', (SELECT COALESCE(MAX(rowid),0)
            FROM native_events))
            ON CONFLICT(key)
            DO UPDATE SET value=MAX(value,excluded.value)")
                .execute(&mut *transaction).await?;
            let query = format!(
                "SELECT event_id, raw_object_id FROM native_events WHERE kind IN ({DISPOSABLE_KINDS}) LIMIT 256"
            );
            let rows = sqlx::query(&query).fetch_all(&mut *transaction).await?;
            if rows.is_empty() {
                transaction.commit().await?;
                break;
            }
            for row in rows {
                let id: String = row.get("event_id");
                let raw: Option<String> = row.get("raw_object_id");
                Self::remove_event(&mut transaction, &id, raw, &mut report).await?;
            }
            Self::invalidate_context(&mut transaction).await?;
            transaction.commit().await?;
            tokio::task::yield_now().await;
        }
        self.cleanup_legacy_claude(&mut report).await?;
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        report.handoffs_removed += sqlx::query(
            "DELETE
            FROM handoffs
            WHERE id IN (SELECT id
            FROM (SELECT id, ROW_NUMBER() OVER (PARTITION BY global_session_id
            ORDER BY created_at DESC,id DESC) AS rank
            FROM handoffs)
            WHERE rank > ?)",
        )
        .bind(i64::try_from(keep_handoffs).unwrap_or(i64::MAX))
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        sqlx::query("DELETE FROM state_snapshots WHERE NOT EXISTS (SELECT 1 FROM handoffs WHERE snapshot_id=state_snapshots.id)").execute(&mut *transaction).await?;
        transaction.commit().await?;
        self.cleanup_blobs(&mut report).await?;
        sqlx::query("PRAGMA incremental_vacuum(2048)")
            .execute(&self.pool)
            .await?;
        sqlx::query("PRAGMA wal_checkpoint(PASSIVE)")
            .execute(&self.pool)
            .await?;
        Ok(report)
    }

    /// Garbage collection holds the same `SQLite` writer lock as blob publishers.
    /// Metadata deletion commits before unlink, so crashes leave only reclaimable orphans.
    async fn cleanup_blobs(&self, report: &mut MaintenanceReport) -> Result<(), StorageError> {
        loop {
            let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            let hashes: Vec<String> =
                sqlx::query_scalar("SELECT blob_hash FROM blob_cleanup_queue LIMIT 128")
                    .fetch_all(&mut *transaction)
                    .await?;
            if hashes.is_empty() {
                break;
            }
            for hash in hashes {
                self.unlink_unreferenced(&mut transaction, &hash, report)
                    .await?;
                sqlx::query("DELETE FROM blob_cleanup_queue WHERE blob_hash=?")
                    .bind(hash)
                    .execute(&mut *transaction)
                    .await?;
            }
            transaction.commit().await?;
        }
        // Also collect failed-publication orphans, streaming directory entries rather than collecting paths.
        let mut entries = tokio::fs::read_dir(&self.blobs.root).await?;
        while let Some(entry) = entries.next_entry().await? {
            let metadata = entry.metadata().await?;
            if !metadata.is_file()
                || metadata.modified()?.elapsed().unwrap_or_default() < Duration::from_secs(86400)
            {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !(name.starts_with("sha256:")
                || name.starts_with(".sha256:")
                    && Path::new(&name)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("tmp")))
            {
                continue;
            }
            let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            self.unlink_unreferenced(&mut transaction, &name, report)
                .await?;
            transaction.commit().await?;
        }
        Ok(())
    }

    async fn unlink_unreferenced(
        &self,
        transaction: &mut Transaction<'_, Sqlite>,
        hash: &str,
        report: &mut MaintenanceReport,
    ) -> Result<(), StorageError> {
        if Path::new(hash).file_name().and_then(|name| name.to_str()) != Some(hash) {
            return Err(StorageError::InvalidInput {
                field: "blob_hash",
                message: "must be a file name inside the blob root",
            });
        }
        let references: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM raw_objects WHERE blob_hash=?")
                .bind(hash)
                .fetch_one(&mut **transaction)
                .await?;
        if references != 0 {
            return Ok(());
        }
        let path = self.blobs.root.join(hash);
        match tokio::fs::metadata(&path).await {
            Ok(metadata) => {
                tokio::fs::remove_file(path).await?;
                report.blobs_removed += 1;
                report.blob_bytes_removed += metadata.len();
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
}
