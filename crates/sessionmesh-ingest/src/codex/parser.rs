//! Pure Codex rollout parsing with record-level error isolation.

use std::collections::{BTreeMap, HashSet};

use serde_json::{Map, Value};
use sessionmesh_core::event::{
    CanonicalEvent, EventDraft, EventKind, EventProvenance, EventTimestamp, TimestampPrecision,
    ToolIdentity, WorkspaceContext,
};

/// Immutable source context supplied by discovery and ingestion.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexParseContext {
    /// Runtime-readable source path.
    pub source_path: String,
    /// Original host-facing path.
    pub original_path: Option<String>,
    /// Stable identity for this file generation.
    pub source_generation: String,
    /// Byte offset assigned to the first supplied record.
    pub base_offset: u64,
    /// Native sequence assigned to the first supplied record.
    pub first_native_sequence: u64,
    /// Parser version retained in provenance.
    pub adapter_version: String,
    /// Session identity used only if metadata is absent.
    pub fallback_session_id: String,
    /// Durable ingestion sequence assigned to the first record.
    pub first_ingestion_sequence: u64,
}

/// Exact native bytes and their source coordinates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawRolloutRecord {
    /// Zero-based byte offset in the rollout generation.
    pub source_offset: u64,
    /// One-based physical line number.
    pub line_number: u64,
    /// Original bytes, including a trailing newline when present.
    pub bytes: Vec<u8>,
}

/// Smallest isolated parser failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseIssue {
    /// Stable category.
    pub code: &'static str,
    /// Safe explanation without copying native payload text.
    pub message: String,
}

/// One raw record and its optional normalized interpretation.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedRecord {
    /// Unchanged native record.
    pub raw: RawRolloutRecord,
    /// Canonical event when normalization succeeded.
    pub event: Option<CanonicalEvent>,
    /// Record-local issue when normalization was unsafe.
    pub issue: Option<ParseIssue>,
    /// JSON pointers that should be redacted before derived processing.
    pub sensitive_fields: Vec<String>,
}

/// Parses complete rollout bytes without filesystem or database access.
///
/// Invalid records retain their original bytes and do not prevent later
/// records from being interpreted.
#[must_use]
pub fn parse_rollout(bytes: &[u8], context: &CodexParseContext) -> Vec<ParsedRecord> {
    let mut state = ParserState {
        session_id: context.fallback_session_id.clone(),
        workspace: None,
        tool_calls: HashSet::new(),
    };
    let mut offset = context.base_offset;
    let mut line_number = context.first_native_sequence.saturating_add(1);
    bytes
        .split_inclusive(|byte| *byte == b'\n')
        .map(|native_bytes| {
            let record = RawRolloutRecord {
                source_offset: offset,
                line_number,
                bytes: native_bytes.to_vec(),
            };
            offset = offset.saturating_add(u64::try_from(native_bytes.len()).unwrap_or(u64::MAX));
            line_number = line_number.saturating_add(1);
            parse_record(record, context, &mut state)
        })
        .collect()
}

struct ParserState {
    session_id: String,
    workspace: Option<WorkspaceContext>,
    tool_calls: HashSet<String>,
}

fn parse_record(
    raw: RawRolloutRecord,
    context: &CodexParseContext,
    state: &mut ParserState,
) -> ParsedRecord {
    if raw.bytes.last() != Some(&b'\n') {
        return failed(
            raw,
            "codex_record_incomplete",
            "record has no line terminator",
        );
    }
    let line = raw.bytes.strip_suffix(b"\n").unwrap_or(&raw.bytes);
    let json_bytes = line.strip_suffix(b"\r").unwrap_or(line);
    let native = match serde_json::from_slice::<Value>(json_bytes) {
        Ok(Value::Object(native)) => native,
        Ok(_) => {
            return failed(
                raw,
                "codex_record_not_object",
                "record must be a JSON object",
            );
        }
        Err(error) => {
            return failed(
                raw,
                "codex_record_malformed",
                &format!("{:?}", error.classify()).to_ascii_lowercase(),
            );
        }
    };
    let sensitive_fields = sensitive_pointers(&Value::Object(native.clone()), "");
    match normalize(&native, &raw, context, state) {
        Ok(event) => ParsedRecord {
            raw,
            event: Some(event),
            issue: None,
            sensitive_fields,
        },
        Err(issue) => ParsedRecord {
            raw,
            event: None,
            issue: Some(issue),
            sensitive_fields,
        },
    }
}

fn failed(raw: RawRolloutRecord, code: &'static str, message: &str) -> ParsedRecord {
    ParsedRecord {
        raw,
        event: None,
        issue: Some(ParseIssue {
            code,
            message: message.to_owned(),
        }),
        sensitive_fields: Vec::new(),
    }
}

fn normalize(
    native: &Map<String, Value>,
    raw: &RawRolloutRecord,
    context: &CodexParseContext,
    state: &mut ParserState,
) -> Result<CanonicalEvent, ParseIssue> {
    let timestamp_text = native
        .get("timestamp")
        .and_then(Value::as_str)
        .ok_or_else(|| issue("codex_timestamp_missing", "timestamp is required"))?;
    let timestamp = EventTimestamp::parse(timestamp_text)
        .map_err(|_| issue("codex_timestamp_invalid", "timestamp is not RFC 3339"))?;
    let native_type = native
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| issue("codex_type_missing", "type is required"))?;
    let payload = native
        .get("payload")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    if native_type == "session_meta" {
        if let Some(id) = payload.get("id").and_then(Value::as_str) {
            id.clone_into(&mut state.session_id);
        }
        state.workspace = workspace_from(&payload, state.workspace.clone());
    } else if native_type == "turn_context" {
        state.workspace = workspace_from(&payload, state.workspace.clone());
    }

    let (kind, normalized_payload) = map_event(native_type, &payload, state);
    let sequence = raw.line_number - 1;
    CanonicalEvent::from_draft(EventDraft {
        tool: ToolIdentity {
            family: "codex".to_owned(),
            surface: "cli".to_owned(),
            profile: "default".to_owned(),
        },
        native_session_id: state.session_id.clone(),
        sequence,
        timestamp,
        timestamp_precision: timestamp_precision(timestamp_text),
        kind,
        workspace: state.workspace.clone(),
        payload: normalized_payload,
        provenance: EventProvenance {
            source_path: context.source_path.clone(),
            original_path: context.original_path.clone(),
            source_offset: raw.source_offset,
            source_generation: context.source_generation.clone(),
            adapter_version: context.adapter_version.clone(),
            ingestion_sequence: context.first_ingestion_sequence + sequence,
            ordering_confidence: Some(1.0),
        },
    })
    .map_err(|_| {
        issue(
            "codex_canonical_event_invalid",
            "canonical validation failed",
        )
    })
}

fn map_event(
    native_type: &str,
    payload: &Map<String, Value>,
    state: &mut ParserState,
) -> (EventKind, BTreeMap<String, Value>) {
    let subtype = payload.get("type").and_then(Value::as_str);
    let kind = match (native_type, subtype) {
        ("session_meta", _) | ("event_msg", Some("task_started")) => EventKind::SessionStart,
        ("event_msg", Some("task_complete")) => EventKind::SessionEnd,
        ("response_item", Some("message")) => match payload.get("role").and_then(Value::as_str) {
            Some("user") => EventKind::UserMessage,
            Some("assistant") => EventKind::AssistantMessage,
            _ => EventKind::SystemMessage,
        },
        ("event_msg", Some("user_message")) => EventKind::UserMessage,
        ("event_msg", Some("agent_message" | "agent_reasoning")) => EventKind::AssistantMessage,
        ("response_item", Some("function_call")) => EventKind::ToolCall,
        ("response_item", Some("function_call_output")) => EventKind::ToolResult,
        ("event_msg", Some("context_compacted")) | ("compacted", _) => EventKind::Compaction,
        ("turn_context", _) => EventKind::GitState,
        ("event_msg", Some("plan_update")) => EventKind::Plan,
        ("event_msg", Some("error")) => EventKind::Error,
        _ => EventKind::Unknown,
    };
    let mut normalized = payload
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    normalized.insert(
        "native_type".to_owned(),
        Value::String(native_type.to_owned()),
    );
    if matches!(
        kind,
        EventKind::UserMessage | EventKind::AssistantMessage | EventKind::SystemMessage
    ) {
        normalized.insert("text".to_owned(), Value::String(extract_text(payload)));
    }
    if kind == EventKind::ToolCall
        && let Some(call_id) = payload.get("call_id").and_then(Value::as_str)
    {
        state.tool_calls.insert(call_id.to_owned());
    }
    if kind == EventKind::ToolResult
        && let Some(call_id) = payload.get("call_id").and_then(Value::as_str)
    {
        normalized.insert(
            "matched_call".to_owned(),
            Value::Bool(state.tool_calls.contains(call_id)),
        );
    }
    (kind, normalized)
}

fn extract_text(payload: &Map<String, Value>) -> String {
    if let Some(message) = payload.get("message").and_then(Value::as_str) {
        return message.to_owned();
    }
    payload
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("")
}

fn workspace_from(
    payload: &Map<String, Value>,
    previous: Option<WorkspaceContext>,
) -> Option<WorkspaceContext> {
    let git = payload.get("git").and_then(Value::as_object);
    let prior = previous.unwrap_or(WorkspaceContext {
        cwd: None,
        repository_id: None,
        branch: None,
        head: None,
    });
    let workspace = WorkspaceContext {
        cwd: payload
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or(prior.cwd),
        repository_id: prior.repository_id,
        branch: git
            .and_then(|value| value.get("branch"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or(prior.branch),
        head: git
            .and_then(|value| value.get("head"))
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or(prior.head),
    };
    (workspace.cwd.is_some() || workspace.branch.is_some() || workspace.head.is_some())
        .then_some(workspace)
}

fn timestamp_precision(timestamp: &str) -> TimestampPrecision {
    let Some(fraction) = timestamp
        .split_once('.')
        .map(|(_, remainder)| remainder.split(['Z', '+', '-']).next().unwrap_or_default())
    else {
        return TimestampPrecision::Second;
    };
    match fraction.len() {
        0 => TimestampPrecision::Second,
        1..=3 => TimestampPrecision::Millisecond,
        4..=6 => TimestampPrecision::Microsecond,
        _ => TimestampPrecision::Nanosecond,
    }
}

fn sensitive_pointers(value: &Value, prefix: &str) -> Vec<String> {
    let mut pointers = Vec::new();
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let pointer = format!("{prefix}/{}", escape_pointer(key));
                if is_sensitive_key(key) {
                    pointers.push(pointer.clone());
                }
                pointers.extend(sensitive_pointers(child, &pointer));
            }
        }
        Value::Array(array) => {
            for (index, child) in array.iter().enumerate() {
                let pointer = format!("{prefix}/{index}");
                pointers.extend(sensitive_pointers(child, &pointer));
            }
        }
        _ => {}
    }
    pointers
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    ["api_key", "authorization", "password", "secret", "token"]
        .iter()
        .any(|candidate| key.contains(candidate))
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn issue(code: &'static str, message: &str) -> ParseIssue {
    ParseIssue {
        code,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../../../../fixtures/codex/parser/rollout.jsonl");
    const EXPECTED: &str = include_str!("../../../../fixtures/codex/parser/expected.json");

    #[derive(Deserialize)]
    struct Expected {
        session_id: String,
        kinds: Vec<String>,
        tool_result_call_id: String,
        tool_result_matched: bool,
        sensitive_pointer: String,
        malformed_records: usize,
    }

    fn context() -> CodexParseContext {
        CodexParseContext {
            source_path: "/sources/codex/rollout.jsonl".to_owned(),
            original_path: Some("~/.codex/rollout.jsonl".to_owned()),
            source_generation: "device:inode".to_owned(),
            base_offset: 0,
            first_native_sequence: 0,
            adapter_version: "0.1.0".to_owned(),
            fallback_session_id: "fallback".to_owned(),
            first_ingestion_sequence: 100,
        }
    }

    #[test]
    fn sanitized_fixture_matches_expected_mapping() {
        let expected: Expected = serde_json::from_str(EXPECTED).expect("expected JSON is valid");
        let parsed = parse_rollout(FIXTURE, &context());
        let events = parsed
            .iter()
            .filter_map(|record| record.event.as_ref())
            .collect::<Vec<_>>();
        let kinds = events
            .iter()
            .map(|event| {
                serde_json::to_value(event.kind)
                    .expect("kind serializes")
                    .as_str()
                    .expect("kind is a string")
                    .to_owned()
            })
            .collect::<Vec<_>>();

        assert_eq!(events[0].native_session_id, expected.session_id);
        assert_eq!(kinds, expected.kinds);
        assert_eq!(
            events[4].payload.get("call_id").and_then(Value::as_str),
            Some(expected.tool_result_call_id.as_str())
        );
        assert_eq!(
            events[4]
                .payload
                .get("matched_call")
                .and_then(Value::as_bool),
            Some(expected.tool_result_matched)
        );
        assert_eq!(
            parsed
                .iter()
                .filter(|record| record.issue.is_some())
                .count(),
            expected.malformed_records
        );
        assert!(
            parsed[7]
                .sensitive_fields
                .contains(&expected.sensitive_pointer)
        );
    }

    #[test]
    fn multiline_unicode_and_timestamp_precision_are_preserved() {
        let parsed = parse_rollout(FIXTURE, &context());
        let message = parsed[1].event.as_ref().expect("message should parse");

        assert_eq!(
            message.payload.get("text").and_then(Value::as_str),
            Some("Implement\nUnicode: Grüße 🌍")
        );
        assert_eq!(message.timestamp_precision, TimestampPrecision::Millisecond);
        assert_eq!(
            parsed[2]
                .event
                .as_ref()
                .map(|event| event.timestamp_precision),
            Some(TimestampPrecision::Microsecond)
        );
    }

    #[test]
    fn malformed_record_does_not_suppress_later_unknown_event() {
        let parsed = parse_rollout(FIXTURE, &context());

        assert_eq!(
            parsed[5].issue.as_ref().map(|issue| issue.code),
            Some("codex_record_malformed")
        );
        assert_eq!(
            parsed[7].event.as_ref().map(|event| event.kind),
            Some(EventKind::Unknown)
        );
        assert_eq!(
            parsed[7].raw.bytes.strip_suffix(b"\n").unwrap(),
            FIXTURE.split(|byte| *byte == b'\n').nth(7).unwrap()
        );
    }

    #[test]
    fn every_event_points_to_its_raw_record() {
        let parsed = parse_rollout(FIXTURE, &context());

        for record in parsed.iter().filter(|record| record.event.is_some()) {
            let event = record.event.as_ref().expect("filtered event exists");
            assert_eq!(event.provenance.source_offset, record.raw.source_offset);
            assert_eq!(event.provenance.source_path, context().source_path);
        }
    }

    #[test]
    fn equal_input_produces_stable_event_ids() {
        let first = parse_rollout(FIXTURE, &context());
        let second = parse_rollout(FIXTURE, &context());
        let first_ids = first
            .iter()
            .filter_map(|record| record.event.as_ref())
            .map(|event| event.event_id.as_str())
            .collect::<Vec<_>>();
        let second_ids = second
            .iter()
            .filter_map(|record| record.event.as_ref())
            .map(|event| event.event_id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(first_ids, second_ids);
        assert_eq!(
            first
                .iter()
                .flat_map(|record| record.raw.bytes.iter().copied())
                .collect::<Vec<_>>(),
            FIXTURE
        );
    }

    #[test]
    fn missing_optional_message_content_is_accepted() {
        let bytes = b"{\"timestamp\":\"2026-07-20T14:30:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\"}}\n";
        let parsed = parse_rollout(bytes, &context());

        assert!(parsed[0].issue.is_none());
        assert_eq!(
            parsed[0]
                .event
                .as_ref()
                .and_then(|event| event.payload.get("text"))
                .and_then(Value::as_str),
            Some("")
        );
    }

    #[test]
    fn missing_timestamp_is_isolated() {
        let bytes = b"{\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\"}}\n{\"timestamp\":\"2026-07-20T14:30:01Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"agent_message\",\"message\":\"later\"}}\n";
        let parsed = parse_rollout(bytes, &context());

        assert_eq!(
            parsed[0].issue.as_ref().unwrap().code,
            "codex_timestamp_missing"
        );
        assert!(parsed[1].event.is_some());
    }

    #[test]
    fn unterminated_record_is_left_for_incremental_ingestion() {
        let parsed = parse_rollout(b"{\"timestamp\":\"2026-07-20T14:30:00Z\"}", &context());

        assert_eq!(
            parsed[0].issue.as_ref().unwrap().code,
            "codex_record_incomplete"
        );
    }
}
