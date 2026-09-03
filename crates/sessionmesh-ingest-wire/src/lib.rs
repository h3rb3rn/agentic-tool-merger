//! Wire contract shared between the `SessionMesh` daemon's network
//! ingestion API and remote collector processes.
//!
//! Deliberately minimal: only mechanical JSON-to-domain-type conversion
//! lives here. Security-sensitive verification (event ID recomputation, raw
//! object identity checks, authentication, rate limiting) stays in the
//! daemon's API layer, which is the only side that must not trust its
//! input. This crate has no opinion on trust and is safe for both the
//! server and an untrusted collector to depend on.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use sessionmesh_core::event::{CanonicalEvent, EventError};
use sessionmesh_storage::{IngestionBatch, IngestionCursor, RawObject};

/// One collector-reported batch: immutable raw objects, canonical events as
/// compact JSON, and the resulting cursor.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IngestBatchRequest {
    /// Collector-local source identity, before server-side scoping.
    pub source_id: String,
    /// Immutable raw objects referenced by `events`.
    pub raw_objects: Vec<RawObjectWire>,
    /// Each entry is one canonical event as compact JSON
    /// (`CanonicalEvent::to_json`). The receiving daemon re-validates every
    /// entry with `CanonicalEvent::from_json`, which recomputes and checks
    /// the event ID from content.
    pub events: Vec<String>,
    /// Cursor to commit once every event above is accepted.
    pub cursor: IngestCursorWire,
}

impl IngestBatchRequest {
    /// Builds a wire batch from a prepared local batch, ready to POST.
    ///
    /// # Errors
    ///
    /// Returns an error if an event fails to serialize (not expected for an
    /// already-constructed [`CanonicalEvent`]).
    pub fn from_batch(batch: &IngestionBatch) -> Result<Self, EventError> {
        Ok(Self {
            source_id: batch.cursor.source_id.clone(),
            raw_objects: batch
                .raw_objects
                .iter()
                .cloned()
                .map(RawObjectWire::from)
                .collect(),
            events: batch
                .events
                .iter()
                .map(CanonicalEvent::to_json)
                .collect::<Result<_, _>>()?,
            cursor: IngestCursorWire::from(&batch.cursor),
        })
    }
}

/// Wire form of one immutable [`RawObject`]. Bytes are base64-encoded for
/// safe JSON transport.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RawObjectWire {
    /// Content-derived identity; the daemon re-verifies this from `bytes`.
    pub id: String,
    /// Base64-encoded native bytes.
    pub bytes_base64: String,
    /// Collector-readable source path.
    pub source_path: String,
    /// Original host path shown in provenance.
    pub original_path: Option<String>,
    /// Source-local byte offset.
    pub source_offset: u64,
    /// Observed complete source size.
    pub source_size: u64,
    /// Native modification timestamp, when available.
    pub source_modified_at: Option<String>,
    /// Native permission bits, when available.
    pub source_permissions: Option<u32>,
    /// Stable source generation.
    pub source_generation: String,
    /// Parser version used for this import.
    pub parser_version: String,
    /// Import timestamp.
    pub imported_at: String,
}

impl From<RawObject> for RawObjectWire {
    fn from(raw: RawObject) -> Self {
        Self {
            id: raw.id,
            bytes_base64: BASE64.encode(&raw.bytes),
            source_path: raw.source_path,
            original_path: raw.original_path,
            source_offset: raw.source_offset,
            source_size: raw.source_size,
            source_modified_at: raw.source_modified_at,
            source_permissions: raw.source_permissions,
            source_generation: raw.source_generation,
            parser_version: raw.parser_version,
            imported_at: raw.imported_at,
        }
    }
}

impl RawObjectWire {
    /// Decodes this wire object back into a [`RawObject`].
    ///
    /// # Errors
    ///
    /// Returns an error if `bytes_base64` is not valid base64. Callers on
    /// the trust boundary (the daemon's API layer) must additionally
    /// re-verify `id` against the decoded content; this conversion alone
    /// does not authenticate anything.
    pub fn into_raw_object(self) -> Result<RawObject, base64::DecodeError> {
        Ok(RawObject {
            id: self.id,
            bytes: BASE64.decode(self.bytes_base64)?,
            source_path: self.source_path,
            original_path: self.original_path,
            source_offset: self.source_offset,
            source_size: self.source_size,
            source_modified_at: self.source_modified_at,
            source_permissions: self.source_permissions,
            source_generation: self.source_generation,
            parser_version: self.parser_version,
            imported_at: self.imported_at,
        })
    }
}

/// Wire form of an [`IngestionCursor`], without its `source_id` — the
/// server always derives the committed cursor's identity from the
/// authenticated collector plus [`IngestBatchRequest::source_id`], never
/// from client-supplied cursor content.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IngestCursorWire {
    /// Source generation associated with this offset.
    pub source_generation: String,
    /// Last committed byte boundary.
    pub byte_offset: u64,
    /// Native sequence assigned to the next complete physical record.
    pub next_sequence: u64,
    /// Base64-encoded incomplete final bytes retained until a complete
    /// record arrives.
    pub partial_line_base64: String,
    /// Cursor update timestamp.
    pub updated_at: String,
}

impl From<&IngestionCursor> for IngestCursorWire {
    fn from(cursor: &IngestionCursor) -> Self {
        Self {
            source_generation: cursor.source_generation.clone(),
            byte_offset: cursor.byte_offset,
            next_sequence: cursor.next_sequence,
            partial_line_base64: BASE64.encode(&cursor.partial_line),
            updated_at: cursor.updated_at.clone(),
        }
    }
}

impl IngestCursorWire {
    /// Decodes this wire cursor back into an [`IngestionCursor`], scoped
    /// under the given `source_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if `partial_line_base64` is not valid base64.
    pub fn into_cursor(self, source_id: String) -> Result<IngestionCursor, base64::DecodeError> {
        Ok(IngestionCursor {
            source_id,
            source_generation: self.source_generation,
            byte_offset: self.byte_offset,
            next_sequence: self.next_sequence,
            partial_line: BASE64.decode(self.partial_line_base64)?,
            updated_at: self.updated_at,
        })
    }
}

/// Response to a successfully committed [`IngestBatchRequest`].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IngestBatchResponse {
    /// Canonical events committed from this batch.
    pub accepted_events: usize,
    /// Raw objects committed from this batch.
    pub accepted_raw_objects: usize,
}

/// Response to a cursor lookup. `cursor` is `None` when the collector has
/// never successfully committed a batch for the requested source.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct IngestCursorResponse {
    /// The collector's last committed cursor for this source, if any.
    pub cursor: Option<IngestCursorWire>,
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sessionmesh_core::event::{
        EventDraft, EventProvenance, EventTimestamp, TimestampPrecision, ToolIdentity,
    };

    use super::*;

    fn sample_batch() -> IngestionBatch {
        let event = CanonicalEvent::from_draft(EventDraft {
            tool: ToolIdentity {
                family: "codex".to_owned(),
                surface: "cli".to_owned(),
                profile: "default".to_owned(),
            },
            native_session_id: "remote-session".to_owned(),
            sequence: 0,
            timestamp: EventTimestamp::parse("2026-07-20T14:30:00Z").unwrap(),
            timestamp_precision: TimestampPrecision::Second,
            kind: sessionmesh_core::event::EventKind::SessionStart,
            workspace: None,
            payload: BTreeMap::new(),
            provenance: EventProvenance {
                source_path: "/collector/rollout.jsonl".to_owned(),
                original_path: Some("~/.codex/rollout.jsonl".to_owned()),
                source_offset: 0,
                source_generation: "generation-1".to_owned(),
                adapter_version: "0.1.0".to_owned(),
                ingestion_sequence: 0,
                ordering_confidence: Some(1.0),
            },
        })
        .unwrap();
        IngestionBatch {
            raw_objects: vec![RawObject {
                id: "sha256:deadbeef".to_owned(),
                bytes: b"raw line content".to_vec(),
                source_path: "/collector/rollout.jsonl".to_owned(),
                original_path: Some("~/.codex/rollout.jsonl".to_owned()),
                source_offset: 0,
                source_size: 17,
                source_modified_at: None,
                source_permissions: None,
                source_generation: "generation-1".to_owned(),
                parser_version: "0.1.0".to_owned(),
                imported_at: "2026-07-20T14:30:00Z".to_owned(),
            }],
            events: vec![event],
            cursor: IngestionCursor {
                source_id: "codex:remote-rollout".to_owned(),
                source_generation: "generation-1".to_owned(),
                byte_offset: 17,
                next_sequence: 1,
                partial_line: b"partial".to_vec(),
                updated_at: "2026-07-20T14:30:05Z".to_owned(),
            },
        }
    }

    #[test]
    fn batch_survives_a_json_wire_round_trip() {
        let batch = sample_batch();
        let request = IngestBatchRequest::from_batch(&batch).expect("batch should serialize");
        let wire_json = serde_json::to_string(&request).expect("request should serialize");
        let decoded: IngestBatchRequest =
            serde_json::from_str(&wire_json).expect("request should deserialize");

        assert_eq!(decoded.source_id, batch.cursor.source_id);
        assert_eq!(decoded.raw_objects.len(), 1);
        assert_eq!(decoded.events.len(), 1);

        let raw = decoded.raw_objects.into_iter().next().unwrap();
        let decoded_raw = raw.clone().into_raw_object().expect("bytes should decode");
        assert_eq!(decoded_raw, batch.raw_objects[0]);

        let decoded_cursor = decoded
            .cursor
            .into_cursor("collector:c1:codex:remote-rollout".to_owned())
            .expect("partial line should decode");
        assert_eq!(
            decoded_cursor.source_generation,
            batch.cursor.source_generation
        );
        assert_eq!(decoded_cursor.byte_offset, batch.cursor.byte_offset);
        assert_eq!(decoded_cursor.partial_line, batch.cursor.partial_line);
        assert_eq!(
            decoded_cursor.source_id,
            "collector:c1:codex:remote-rollout"
        );

        let recovered_event =
            CanonicalEvent::from_json(&decoded.events[0]).expect("event JSON should validate");
        assert_eq!(recovered_event, batch.events[0]);
    }

    #[test]
    fn cursor_response_round_trips_a_missing_cursor() {
        let response = IngestCursorResponse { cursor: None };
        let json = serde_json::to_string(&response).unwrap();
        let decoded: IngestCursorResponse = serde_json::from_str(&json).unwrap();
        assert!(decoded.cursor.is_none());
    }
}
