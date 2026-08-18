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
fn list_and_stream_prompt_data_expose_structured_session_state() {
    let home = tempfile::tempdir().unwrap();
    let path = fixture(home.path());
    let backend = backend_for(AgentKind::Claude);
    let listed = list_data(backend.as_ref(), home.path()).unwrap();
    assert_eq!(listed.sessions.len(), 1);
    assert_eq!(listed.sessions[0].title, "first prompt");
    let id = listed.sessions[0].id.clone();
    let mut prompts = Vec::new();
    let (resolved, warnings) =
        stream_prompt_data(backend.as_ref(), home.path(), &id, &mut |prompt| {
            prompts.push(prompt.text);
            Ok(true)
        })
        .unwrap();
    assert_eq!(resolved, id);
    assert!(warnings.is_empty());
    assert_eq!(prompts, ["first prompt", "second prompt"]);
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

#[test]
fn canonical_uuid_display_uses_a_short_suffix() {
    let id = "12345678-1234-1234-1234-123456789abc";
    assert!(is_canonical_uuid(id));
    assert!(!is_canonical_uuid("short"));
}
