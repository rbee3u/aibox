use super::*;
use crate::testutil::{only, write_jsonl};

#[test]
fn strict_and_tolerant_discovery_keep_only_rollout_jsonl_transcripts() {
    let dir = tempfile::tempdir().unwrap();
    let rollout = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-3f2a1b6c-1111-2222-3333-444455556666.jsonl",
        &[r#"{"type":"session_meta"}"#],
    );
    write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/session-x-ignored.jsonl",
        &[r#"{"type":"session_meta"}"#],
    );
    std::fs::write(
        dir.path()
            .join(".codex/sessions/2026/07/14/rollout-x-ignored.txt"),
        "{}\n",
    )
    .unwrap();

    let discovery = Codex.list_files(dir.path()).unwrap();

    assert_eq!(Codex.files(dir.path()).unwrap(), vec![rollout.clone()]);
    assert_eq!(discovery.files, vec![rollout]);
    assert!(discovery.errors.is_empty());
}

#[test]
fn list_files_and_files_are_empty_before_the_first_codex_run() {
    let dir = tempfile::tempdir().unwrap();

    assert!(Codex.files(dir.path()).unwrap().is_empty());
    let discovery = Codex.list_files(dir.path()).unwrap();
    assert!(discovery.files.is_empty());
    assert!(discovery.errors.is_empty());
}

#[cfg(unix)]
#[test]
fn non_utf8_rollout_names_have_a_lossy_addressable_id() {
    use std::os::unix::ffi::OsStringExt;

    let transcript = std::path::PathBuf::from(std::ffi::OsString::from_vec(
        b"rollout-session-\xff.jsonl".to_vec(),
    ));

    assert_eq!(Codex.id_of(&transcript), "rollout-session-\u{fffd}");
}

#[test]
fn id_uses_a_trailing_uuid_and_otherwise_preserves_the_stem() {
    for (path, expected) in [
        (
            "/h/.codex/sessions/2026/07/14/rollout-2026-07-14T02-16-00-3f2a1b6c-1111-2222-3333-444455556666.jsonl",
            "3f2a1b6c-1111-2222-3333-444455556666",
        ),
        (
            "/h/.codex/sessions/rollout-this-name-is-longer-than-a-uuid-but-has-no-session-id.jsonl",
            "rollout-this-name-is-longer-than-a-uuid-but-has-no-session-id",
        ),
        ("/h/.codex/sessions/rollout-short.jsonl", "rollout-short"),
        ("/h/.codex/sessions/x.jsonl", "x"),
    ] {
        assert_eq!(Codex.id_of(Path::new(path)), expected, "{path}");
    }
}

#[test]
fn summarize_uses_first_real_prompt_and_meta_ts() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-aaaaaaaa-1111-2222-3333-444455556666.jsonl",
        &[
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"session_meta","payload":{}}"#,
            r#"{"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"real question"}]}}"#,
        ],
    );
    let s = Codex.summarize(&path).unwrap();
    assert_eq!(s.start_ts, "2026-07-14T02:16:00Z");
    assert_eq!(s.title, "real question");
}

#[test]
fn summarize_prefers_session_meta_payload_timestamp() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-abababab-1111-2222-3333-444455556666.jsonl",
        &[
            r#"{"timestamp":"2026-07-14T02:16:29Z","type":"session_meta","payload":{"timestamp":"2026-07-14T02:16:00Z"}}"#,
        ],
    );

    let summary = Codex.summarize(&path).unwrap();

    assert_eq!(summary.start_ts, "2026-07-14T02:16:00Z");
}

#[test]
fn is_wrapper_text_matches_all_branches() {
    for (case, text) in [
        (
            "environment context",
            "<environment_context>cwd=/work</environment_context>",
        ),
        (
            "indented environment context",
            "\n  <environment_context>cwd=/work</environment_context>",
        ),
        (
            "user instructions",
            "<user_instructions>be nice</user_instructions>",
        ),
        ("app context", "<app-context>x</app-context>"),
        (
            "application instructions",
            "<apps_instructions>x</apps_instructions>",
        ),
        ("paired user shell", "<user_shell name=\"ls\"></user_shell>"),
        ("self-closing user shell", "<user_shell name=\"ls\" />"),
        ("instructions", "<INSTRUCTIONS>x</INSTRUCTIONS>"),
        ("skill", "<skill>x</skill>"),
        (
            "permissions",
            "<permissions instructions>x</permissions instructions>",
        ),
        (
            "plugin instructions",
            "<plugins_instructions>x</plugins_instructions>",
        ),
        (
            "skill instructions",
            "<skills_instructions>x</skills_instructions>",
        ),
        (
            "collaboration mode",
            "<collaboration_mode>x</collaboration_mode>",
        ),
        (
            "recommended plugins",
            "<recommended_plugins>x</recommended_plugins>",
        ),
        ("environment heading", "## My env\nlinux"),
        ("indented environment heading", "\n  ## My env\nlinux"),
        (
            "instruction preamble",
            "# Base instructions for gpt-5.5\nmore",
        ),
        (
            "indented instruction preamble",
            "  # Base instructions for gpt-5.5\nmore",
        ),
    ] {
        assert!(is_wrapper_text(text), "{case} should be filtered: {text:?}");
    }

    for (case, text) in [
        ("ordinary heading", "# just a heading"),
        (
            "instruction phrase after the first line",
            "preamble\n# instructions for x",
        ),
        (
            "unterminated wrapper-like text",
            "<environment_context>literal prompt",
        ),
        (
            "real prompt after wrapper",
            "<environment_context>cwd=/work</environment_context>\nreal ask",
        ),
        ("longer environment heading", "## My env is literal text"),
        ("plain prompt", "the real ask"),
    ] {
        assert!(
            !is_wrapper_text(text),
            "{case} must remain visible as user text: {text:?}"
        );
    }
}

#[test]
fn injected_wrapper_turns_are_filtered() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-bbbbbbbb-1111-2222-3333-444455556666.jsonl",
        &[
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"session_meta","payload":{}}"#,
            // A turn bundling an injected env block + the real prompt.
            r#"{"type":"response_item","payload":{"role":"user","content":[{"type":"text","text":"<environment_context>cwd=/work</environment_context>"},{"type":"input_text","text":"the real ask"}]}}"#,
        ],
    );
    let ps = Codex.prompts(&path).unwrap();
    assert_eq!(only(&ps).text, "the real ask");
}

#[test]
fn wrapper_prefix_in_one_text_item_keeps_trailing_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-bcbcbcbc-1111-2222-3333-444455556666.jsonl",
        &[
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"session_meta","payload":{}}"#,
            r##"{"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"<recommended_plugins>x</recommended_plugins>\n# AGENTS.md instructions for /work\n\n<INSTRUCTIONS>ignored</INSTRUCTIONS>\nreal ask"}]}}"##,
        ],
    );

    let ps = Codex.prompts(&path).unwrap();
    let summary = Codex.summarize(&path).unwrap();

    assert_eq!(only(&ps).text, "real ask");
    assert_eq!(summary.title, "real ask");
}

#[test]
fn user_shell_prefix_in_one_text_item_keeps_trailing_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-bdbdbdbd-1111-2222-3333-444455556666.jsonl",
        &[
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"session_meta","payload":{}}"#,
            r##"{"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"<user_shell name=\"pwd\" />\nreal ask after shell"}]}}"##,
        ],
    );

    let ps = Codex.prompts(&path).unwrap();
    let summary = Codex.summarize(&path).unwrap();

    assert_eq!(only(&ps).text, "real ask after shell");
    assert_eq!(summary.title, "real ask after shell");
}

#[test]
fn non_wrapper_text_content_is_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-99999999-1111-2222-3333-444455556666.jsonl",
        &[
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"response_item","payload":{"role":"user","content":[{"type":"text","text":"plain text prompt"},{"type":"input_text","text":"typed prompt"}]}}"#,
        ],
    );

    let ps = Codex.prompts(&path).unwrap();

    let prompt = only(&ps);
    assert_eq!(prompt.text, "plain text prompt\ntyped prompt");
    assert_eq!(prompt.timestamp, "2026-07-14T02:16:00Z");
}

#[test]
fn unsupported_user_content_items_are_not_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-13131313-1111-2222-3333-444455556666.jsonl",
        &[
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"response_item","payload":{"role":"user","content":[{"type":"output_text","text":"tool echo"},{"type":"input_image","text":"image alt"},{"type":"input_text","text":"real ask"}]}}"#,
        ],
    );

    let ps = Codex.prompts(&path).unwrap();
    let summary = Codex.summarize(&path).unwrap();

    assert_eq!(only(&ps).text, "real ask");
    assert_eq!(summary.title, "real ask");
}

#[test]
fn unknown_user_record_shape_is_diagnostic() {
    let unknown = serde_json::json!({
        "type": "response_item",
        "payload": {
            "role": "user",
            "content": [{"type": "future_input", "value": "ask"}]
        }
    });
    assert_eq!(
        Codex.prompt_record(&unknown),
        PromptRecord::UnsupportedUserLike
    );

    let partially_supported = serde_json::json!({
        "type": "response_item",
        "payload": {
            "role": "user",
            "content": [
                {"type": "input_text", "text": "visible ask"},
                {"type": "future_input", "value": "unreadable suffix"}
            ]
        }
    });
    assert_eq!(
        Codex.prompt_record(&partially_supported),
        PromptRecord::TypedWithUnsupported("visible ask".to_string())
    );
}

#[test]
fn assistant_response_items_are_not_prompts() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-12121212-1111-2222-3333-444455556666.jsonl",
        &[
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"session_meta","payload":{}}"#,
            r#"{"timestamp":"2026-07-14T02:17:00Z","type":"response_item","payload":{"role":"assistant","content":[{"type":"text","text":"assistant answer"}]}}"#,
            r#"{"timestamp":"2026-07-14T02:18:00Z","type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"real ask"}]}}"#,
        ],
    );

    let ps = Codex.prompts(&path).unwrap();
    let summary = Codex.summarize(&path).unwrap();

    assert_eq!(only(&ps).text, "real ask");
    assert_eq!(summary.title, "real ask");
}

#[test]
fn injected_input_text_wrappers_are_filtered() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-dddddddd-1111-2222-3333-444455556666.jsonl",
        &[
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"session_meta","payload":{}}"#,
            r##"{"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions for /work\n\n<INSTRUCTIONS>\nignored\n</INSTRUCTIONS>"},{"type":"input_text","text":"<environment_context>\n  <cwd>/work</cwd>\n</environment_context>"}]}}"##,
            r#"{"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"<skill>\nignored\n</skill>"}]}}"#,
            r#"{"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"first real ask"}]}}"#,
        ],
    );

    let ps = Codex.prompts(&path).unwrap();
    let summary = Codex.summarize(&path).unwrap();

    assert_eq!(only(&ps).text, "first real ask");
    assert_eq!(summary.title, "first real ask");
}

#[test]
fn summarize_uses_session_meta_timestamp_not_parsed_line_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-eeeeeeee-1111-2222-3333-444455556666.jsonl",
        &[
            "not json",
            r#"{"timestamp":"2026-07-14T02:17:00Z","type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"real question"}]}}"#,
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"session_meta","payload":{}}"#,
        ],
    );

    let s = Codex.summarize(&path).unwrap();

    assert_eq!(s.start_ts, "2026-07-14T02:16:00Z");
    assert_eq!(s.title, "real question");
}

#[test]
fn summarize_falls_back_to_first_timestamp_without_session_meta() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-ffffffff-1111-2222-3333-444455556666.jsonl",
        &[
            r#"{"timestamp":"2026-07-14T02:18:00Z","type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"real question"}]}}"#,
            r#"{"timestamp":"2026-07-14T02:19:00Z","type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"second"}]}}"#,
        ],
    );

    let s = Codex.summarize(&path).unwrap();

    assert_eq!(s.start_ts, "2026-07-14T02:18:00Z");
}

#[test]
fn turn_that_is_all_wrapper_yields_no_prompts_but_still_summarizes() {
    // Every user turn is an injected wrapper, so no real prompt survives —
    // but the session still summarizes (empty title, meta ts) so `list` and
    // `delete --all` can see and clear it.
    let dir = tempfile::tempdir().unwrap();
    let path = write_jsonl(
        dir.path(),
        ".codex/sessions/2026/07/14/rollout-x-cccccccc-1111-2222-3333-444455556666.jsonl",
        &[
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"session_meta","payload":{}}"#,
            r#"{"type":"response_item","payload":{"role":"user","content":[{"type":"text","text":"<user_instructions>be nice</user_instructions>"}]}}"#,
        ],
    );
    let s = Codex.summarize(&path).unwrap();
    assert_eq!(s.title, "");
    assert_eq!(s.start_ts, "2026-07-14T02:16:00Z");
    assert!(Codex.prompts(&path).unwrap().is_empty());
}
