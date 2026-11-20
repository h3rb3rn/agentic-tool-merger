//! Grok Build read-only import contracts using synthetic ACP update records.
use sessionmesh_core::event::EventKind;
use sessionmesh_ingest::external::{ExternalSource, ExternalTool, discover_grok, ingest_external};
use sessionmesh_storage::{EventRepository, Storage};
use std::io::Write as _;

fn update(kind: &str, text: &str) -> String {
    format!(
        "{}\n",
        serde_json::json!({"timestamp":1_795_000_000,"timestamp_us":1_795_000_000_123_456_i64,"method":"session/update","params":{"sessionId":"native-session","_meta":{"eventId":"native-session-1"},"update":{"sessionUpdate":kind,"content":{"type":"text","text":text}}}})
    )
}

fn source(root: &std::path::Path) -> ExternalSource {
    let path = root.join("sessions/%2Fwork%2FGr%C3%BCn%2Bproject/native-session/updates.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    ExternalSource {
        tool: ExternalTool::Grok,
        original_path: std::path::PathBuf::from(
            "/host/.grok/sessions/%2Fwork%2FGr%C3%BCn%2Bproject/native-session/updates.jsonl",
        ),
        path,
    }
}

#[tokio::test]
async fn preserves_conversation_provenance_and_resumes_partial_append() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path());
    let original = update(
        "user_message_chunk",
        "Keep complete prompt: ```rust\nlet x = 1;\n``` 🇩🇪",
    ) + &update("tool_call", "discarded tool code")
        + &update("agent_message_chunk", "First reply")
        + &update("agent_thought_chunk", "Reasoning");
    std::fs::write(&src.path, &original).unwrap();
    let storage =
        Storage::open_with_retention(dir.path().join("state.db"), dir.path().join("blobs"), true)
            .await
            .unwrap();
    let first = ingest_external(&storage, &src, "2026-11-20T00:00:00Z")
        .await
        .unwrap();
    assert_eq!(first.events.len(), 3);
    assert_eq!(first.events[0].tool.family, "grok");
    assert_eq!(first.events[0].native_session_id, "grok:native-session");
    assert_eq!(
        first.events[0].workspace.as_ref().unwrap().cwd.as_deref(),
        Some("/work/Grün+project")
    );
    assert_eq!(first.events[0].kind, EventKind::UserMessage);
    assert_eq!(first.events[1].kind, EventKind::AssistantMessage);
    assert_eq!(first.events[2].kind, EventKind::Unknown);
    assert!(first.events[0].timestamp.as_str().contains("123456"));
    assert_eq!(
        first.events[0].provenance.original_path.as_deref(),
        src.original_path.to_str()
    );
    assert_eq!(
        first.events[0].payload["text"],
        "Keep complete prompt: ```rust\nlet x = 1;\n``` 🇩🇪"
    );
    assert!(
        !ingest_external(&storage, &src, "2026-11-20T00:01:00Z")
            .await
            .unwrap()
            .changed
    );
    let next = update("agent_message_chunk", "Second reply");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&src.path)
        .unwrap();
    file.write_all(next.trim_end_matches('\n').as_bytes())
        .unwrap();
    assert!(
        !ingest_external(&storage, &src, "2026-11-20T00:02:00Z")
            .await
            .unwrap()
            .changed
    );
    file.write_all(b"\n").unwrap();
    let second = ingest_external(&storage, &src, "2026-11-20T00:03:00Z")
        .await
        .unwrap();
    assert_eq!(second.events.len(), 1);
    assert_eq!(second.events[0].sequence, 3);
    assert_eq!(
        second.events[0].provenance.source_offset,
        original.len() as u64
    );
    assert_eq!(storage.event_count().await.unwrap(), 4);
    assert_eq!(
        std::fs::read_to_string(&src.path).unwrap(),
        original + &next
    );
    assert_eq!(
        storage
            .get_canonical_event(first.events[0].event_id.as_str())
            .await
            .unwrap(),
        Some(first.events[0].clone())
    );
}

#[test]
fn discovers_only_authoritative_updates_at_session_depth() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path());
    std::fs::write(&src.path, "").unwrap();
    std::fs::write(src.path.with_file_name("chat_history.jsonl"), "").unwrap();
    let artifact = src
        .path
        .parent()
        .unwrap()
        .join("artifacts/nested/updates.jsonl");
    std::fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    std::fs::write(artifact, "").unwrap();
    let found = discover_grok(dir.path(), std::path::Path::new("/host/.grok"));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0], src);
}

#[tokio::test]
async fn hashed_workspace_and_invalid_json_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let mut src = source(dir.path());
    src.path = dir
        .path()
        .join("sessions/hashed-group/native-session/updates.jsonl");
    std::fs::create_dir_all(src.path.parent().unwrap()).unwrap();
    std::fs::write(
        src.path.parent().unwrap().parent().unwrap().join(".cwd"),
        "/work/long workspace",
    )
    .unwrap();
    std::fs::write(&src.path, update("user_message_chunk", "Prompt")).unwrap();
    let storage = Storage::open(dir.path().join("state.db"), dir.path().join("blobs"))
        .await
        .unwrap();
    let imported = ingest_external(&storage, &src, "2026-11-20T00:00:00Z")
        .await
        .unwrap();
    assert_eq!(
        imported.events[0]
            .workspace
            .as_ref()
            .unwrap()
            .cwd
            .as_deref(),
        Some("/work/long workspace")
    );
    std::fs::write(&src.path, "invalid JSON\n").unwrap();
    assert!(
        ingest_external(&storage, &src, "2026-11-20T00:00:00Z")
            .await
            .is_err()
    );
    assert_eq!(storage.event_count().await.unwrap(), 1);
}

#[tokio::test]
async fn batches_skip_tool_bodies_but_keep_large_replies_and_replay_identity() {
    let dir = tempfile::tempdir().unwrap();
    let src = source(dir.path());
    let text = "x".repeat(4 * 1024 * 1024 + 100);
    let first = update("agent_message_chunk", &text);
    let rest =
        update("tool_call_update", "tool output") + &update("user_message_chunk", "After batch");
    std::fs::write(&src.path, first.clone() + &rest).unwrap();
    let storage = Storage::open(dir.path().join("state.db"), dir.path().join("blobs"))
        .await
        .unwrap();
    let a = ingest_external(&storage, &src, "2026-11-20T00:00:00Z")
        .await
        .unwrap();
    assert!(!a.caught_up);
    assert_eq!(a.events.len(), 1);
    assert_eq!(a.events[0].payload["text"], text);
    let b = ingest_external(&storage, &src, "2026-11-20T00:00:00Z")
        .await
        .unwrap();
    assert!(b.caught_up);
    assert_eq!(b.events.len(), 1);
    assert_eq!(b.events[0].sequence, 1);
    let mut replacement = update("user_message_chunk", "Replacement");
    replacement.push_str(&(first + &rest));
    std::fs::write(&src.path, replacement).unwrap();
    while !ingest_external(&storage, &src, "2026-11-20T00:00:00Z")
        .await
        .unwrap()
        .caught_up
    {}
    assert_eq!(storage.event_count().await.unwrap(), 5);
}
