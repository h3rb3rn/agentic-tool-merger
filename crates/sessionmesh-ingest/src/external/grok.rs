//! Grok Build's authoritative ACP update log, independent of compacted chat snapshots.
use super::{ExternalError, ExternalSource, ParsedRecord, event};
use sessionmesh_core::event::{EventKind, WorkspaceContext};
use std::{collections::BTreeMap, io::Read as _, path::Path};

/// Reads only the documented group/session/update layout; never traverses artifacts or links.
pub(super) fn discover(home: &Path, original_home: &Path) -> Vec<ExternalSource> {
    let mut sources = Vec::new();
    let Ok(groups) = std::fs::read_dir(home.join("sessions")) else {
        return sources;
    };
    for group in groups
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
    {
        let Ok(sessions) = std::fs::read_dir(group.path()) else {
            continue;
        };
        for session in sessions
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        {
            let path = session.path().join("updates.jsonl");
            if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file())
            {
                sources.push(ExternalSource {
                    tool: super::ExternalTool::Grok,
                    original_path: original_home
                        .join(path.strip_prefix(home).expect("discovered beneath home")),
                    path,
                });
            }
        }
    }
    sources.sort_by(|left, right| left.path.cmp(&right.path));
    sources
}

/// Preserves each native conversation chunk and its exact line/offset; tools stay outside this surface.
pub(super) fn parse(
    bytes: &[u8],
    source: &ExternalSource,
    generation: &str,
    base_offset: u64,
    base_sequence: u64,
) -> Result<Vec<ParsedRecord>, ExternalError> {
    let directory = source
        .path
        .parent()
        .ok_or_else(|| invalid("missing Grok session directory"))?;
    let session_id = directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("invalid Grok session directory"))?;
    let cwd = workspace(
        directory
            .parent()
            .ok_or_else(|| invalid("missing Grok workspace directory"))?,
    )?;
    let mut records = Vec::new();
    let mut offset = base_offset;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let start = offset;
        offset = offset.saturating_add(line.len() as u64);
        let value: serde_json::Value = serde_json::from_slice(line)?;
        let params = &value["params"];
        if params["_meta"]["origin"] == "sessionmesh"
            && params["_meta"]["ingestion_excluded"] == true
        {
            continue;
        }
        let update = &params["update"];
        let native_type = update["sessionUpdate"].as_str().unwrap_or("");
        let kind = match native_type {
            "user_message_chunk" => EventKind::UserMessage,
            "agent_message_chunk" => EventKind::AssistantMessage,
            "agent_thought_chunk" => EventKind::Unknown,
            _ => continue,
        };
        if params["sessionId"].as_str() != Some(session_id) {
            return Err(invalid(
                "Grok update session identity does not match its directory",
            ));
        }
        let timestamp = timestamp(&value)?;
        let mut payload = BTreeMap::new();
        payload.insert(
            "text".to_owned(),
            update["content"]["text"].as_str().unwrap_or("").into(),
        );
        payload.insert("content".to_owned(), update["content"].clone());
        payload.insert("native_type".to_owned(), native_type.into());
        payload.insert("native_metadata".to_owned(), params["_meta"].clone());
        records.push(ParsedRecord {
            offset: start,
            raw: line.to_vec(),
            event: event(
                source,
                &format!("grok:{session_id}"),
                base_sequence + records.len() as u64,
                &timestamp,
                kind,
                Some(WorkspaceContext {
                    cwd: Some(cwd.clone()),
                    repository_id: None,
                    branch: None,
                    head: None,
                }),
                payload,
                generation,
                start,
            )?,
        });
    }
    Ok(records)
}

/// Uses native event time rather than mutable transcript mtime, preserving subsecond evidence.
fn timestamp(value: &serde_json::Value) -> Result<String, ExternalError> {
    let time = if let Some(microseconds) = value["timestamp_us"].as_i64() {
        chrono::DateTime::from_timestamp_micros(microseconds)
    } else if let Some(milliseconds) = value
        .pointer("/params/_meta/agentTimestampMs")
        .and_then(serde_json::Value::as_i64)
    {
        chrono::DateTime::from_timestamp_millis(milliseconds)
    } else if let Some(seconds) = value["timestamp"].as_i64() {
        chrono::DateTime::from_timestamp(seconds, 0)
    } else {
        None
    };
    time.map(|time| time.to_rfc3339())
        .ok_or_else(|| invalid("Grok conversation update has no valid native timestamp"))
}

/// Resolves URL-encoded CWD or Grok's bounded .cwd sidecar for long directory names.
fn workspace(group: &Path) -> Result<String, ExternalError> {
    let sidecar = group.join(".cwd");
    if sidecar.try_exists()? {
        let mut text = String::new();
        std::fs::File::open(sidecar)?
            .take(65_537)
            .read_to_string(&mut text)?;
        if text.len() > 65_536 || text.is_empty() {
            return Err(invalid("invalid Grok .cwd sidecar"));
        }
        return Ok(text);
    }
    let encoded = group
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("invalid Grok workspace group"))?;
    let mut decoded = Vec::new();
    let mut bytes = encoded.bytes();
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            let high = bytes
                .next()
                .and_then(|byte| char::from(byte).to_digit(16))
                .ok_or_else(|| invalid("invalid Grok workspace encoding"))?;
            let low = bytes
                .next()
                .and_then(|byte| char::from(byte).to_digit(16))
                .ok_or_else(|| invalid("invalid Grok workspace encoding"))?;
            u8::try_from(high * 16 + low).expect("decoded byte")
        } else {
            byte
        });
    }
    String::from_utf8(decoded).map_err(|_| invalid("Grok workspace encoding is not UTF-8"))
}

/// Reports format problems without echoing private transcript contents.
fn invalid(message: &'static str) -> ExternalError {
    ExternalError::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    ))
}
