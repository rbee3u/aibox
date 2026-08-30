use super::*;
use crate::testutil::{only, write_jsonl};

#[test]
fn strict_and_tolerant_discovery_find_only_jsonl_under_projects() {
    let dir = tempfile::tempdir().unwrap();
    let transcript = write_jsonl(
        dir.path(),
        ".claude/projects/p/3f2a1b6c-0000-0000-0000-000000000000.jsonl",
        &[r#"{"type":"assistant"}"#],
    );
    write_jsonl(
        dir.path(),
        ".claude/not-projects/ignored.jsonl",
        &[r#"{"type":"assistant"}"#],
    );
    std::fs::write(
        dir.path().join(".claude/projects/p/not-a-transcript.txt"),
        "{}\n",
    )
    .unwrap();

    let discovery = Claude.list_files(dir.path()).unwrap();

    assert_eq!(Claude.files(dir.path()).unwrap(), vec![transcript.clone()]);
    assert_eq!(discovery.files, vec![transcript]);
    assert!(discovery.errors.is_empty());
}

#[test]
fn list_files_and_files_are_empty_before_the_first_claude_run() {
    let dir = tempfile::tempdir().unwrap();

    assert!(Claude.files(dir.path()).unwrap().is_empty());
    let discovery = Claude.list_files(dir.path()).unwrap();
    assert!(discovery.files.is_empty());
    assert!(discovery.errors.is_empty());
}

#[cfg(unix)]
#[test]
fn non_utf8_transcript_names_have_a_lossy_addressable_id() {
    use std::os::unix::ffi::OsStringExt;

    let transcript =
        std::path::PathBuf::from(std::ffi::OsString::from_vec(b"session-\xff.jsonl".to_vec()));

    assert_eq!(Claude.id_of(&transcript), "session-\u{fffd}");
}

#[test]
fn summarize_prefers_the_last_non_empty_ai_title() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let path = write_jsonl(
        home,
        ".claude/projects/p/3f2a1b6c-0000-0000-0000-000000000000.jsonl",
        &[
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"user","promptSource":"typed","message":{"role":"user","content":"first prompt"}}"#,
            r#"{"type":"ai-title","aiTitle":"Draft Title"}"#,
            r#"{"type":"ai-title","aiTitle":"Final Title"}"#,
            r#"{"type":"ai-title","aiTitle":""}"#,
            r#"{"type":"user","promptSource":"typed","message":{"role":"user","content":"second"}}"#,
        ],
    );
    let s = Claude.summarize(&path).unwrap();
    assert_eq!(s.title, "Final Title");
    assert_eq!(s.start_ts, "2026-07-14T02:16:00Z");
    assert!(s.id.starts_with("3f2a1b6c"));
}

#[test]
fn summarize_ignores_empty_timestamps() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".claude/projects/p/3f2a1b6c-0000-0000-0000-000000000002.jsonl",
        &[
            r#"{"timestamp":"","type":"assistant"}"#,
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"user","promptSource":"typed","message":{"role":"user","content":"first prompt"}}"#,
        ],
    );

    let s = Claude.summarize(&path).unwrap();

    assert_eq!(s.start_ts, "2026-07-14T02:16:00Z");
}

#[test]
fn summarize_falls_back_to_first_typed() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".claude/projects/p/aaaa.jsonl",
        &[
            r#"{"timestamp":"2026-01-01T00:00:00Z","type":"user","promptSource":"typed","message":{"role":"user","content":"only prompt"}}"#,
        ],
    );
    let s = Claude.summarize(&path).unwrap();
    assert_eq!(s.title, "only prompt");
}

#[test]
fn prompts_ignore_non_typed_user_sources() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".claude/projects/p/aaaa-bbbb.jsonl",
        &[
            r#"{"timestamp":"2026-01-01T00:00:00Z","type":"user","promptSource":"tool","message":{"role":"user","content":"tool echo"}}"#,
            r#"{"timestamp":"2026-01-01T00:01:00Z","type":"user","promptSource":"typed","message":{"role":"user","content":"real prompt"}}"#,
        ],
    );

    let prompts = Claude.prompts(&path).unwrap();
    let summary = Claude.summarize(&path).unwrap();

    assert_eq!(only(&prompts).text, "real prompt");
    assert_eq!(summary.title, "real prompt");
}

#[test]
fn unknown_user_shape_is_diagnostic_but_known_non_prompt_is_not() {
    let unknown = serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": "unknown"}
    });
    let tool = serde_json::json!({
        "type": "user",
        "promptSource": "tool",
        "message": {"role": "user", "content": "tool output"}
    });
    assert_eq!(
        Claude.prompt_record(&unknown),
        PromptRecord::UnsupportedUserLike
    );
    assert_eq!(Claude.prompt_record(&tool), PromptRecord::NotTyped);
}

#[test]
fn prompts_accept_current_unmarked_user_messages_but_reject_meta_and_tool_results() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".claude/projects/p/current.jsonl",
        &[
            r#"{"type":"user","userType":"external","isMeta":true,"message":{"role":"user","content":"injected context"}}"#,
            r#"{"type":"user","userType":"external","message":{"role":"user","content":[{"type":"tool_result","content":"tool output"}]}}"#,
            r#"{"timestamp":"2026-07-14T09:00:00Z","type":"user","userType":"external","message":{"role":"user","content":"current real prompt"}}"#,
        ],
    );

    let prompts = Claude.prompts(&path).unwrap();
    let summary = Claude.summarize(&path).unwrap();

    assert_eq!(only(&prompts).text, "current real prompt");
    assert_eq!(summary.title, "current real prompt");
}

#[test]
fn sessions_without_typed_prompts_still_summarize_with_empty_title() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".claude/projects/p/bbbb.jsonl",
        &[
            r#"{"timestamp":"2026-01-01T00:00:00Z","type":"user","message":{"role":"user","content":"injected"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":"hi"}}"#,
        ],
    );
    let s = Claude.summarize(&path).unwrap();
    assert_eq!(s.title, "");
    assert_eq!(s.start_ts, "2026-01-01T00:00:00Z");
    assert!(Claude.prompts(&path).unwrap().is_empty());
}

#[test]
fn prompts_decodes_unicode_and_escapes() {
    let dir = tempfile::tempdir().unwrap();
    // Unicode escape plus an embedded newline escape.
    let path = write_jsonl(
        dir.path(),
        ".claude/projects/p/cccc.jsonl",
        &[
            r#"{"type":"user","promptSource":"typed","timestamp":"2026-07-14T09:00:00Z","message":{"role":"user","content":"line1\nline2 caf\u00e9"}}"#,
        ],
    );
    let ps = Claude.prompts(&path).unwrap();
    let prompt = only(&ps);
    assert_eq!(prompt.text, "line1\nline2 caf\u{e9}");
    assert_eq!(prompt.timestamp, "2026-07-14T09:00:00Z");
}

#[test]
fn content_block_array_form() {
    let v: Value = serde_json::from_str(
        r#"{"message":{"role":"user","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}"#,
    )
    .unwrap();
    assert_eq!(content_record(&v), PromptRecord::Typed("a\nb".to_string()));

    for content in [
        serde_json::json!([]),
        serde_json::json!([{"type": "image"}, {"type": "text", "text": ""}]),
        serde_json::json!({"type": "text", "text": "not an array"}),
    ] {
        let value = serde_json::json!({"message": {"content": content}});
        assert_eq!(
            content_record(&value),
            PromptRecord::UnsupportedUserLike,
            "unsupported or empty content must not become a typed prompt: {value}"
        );
    }
}

#[test]
fn unknown_content_blocks_are_diagnostic_without_hiding_readable_text() {
    let partial = serde_json::json!({
        "message": {
            "content": [
                {"type": "text", "text": "visible ask"},
                {"type": "future_block", "text": "do not trust this shape"}
            ]
        }
    });
    assert_eq!(
        content_record(&partial),
        PromptRecord::TypedWithUnsupported("visible ask".to_string())
    );

    let unknown = serde_json::json!({
        "message": {"content": [{"type": "future_block", "text": "hidden"}]}
    });
    assert_eq!(content_record(&unknown), PromptRecord::UnsupportedUserLike);
}

#[test]
fn typed_prompt_with_mixed_content_array_keeps_only_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".claude/projects/p/3f2a1b6c-0000-0000-0000-000000000000.jsonl",
        &[
            r#"{"type":"user","promptSource":"typed","timestamp":"2026-07-14T02:16:00Z","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","data":"iVBORw0KGgo="}},{"type":"text","text":"what is this"}]}}"#,
        ],
    );

    let prompts = Claude.prompts(&path).unwrap();

    let prompt = only(&prompts);
    assert_eq!(
        prompt.text, "what is this",
        "the image block contributes no text; only the typed text block remains"
    );
}
