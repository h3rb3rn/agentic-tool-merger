//! Versioned canonical events with deterministic identity and complete provenance.

use std::{
    collections::BTreeMap,
    error::Error,
    fmt::{self, Display, Formatter, Write},
};

use chrono::DateTime;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Current canonical-event wire version.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SchemaVersion {
    /// Initial canonical event contract.
    #[serde(rename = "1.0")]
    V1_0,
}

/// Deterministic SHA-256 identifier for one normalized event.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EventId(String);

impl EventId {
    /// Returns the wire representation, including its `sha256:` prefix.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn from_digest(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        let mut value = String::with_capacity(71);
        value.push_str("sha256:");
        for byte in digest {
            write!(&mut value, "{byte:02x}").expect("writing to a String cannot fail");
        }
        Self(value)
    }

    fn parse(value: String) -> Result<Self, &'static str> {
        let Some(hex) = value.strip_prefix("sha256:") else {
            return Err("event ID must start with sha256:");
        };
        if hex.len() != 64
            || !hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err("event ID must contain 64 lowercase hexadecimal characters");
        }
        Ok(Self(value))
    }
}

impl<'de> Deserialize<'de> for EventId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(de::Error::custom)
    }
}

/// Original RFC 3339 timestamp retained exactly as emitted by the source.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct EventTimestamp(String);

impl EventTimestamp {
    /// Parses and preserves an RFC 3339 timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`EventError::InvalidTimestamp`] when the value is not an RFC
    /// 3339 date-time with an explicit offset.
    pub fn parse(value: impl Into<String>) -> Result<Self, EventError> {
        let value = value.into();
        DateTime::parse_from_rfc3339(&value)
            .map_err(|error| EventError::InvalidTimestamp(error.to_string()))?;
        Ok(Self(value))
    }

    /// Returns the timestamp exactly as supplied by the native source.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for EventTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(de::Error::custom)
    }
}

/// Precision claimed or inferred for a native timestamp.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TimestampPrecision {
    /// The source does not expose reliable precision.
    Unknown,
    /// Whole-second precision.
    Second,
    /// Millisecond precision.
    Millisecond,
    /// Microsecond precision.
    Microsecond,
    /// Nanosecond precision.
    Nanosecond,
}

/// Canonical event categories shared by all adapters.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// A message authored by the user.
    UserMessage,
    /// A message authored by the assistant.
    AssistantMessage,
    /// A system-level message retained as data, never promoted automatically.
    SystemMessage,
    /// A tool invocation request.
    ToolCall,
    /// A tool invocation response.
    ToolResult,
    /// A file-read observation.
    FileRead,
    /// A file-write observation.
    FileWrite,
    /// A source-code patch.
    Patch,
    /// A shell command request.
    ShellCommand,
    /// A shell command result.
    CommandResult,
    /// Repository or worktree state.
    GitState,
    /// A plan produced or updated during the session.
    Plan,
    /// A tracked unit of work.
    Task,
    /// An explicit decision.
    Decision,
    /// A recoverable native checkpoint.
    Checkpoint,
    /// A context-compaction event.
    Compaction,
    /// An error or diagnostic event.
    Error,
    /// A subagent creation event.
    SubagentSpawn,
    /// Native session start.
    SessionStart,
    /// Native session end.
    SessionEnd,
    /// An unsupported native event retained without information loss.
    Unknown,
}

/// Tool family, surface, and profile that produced an event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolIdentity {
    /// Stable tool-family identifier, such as `codex`.
    pub family: String,
    /// User-facing integration surface, such as `cli`.
    pub surface: String,
    /// Profile used to discover and parse the source.
    pub profile: String,
}

/// Optional repository context observed with an event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceContext {
    /// Native working directory.
    pub cwd: Option<String>,
    /// `SessionMesh` repository identity.
    pub repository_id: Option<String>,
    /// Observed branch name.
    pub branch: Option<String>,
    /// Observed Git commit.
    pub head: Option<String>,
}

/// Source coordinates and ordering evidence retained for a normalized event.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventProvenance {
    /// Path used by the collector to read the source.
    pub source_path: String,
    /// Optional original host path shown to the user.
    pub original_path: Option<String>,
    /// Byte offset or equivalent source-local record position.
    pub source_offset: u64,
    /// Stable identity for one source-file generation.
    pub source_generation: String,
    /// Adapter version that produced this interpretation.
    pub adapter_version: String,
    /// Durable order in which `SessionMesh` committed this event.
    pub ingestion_sequence: u64,
    /// Confidence in inferred cross-source ordering.
    pub ordering_confidence: Option<f64>,
}

/// Input needed to construct a canonical event before its ID is known.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventDraft {
    /// Tool that produced the event.
    pub tool: ToolIdentity,
    /// Native session identifier.
    pub native_session_id: String,
    /// Native source-local sequence.
    pub sequence: u64,
    /// Original event timestamp.
    pub timestamp: EventTimestamp,
    /// Original timestamp precision.
    pub timestamp_precision: TimestampPrecision,
    /// Canonical category.
    pub kind: EventKind,
    /// Optional repository context.
    pub workspace: Option<WorkspaceContext>,
    /// Kind-specific payload with lexically ordered object keys.
    pub payload: BTreeMap<String, Value>,
    /// Complete native-source provenance.
    pub provenance: EventProvenance,
}

/// One validated, versioned, and deterministically identified canonical event.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalEvent {
    /// Wire-schema version.
    pub schema_version: SchemaVersion,
    /// Deterministic normalized-event identity.
    pub event_id: EventId,
    /// Tool that produced the event.
    pub tool: ToolIdentity,
    /// Native session identifier.
    pub native_session_id: String,
    /// Native source-local sequence.
    pub sequence: u64,
    /// Original event timestamp.
    pub timestamp: EventTimestamp,
    /// Original timestamp precision.
    pub timestamp_precision: TimestampPrecision,
    /// Canonical category.
    pub kind: EventKind,
    /// Optional repository context.
    pub workspace: Option<WorkspaceContext>,
    /// Kind-specific payload with lexically ordered object keys.
    pub payload: BTreeMap<String, Value>,
    /// Complete native-source provenance.
    pub provenance: EventProvenance,
}

impl CanonicalEvent {
    /// Constructs an event and derives its stable ID from semantic and native
    /// source identity.
    ///
    /// # Errors
    ///
    /// Returns [`EventError::Serialization`] if canonical identity input cannot
    /// be serialized.
    pub fn from_draft(draft: EventDraft) -> Result<Self, EventError> {
        validate_draft(&draft)?;
        let event_id = compute_event_id(&draft)?;
        Ok(Self {
            schema_version: SchemaVersion::V1_0,
            event_id,
            tool: draft.tool,
            native_session_id: draft.native_session_id,
            sequence: draft.sequence,
            timestamp: draft.timestamp,
            timestamp_precision: draft.timestamp_precision,
            kind: draft.kind,
            workspace: draft.workspace,
            payload: draft.payload,
            provenance: draft.provenance,
        })
    }

    /// Parses JSON and verifies its schema version, timestamp, and deterministic
    /// event ID.
    ///
    /// # Errors
    ///
    /// Returns an [`EventError`] for malformed JSON, unsupported values, empty
    /// required identity fields, or an ID that does not match the event.
    pub fn from_json(json: &str) -> Result<Self, EventError> {
        let event: Self =
            serde_json::from_str(json).map_err(|error| EventError::Json(error.to_string()))?;
        let draft = event.as_draft();
        validate_draft(&draft)?;
        let expected = compute_event_id(&draft)?;
        if event.event_id != expected {
            return Err(EventError::EventIdMismatch {
                supplied: event.event_id,
                expected,
            });
        }
        Ok(event)
    }

    /// Serializes the canonical event to stable compact JSON.
    ///
    /// # Errors
    ///
    /// Returns [`EventError::Serialization`] when serialization fails.
    pub fn to_json(&self) -> Result<String, EventError> {
        serde_json::to_string(self).map_err(|error| EventError::Serialization(error.to_string()))
    }

    fn as_draft(&self) -> EventDraft {
        EventDraft {
            tool: self.tool.clone(),
            native_session_id: self.native_session_id.clone(),
            sequence: self.sequence,
            timestamp: self.timestamp.clone(),
            timestamp_precision: self.timestamp_precision,
            kind: self.kind,
            workspace: self.workspace.clone(),
            payload: self.payload.clone(),
            provenance: self.provenance.clone(),
        }
    }
}

/// Failure returned while constructing or validating canonical events.
#[derive(Clone, Debug, PartialEq)]
pub enum EventError {
    /// An RFC 3339 timestamp was invalid.
    InvalidTimestamp(String),
    /// JSON parsing or schema-version deserialization failed.
    Json(String),
    /// A required domain field was empty or invalid.
    InvalidField {
        /// Dotted field name.
        field: &'static str,
        /// Safe validation explanation.
        message: &'static str,
    },
    /// Supplied ID did not match the canonical identity input.
    EventIdMismatch {
        /// ID stored in the input.
        supplied: EventId,
        /// ID recomputed from the event.
        expected: EventId,
    },
    /// Canonical JSON serialization failed.
    Serialization(String),
}

impl Display for EventError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTimestamp(message) => write!(formatter, "invalid timestamp: {message}"),
            Self::Json(message) => write!(formatter, "invalid canonical event JSON: {message}"),
            Self::InvalidField { field, message } => write!(formatter, "{field}: {message}"),
            Self::EventIdMismatch { supplied, expected } => write!(
                formatter,
                "event_id mismatch: supplied {}, expected {}",
                supplied.as_str(),
                expected.as_str()
            ),
            Self::Serialization(message) => {
                write!(formatter, "canonical serialization failed: {message}")
            }
        }
    }
}

impl Error for EventError {}

#[derive(Serialize)]
struct EventIdentity<'a> {
    schema_version: SchemaVersion,
    tool: &'a ToolIdentity,
    native_session_id: &'a str,
    sequence: u64,
    timestamp: &'a EventTimestamp,
    timestamp_precision: TimestampPrecision,
    kind: EventKind,
    workspace: &'a Option<WorkspaceContext>,
    payload: &'a BTreeMap<String, Value>,
    source_generation: &'a str,
    source_offset: u64,
}

fn compute_event_id(draft: &EventDraft) -> Result<EventId, EventError> {
    let identity = EventIdentity {
        schema_version: SchemaVersion::V1_0,
        tool: &draft.tool,
        native_session_id: &draft.native_session_id,
        sequence: draft.sequence,
        timestamp: &draft.timestamp,
        timestamp_precision: draft.timestamp_precision,
        kind: draft.kind,
        workspace: &draft.workspace,
        payload: &draft.payload,
        source_generation: &draft.provenance.source_generation,
        source_offset: draft.provenance.source_offset,
    };
    let bytes = serde_json::to_vec(&identity)
        .map_err(|error| EventError::Serialization(error.to_string()))?;
    Ok(EventId::from_digest(&bytes))
}

fn validate_draft(draft: &EventDraft) -> Result<(), EventError> {
    for (field, value) in [
        ("tool.family", draft.tool.family.as_str()),
        ("tool.surface", draft.tool.surface.as_str()),
        ("tool.profile", draft.tool.profile.as_str()),
        ("native_session_id", draft.native_session_id.as_str()),
        (
            "provenance.source_path",
            draft.provenance.source_path.as_str(),
        ),
        (
            "provenance.source_generation",
            draft.provenance.source_generation.as_str(),
        ),
        (
            "provenance.adapter_version",
            draft.provenance.adapter_version.as_str(),
        ),
    ] {
        if value.is_empty() {
            return Err(EventError::InvalidField {
                field,
                message: "must not be empty",
            });
        }
    }
    if draft
        .provenance
        .ordering_confidence
        .is_some_and(|confidence| !(0.0..=1.0).contains(&confidence))
    {
        return Err(EventError::InvalidField {
            field: "provenance.ordering_confidence",
            message: "must be between 0 and 1",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> EventDraft {
        EventDraft {
            tool: ToolIdentity {
                family: "codex".to_owned(),
                surface: "cli".to_owned(),
                profile: "default".to_owned(),
            },
            native_session_id: "session-α".to_owned(),
            sequence: 42,
            timestamp: EventTimestamp::parse("2026-07-20T14:31:22.192Z")
                .expect("fixture timestamp is valid"),
            timestamp_precision: TimestampPrecision::Millisecond,
            kind: EventKind::ToolResult,
            workspace: Some(WorkspaceContext {
                cwd: Some("/workspace/sessionmesh".to_owned()),
                repository_id: Some("repo_example".to_owned()),
                branch: Some("main".to_owned()),
                head: Some("a218df4".to_owned()),
            }),
            payload: BTreeMap::from([
                ("exit_code".to_owned(), Value::from(0)),
                ("message".to_owned(), Value::from("Grüße")),
                ("tool_name".to_owned(), Value::from("shell")),
            ]),
            provenance: EventProvenance {
                source_path: "/sources/codex/rollout.jsonl".to_owned(),
                original_path: Some("~/.codex/rollout.jsonl".to_owned()),
                source_offset: 17_428,
                source_generation: "device:inode".to_owned(),
                adapter_version: "0.1.0".to_owned(),
                ingestion_sequence: 108,
                ordering_confidence: Some(1.0),
            },
        }
    }

    #[test]
    fn round_trips_canonical_json() {
        let event = CanonicalEvent::from_draft(draft()).expect("draft should be valid");
        let json = event.to_json().expect("event should serialize");
        let parsed = CanonicalEvent::from_json(&json).expect("serialized event should validate");

        assert_eq!(parsed, event);
    }

    #[test]
    fn equal_inputs_produce_equal_ids() {
        let first = CanonicalEvent::from_draft(draft()).expect("draft should be valid");
        let second = CanonicalEvent::from_draft(draft()).expect("draft should be valid");

        assert_eq!(first.event_id, second.event_id);
    }

    #[test]
    fn payload_key_insertion_order_does_not_change_id() {
        let first = draft();
        let mut second = draft();
        second.payload = [
            ("tool_name".to_owned(), Value::from("shell")),
            ("message".to_owned(), Value::from("Grüße")),
            ("exit_code".to_owned(), Value::from(0)),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            CanonicalEvent::from_draft(first)
                .expect("first draft is valid")
                .event_id,
            CanonicalEvent::from_draft(second)
                .expect("second draft is valid")
                .event_id
        );
    }

    #[test]
    fn import_metadata_and_runtime_path_do_not_change_id() {
        let first = draft();
        let mut second = draft();
        second.provenance.source_path = "/different/container/path".to_owned();
        second.provenance.original_path = Some("/host/.codex/rollout.jsonl".to_owned());
        second.provenance.adapter_version = "0.2.0".to_owned();
        second.provenance.ingestion_sequence = 9_999;
        second.provenance.ordering_confidence = Some(0.7);

        assert_eq!(
            CanonicalEvent::from_draft(first)
                .expect("first draft is valid")
                .event_id,
            CanonicalEvent::from_draft(second)
                .expect("second draft is valid")
                .event_id
        );
    }

    #[test]
    fn semantic_payload_change_produces_a_different_id() {
        let first = draft();
        let mut second = draft();
        second
            .payload
            .insert("exit_code".to_owned(), Value::from(1));

        assert_ne!(
            CanonicalEvent::from_draft(first)
                .expect("first draft is valid")
                .event_id,
            CanonicalEvent::from_draft(second)
                .expect("second draft is valid")
                .event_id
        );
    }

    #[test]
    fn rejects_invalid_timestamp() {
        let error = EventTimestamp::parse("tomorrow").expect_err("timestamp must be RFC 3339");
        assert!(matches!(error, EventError::InvalidTimestamp(_)));
    }

    #[test]
    fn rejects_unsupported_schema_version() {
        let event = CanonicalEvent::from_draft(draft()).expect("draft should be valid");
        let json = event
            .to_json()
            .expect("event should serialize")
            .replace("\"schema_version\":\"1.0\"", "\"schema_version\":\"2.0\"");

        let error = CanonicalEvent::from_json(&json).expect_err("version must be rejected");
        assert!(matches!(error, EventError::Json(_)));
    }

    #[test]
    fn rejects_tampered_event_id() {
        let event = CanonicalEvent::from_draft(draft()).expect("draft should be valid");
        let original = event.event_id.as_str().to_owned();
        let replacement = format!("sha256:{}", "a".repeat(64));
        let json = event
            .to_json()
            .expect("event should serialize")
            .replace(&original, &replacement);

        let error = CanonicalEvent::from_json(&json).expect_err("tampered ID must be rejected");
        assert!(matches!(error, EventError::EventIdMismatch { .. }));
    }

    #[test]
    fn unknown_kind_retains_native_payload() {
        let mut unknown = draft();
        unknown.kind = EventKind::Unknown;
        unknown
            .payload
            .insert("native_kind".to_owned(), Value::from("future_event"));

        let event = CanonicalEvent::from_draft(unknown).expect("unknown event should be retained");

        assert_eq!(event.kind, EventKind::Unknown);
        assert_eq!(
            event.payload.get("native_kind"),
            Some(&Value::from("future_event"))
        );
    }

    #[test]
    fn published_canonical_fixture_matches_the_rust_contract() {
        let fixture = include_str!("../../../schemas/examples/canonical-event.valid.json");
        CanonicalEvent::from_json(fixture)
            .expect("published canonical fixture must have a valid deterministic ID");
    }
}
