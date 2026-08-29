use super::*;
use crate::agent::AgentKind;
use crate::testutil::write_jsonl;
use std::path::Path;

fn fixture(home: &Path) -> PathBuf {
    write_jsonl(
        home,
        ".claude/projects/example/session.jsonl",
        &[
            r#"{"timestamp":"2026-07-30T10:00:00Z","type":"user","promptSource":"typed","message":{"content":"first prompt"}}"#,
            r#"{"timestamp":"2026-07-30T10:01:00Z","type":"user","promptSource":"typed","message":{"content":"second prompt"}}"#,
        ],
    )
}

#[test]
fn list_and_detail_data_expose_structured_session_state() {
    let home = tempfile::tempdir().unwrap();
    let path = fixture(home.path());
    let backend = backend_for(AgentKind::Claude);
    let listed = list_data(backend.as_ref(), home.path()).unwrap();
    assert_eq!(listed.sessions.len(), 1);
    assert_eq!(listed.sessions[0].title, "first prompt");
    let id = listed.sessions[0].id.clone();
    let records = detail_records_for_test(backend.as_ref(), home.path(), &id).unwrap();
    let messages = records
        .into_iter()
        .filter_map(|record| match record {
            DetailRecord::Message(message) => Some(message.text),
            DetailRecord::Tool(_) | DetailRecord::Evidence(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(messages, ["first prompt", "second prompt"]);
    assert!(path.exists());
}

#[test]
fn malformed_transcripts_remain_visible_with_warnings() {
    let home = tempfile::tempdir().unwrap();
    write_jsonl(
        home.path(),
        ".claude/projects/example/bad.jsonl",
        &["not-json"],
    );
    let backend = backend_for(AgentKind::Claude);
    let listed = list_data(backend.as_ref(), home.path()).unwrap();
    assert_eq!(listed.sessions.len(), 1);
    assert!(listed.partial);
    assert!(!listed.sessions[0].warnings.is_empty());
}

#[test]
fn deletion_is_format_independent_and_supports_all() {
    let home = tempfile::tempdir().unwrap();
    write_jsonl(
        home.path(),
        ".claude/projects/example/one.jsonl",
        &["not-json"],
    );
    write_jsonl(home.path(), ".claude/projects/example/two.jsonl", &[]);
    let backend = backend_for(AgentKind::Claude);
    assert_eq!(
        delete_sessions(backend.as_ref(), home.path(), &[], true).unwrap(),
        2
    );
    assert!(
        discovery_summary(backend.as_ref(), home.path())
            .unwrap()
            .count
            == 0
    );
}

#[test]
fn empty_or_ambiguous_delete_selection_is_rejected() {
    let home = tempfile::tempdir().unwrap();
    write_jsonl(home.path(), ".claude/projects/a/one.jsonl", &[]);
    let backend = backend_for(AgentKind::Claude);
    assert!(delete_sessions(backend.as_ref(), home.path(), &[], false).is_err());
    assert!(delete_sessions(backend.as_ref(), home.path(), &[], true).is_ok());
}

#[cfg(unix)]
#[test]
fn strict_and_tolerant_discovery_ignore_symlinked_and_fifo_transcripts() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().unwrap();
    let transcript = write_jsonl(home.path(), ".claude/projects/example/session.jsonl", &[]);
    let outside = tempfile::tempdir().unwrap();
    let outside_transcript = write_jsonl(outside.path(), "outside.jsonl", &[]);
    let project = transcript.parent().unwrap();
    symlink(&outside_transcript, project.join("linked.jsonl")).unwrap();
    symlink(outside.path(), project.join("linked-directory")).unwrap();
    let fifo = project.join("pipe.jsonl");
    let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: `fifo_path` is a valid NUL-terminated path and mode has no pointers.
    assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);

    let backend = backend_for(AgentKind::Claude);
    assert_eq!(
        backend.files(home.path()).unwrap(),
        vec![transcript.clone()]
    );
    let discovery = backend.list_files(home.path()).unwrap();
    assert_eq!(discovery.files, vec![transcript]);
    assert!(discovery.errors.is_empty());
}

#[cfg(unix)]
#[test]
fn strict_and_tolerant_discovery_reject_symlinked_and_fifo_session_ancestors() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), home.path().join(".claude")).unwrap();
    let backend = backend_for(AgentKind::Claude);
    assert!(backend.files(home.path()).is_err());
    assert!(backend.list_files(home.path()).is_err());

    std::fs::remove_file(home.path().join(".claude")).unwrap();
    std::fs::create_dir(home.path().join(".claude")).unwrap();
    let projects = home.path().join(".claude/projects");
    let projects_path = CString::new(projects.as_os_str().as_bytes()).unwrap();
    // SAFETY: `projects_path` is a valid NUL-terminated path and mode has no pointers.
    assert_eq!(unsafe { libc::mkfifo(projects_path.as_ptr(), 0o600) }, 0);
    assert!(backend.files(home.path()).is_err());
    assert!(backend.list_files(home.path()).is_err());
}

#[test]
fn canonical_uuid_display_uses_a_short_suffix() {
    let id = "12345678-1234-1234-1234-123456789abc";
    assert!(is_canonical_uuid(id));
    assert!(!is_canonical_uuid("short"));
}

#[test]
fn detail_projection_keeps_chat_order_and_tool_activity() {
    let home = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        home.path(),
        ".claude/projects/example/conversation.jsonl",
        &[
            r#"{"timestamp":"2026-07-30T10:00:00Z","type":"user","promptSource":"typed","message":{"role":"user","content":"hello"}}"#,
            r#"{"timestamp":"2026-07-30T10:00:01Z","type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"I will inspect the project."},{"type":"tool_use","id":"tool-1","name":"read_file","input":{"path":"README.md"}}]}}"#,
            r#"{"timestamp":"2026-07-30T10:00:02Z","type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-1","content":"contents"}]},"promptSource":"tool"}"#,
            r#"{"timestamp":"2026-07-30T10:00:03Z","type":"assistant","message":{"role":"assistant","content":"The project is readable."}}"#,
        ],
    );
    let backend = backend_for(AgentKind::Claude);
    let records = detail_records_for_test(backend.as_ref(), home.path(), "conversation").unwrap();
    assert!(path.exists());
    assert!(
        matches!(records[0], DetailRecord::Message(ref message) if message.role == ConversationRole::User)
    );
    assert!(
        matches!(records[1], DetailRecord::Message(ref message) if message.role == ConversationRole::Assistant)
    );
    assert!(matches!(records[2], DetailRecord::Tool(ref tool) if tool.name == "read_file"));
    assert!(
        matches!(records[3], DetailRecord::Tool(ref tool) if tool.status == ToolActivityStatus::Completed)
    );
    assert!(
        matches!(records[4], DetailRecord::Message(ref message) if message.text == "The project is readable.")
    );
}

#[test]
fn list_summary_includes_latest_message_and_counts() {
    let home = tempfile::tempdir().unwrap();
    write_jsonl(
        home.path(),
        ".claude/projects/example/summary.jsonl",
        &[
            r#"{"timestamp":"2026-07-30T10:00:00Z","type":"user","promptSource":"typed","message":{"role":"user","content":"first"}}"#,
            r#"{"timestamp":"2026-07-30T10:00:01Z","type":"assistant","message":{"role":"assistant","content":"latest"}}"#,
        ],
    );
    let backend = backend_for(AgentKind::Claude);
    let listed = list_data(backend.as_ref(), home.path()).unwrap();
    assert_eq!(listed.sessions[0].latest_message, "latest");
    assert_eq!(listed.sessions[0].message_count, 2);
    assert_eq!(listed.sessions[0].tool_count, 0);
}

#[test]
fn codex_detail_projects_messages_tools_and_hidden_internal_records() {
    let home = tempfile::tempdir().unwrap();
    let id = "33333333-3333-3333-3333-333333333333";
    write_jsonl(
        home.path(),
        ".codex/sessions/2026/07/30/rollout-test-33333333-3333-3333-3333-333333333333.jsonl",
        &[
            r#"{"timestamp":"2026-07-30T10:00:00Z","type":"session_meta","payload":{"timestamp":"2026-07-30T10:00:00Z","cwd":"/work","model_provider":"openai","cli_version":"1.2.3"}}"#,
            r#"{"timestamp":"2026-07-30T10:00:01Z","type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"inspect this"}]}}"#,
            r#"{"timestamp":"2026-07-30T10:00:02Z","type":"event_msg","payload":{"type":"agent_message","message":"I will inspect it."}}"#,
            r#"{"timestamp":"2026-07-30T10:00:03Z","type":"response_item","payload":{"type":"function_call","call_id":"call-1","name":"read_file","arguments":{"path":"README.md"}}}"#,
            r#"{"timestamp":"2026-07-30T10:00:04Z","type":"response_item","payload":{"type":"function_call_output","call_id":"call-1","output":"contents"}}"#,
            r#"{"timestamp":"2026-07-30T10:00:05Z","type":"response_item","payload":{"type":"reasoning","summary":"hidden"}}"#,
            r#"{"timestamp":"2026-07-30T10:00:06Z","type":"event_msg","payload":{"role":"developer","content":"internal"}}"#,
        ],
    );
    let backend = backend_for(AgentKind::Codex);
    let records = detail_records_for_test(backend.as_ref(), home.path(), id).unwrap();

    assert!(matches!(
        &records[0],
        DetailRecord::Evidence(evidence) if evidence.native_type == "session_meta"
    ));
    assert!(matches!(
        &records[1],
        DetailRecord::Message(message) if message.role == ConversationRole::User && message.text == "inspect this"
    ));
    assert!(matches!(
        &records[2],
        DetailRecord::Message(message) if message.role == ConversationRole::Assistant && message.text == "I will inspect it."
    ));
    assert!(matches!(
        &records[3],
        DetailRecord::Tool(tool) if tool.name == "read_file" && tool.status == ToolActivityStatus::Started
    ));
    assert!(matches!(
        &records[4],
        DetailRecord::Tool(tool) if tool.call_id.as_deref() == Some("call-1") && tool.status == ToolActivityStatus::Completed
    ));
    assert!(matches!(
        &records[5],
        DetailRecord::Evidence(evidence) if evidence.status == "hidden_internal"
    ));
    assert!(matches!(
        &records[6],
        DetailRecord::Evidence(evidence) if evidence.status == "filtered"
    ));
    assert!(!records.iter().any(|record| matches!(
        record,
        DetailRecord::Message(message) if message.text == "internal" || message.text == "hidden"
    )));
}

#[test]
fn claude_thinking_is_diagnostic_without_hiding_visible_assistant_text() {
    let home = tempfile::tempdir().unwrap();
    write_jsonl(
        home.path(),
        ".claude/projects/example/thinking.jsonl",
        &[
            r#"{"timestamp":"2026-07-30T10:00:00Z","type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"private"},{"type":"output_text","text":"visible answer"}]}}"#,
        ],
    );
    let backend = backend_for(AgentKind::Claude);
    let records = detail_records_for_test(backend.as_ref(), home.path(), "thinking").unwrap();

    assert!(matches!(
        &records[0],
        DetailRecord::Message(message) if message.text == "visible answer"
    ));
    assert!(matches!(
        &records[1],
        DetailRecord::Evidence(evidence) if evidence.status == "hidden_internal"
    ));
}

#[test]
fn detail_stats_include_malformed_entries_and_snapshot() {
    let home = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        home.path(),
        ".claude/projects/example/partial.jsonl",
        &[
            r#"{"timestamp":"2026-07-30T10:00:00Z","type":"user","promptSource":"typed","message":{"role":"user","content":"hello"}}"#,
            "not-json",
        ],
    );
    let backend = backend_for(AgentKind::Claude);
    let mut meta_seen = false;
    let (_, stats, warnings) = stream_detail_data(
        backend.as_ref(),
        home.path(),
        "partial",
        &mut |_| {
            meta_seen = true;
            Ok(true)
        },
        &mut |_| Ok(true),
    )
    .unwrap();

    assert!(meta_seen);
    assert_eq!(stats.entry_count, 2);
    assert_eq!(stats.message_count, 1);
    assert_eq!(stats.malformed_count, 1);
    assert!(!stats.snapshot.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(path.exists());
}

#[test]
fn evidence_reads_utf8_and_base64_and_rejects_stale_or_hidden_entries() {
    let home = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        home.path(),
        ".claude/projects/example/evidence.jsonl",
        &[
            r#"{"timestamp":"2026-07-30T10:00:00Z","type":"user","promptSource":"typed","message":{"role":"user","content":"hello"}}"#,
            r#"{"timestamp":"2026-07-30T10:00:01Z","type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"private"}]}}"#,
        ],
    );
    let mut raw = std::fs::read(&path).unwrap();
    raw.extend_from_slice(b"\xff\n");
    std::fs::write(&path, raw).unwrap();
    let backend = backend_for(AgentKind::Claude);
    let (_, stats, _) = stream_detail_data(
        backend.as_ref(),
        home.path(),
        "evidence",
        &mut |_| Ok(true),
        &mut |_| Ok(true),
    )
    .unwrap();

    let visible = read_evidence(
        backend.as_ref(),
        home.path(),
        "evidence",
        "line-1",
        &stats.snapshot,
    )
    .unwrap();
    assert_eq!(visible.encoding, EvidenceEncoding::Utf8);
    assert!(visible.content.contains("hello"));
    assert!(
        read_evidence(
            backend.as_ref(),
            home.path(),
            "evidence",
            "line-2",
            &stats.snapshot
        )
        .is_err()
    );
    let binary = read_evidence(
        backend.as_ref(),
        home.path(),
        "evidence",
        "line-3",
        &stats.snapshot,
    )
    .unwrap();
    assert_eq!(binary.encoding, EvidenceEncoding::Base64);
    assert_eq!(binary.content, "/w==");

    std::fs::write(&path, b"changed\n").unwrap();
    let stale = read_evidence(
        backend.as_ref(),
        home.path(),
        "evidence",
        "line-1",
        &stats.snapshot,
    );
    assert!(stale.is_err());
}
