//! Claude transcript format: `<home>/.claude/projects/**/<uuid>.jsonl`.
//!
//! Each line is a JSON object. The fields we read:
//! - a top-level `timestamp` (first one seen = session start);
//! - `{"type":"ai-title","aiTitle":"…"}` — the agent-generated title;
//! - `{"type":"user", …, "message":{"content":"…"}}` — a prompt the user
//!   actually typed (as opposed to injected/tool turns). Accepted records use
//!   either `promptSource:"typed"` or `userType:"external"`; injected messages
//!   may carry `isMeta:true`. The text lives in the nested `message.content` (a
//!   plain string, or an array of text-bearing blocks), *not* a top-level
//!   `content`.
//!
//! The session id is just the transcript filename without `.jsonl`.

use crate::session::{PromptRecord, SessionBackend};
use serde_json::Value;
use std::path::Path;

/// Parser for Claude Code's on-disk transcript format.
pub struct Claude;

impl SessionBackend for Claude {
    fn session_dir_components(&self) -> &'static [&'static str] {
        &[".claude", "projects"]
    }

    /// Every `.jsonl` under `projects/` is a transcript.
    fn keep_transcript_name(&self, _name: &str) -> bool {
        true
    }

    fn id_of(&self, path: &Path) -> String {
        path.file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// A real prompt is a non-meta `type:user` turn with human text, identified
    /// by `promptSource:typed` or `userType:external`. Tool results have no text
    /// blocks. Feeds shared title selection and `get` paths.
    fn prompt_record(&self, value: &Value) -> PromptRecord {
        if value.get("type").and_then(Value::as_str) != Some("user") {
            return PromptRecord::NotTyped;
        }
        if value.get("isMeta").and_then(Value::as_bool) == Some(true) {
            return PromptRecord::NotTyped;
        }
        match value.get("promptSource").and_then(Value::as_str) {
            Some("typed") => content_record(value),
            Some(_) => PromptRecord::NotTyped,
            None if value.get("userType").and_then(Value::as_str) == Some("external") => {
                content_record(value)
            }
            None => PromptRecord::UnsupportedUserLike,
        }
    }

    /// Any line bearing a non-empty top-level `timestamp` is a candidate; the
    /// shared streaming loop keeps the first, which is the session start.
    fn start_ts_of(&self, value: &Value) -> Option<String> {
        value
            .get("timestamp")
            .and_then(Value::as_str)
            .filter(|timestamp| !timestamp.is_empty())
            .map(str::to_string)
    }

    /// Surface the agent-generated `ai-title` lines. A session can carry
    /// several (re-titled mid-run); the shared loop keeps the last non-empty
    /// one, falling back to the first typed prompt when there is none.
    fn title_of(&self, value: &Value) -> Option<String> {
        if value.get("type").and_then(Value::as_str) != Some("ai-title") {
            return None;
        }
        value
            .get("aiTitle")
            .and_then(Value::as_str)
            .map(str::to_string)
    }
}

/// Pull a user turn's text out of its `message.content` — Claude nests the turn
/// under a `message` object (`{"role":"user","content":…}`), not at the top level.
/// The content is typically a plain string; some turns use an array of blocks,
/// so we join supported text blocks, ignore known non-text blocks, and flag
/// unknown shapes without hiding text that was still readable.
fn content_record(value: &Value) -> PromptRecord {
    match value
        .get("message")
        .and_then(|message| message.get("content"))
    {
        Some(Value::String(text)) if !text.is_empty() => PromptRecord::Typed(text.clone()),
        Some(Value::Array(items)) => {
            if items.is_empty() {
                return PromptRecord::UnsupportedUserLike;
            }
            let mut parts = Vec::new();
            let mut unsupported = false;
            for item in items {
                match item.get("type").and_then(Value::as_str) {
                    Some("text") => match item.get("text").and_then(Value::as_str) {
                        Some(text) if !text.is_empty() => parts.push(text.to_string()),
                        _ => unsupported = true,
                    },
                    Some("image" | "document" | "tool_result") => {}
                    _ => unsupported = true,
                }
            }
            PromptRecord::from_text_parts(&parts, unsupported)
        }
        _ => PromptRecord::UnsupportedUserLike,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{only, write_jsonl};

    #[test]
    fn files_discovers_jsonl_transcripts_under_projects() {
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

        let files = Claude.files(dir.path()).unwrap();

        assert_eq!(files, vec![transcript]);
    }

    #[test]
    fn list_files_discovers_the_same_transcripts_tolerantly() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = write_jsonl(
            dir.path(),
            ".claude/projects/p/3f2a1b6c-0000-0000-0000-000000000000.jsonl",
            &[r#"{"type":"assistant"}"#],
        );

        let discovery = Claude.list_files(dir.path()).unwrap();

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
    fn summarize_prefers_ai_title() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let path = write_jsonl(
            home,
            ".claude/projects/p/3f2a1b6c-0000-0000-0000-000000000000.jsonl",
            &[
                r#"{"timestamp":"2026-07-14T02:16:00Z","type":"user","promptSource":"typed","message":{"role":"user","content":"first prompt"}}"#,
                r#"{"type":"ai-title","aiTitle":"A Nice Title"}"#,
                r#"{"type":"user","promptSource":"typed","message":{"role":"user","content":"second"}}"#,
            ],
        );
        let s = Claude.summarize(&path).unwrap();
        assert_eq!(s.title, "A Nice Title");
        assert_eq!(s.start_ts, "2026-07-14T02:16:00Z");
        assert!(s.id.starts_with("3f2a1b6c"));
    }

    #[test]
    fn summarize_uses_last_non_empty_ai_title() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_jsonl(
            dir.path(),
            ".claude/projects/p/3f2a1b6c-0000-0000-0000-000000000001.jsonl",
            &[
                r#"{"timestamp":"2026-07-14T02:16:00Z","type":"user","promptSource":"typed","message":{"role":"user","content":"fallback prompt"}}"#,
                r#"{"type":"ai-title","aiTitle":"Draft Title"}"#,
                r#"{"type":"ai-title","aiTitle":""}"#,
                r#"{"type":"ai-title","aiTitle":"Final Title"}"#,
            ],
        );

        let s = Claude.summarize(&path).unwrap();

        assert_eq!(s.title, "Final Title");
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
}
