//! Claude transcript format: `<home>/.claude/projects/*/<uuid>.jsonl`.
//!
//! Each line is a JSON object. The fields we read:
//! - a top-level `timestamp` (first one seen = session start);
//! - `{"type":"ai-title","aiTitle":"…"}` — the agent-generated title;
//! - `{"type":"user","promptSource":"typed", …, "message":{"content":"…"}}` — a
//!   prompt the user actually typed (as opposed to injected/tool turns). The text
//!   lives in the nested `message.content` (a plain string, or a block array),
//!   *not* a top-level `content`. `promptSource` marks turns that count as typed
//!   prompts.
//!
//! The session id is just the transcript filename without `.jsonl`.

use crate::session::SessionBackend;
use serde_json::Value;
use std::path::Path;

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
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string()
    }

    /// A real prompt is a `type:user` turn the human typed (`promptSource:typed`),
    /// with a non-empty `message.content`. Feeds shared title selection and
    /// `get` paths.
    fn typed_text(&self, v: &Value) -> Option<String> {
        if v.get("type").and_then(Value::as_str) != Some("user") || !is_typed(v) {
            return None;
        }
        content_text(v)
    }

    /// Any line bearing a non-empty top-level `timestamp` is a candidate; the
    /// shared streaming loop keeps the first, which is the session start.
    fn start_ts_of(&self, v: &Value) -> Option<String> {
        v.get("timestamp")
            .and_then(Value::as_str)
            .filter(|ts| !ts.is_empty())
            .map(str::to_string)
    }

    /// Surface the agent-generated `ai-title` lines. A session can carry
    /// several (re-titled mid-run); the shared loop keeps the last non-empty
    /// one, falling back to the first typed prompt when there is none.
    fn title_of(&self, v: &Value) -> Option<String> {
        (v.get("type").and_then(Value::as_str) == Some("ai-title"))
            .then(|| v.get("aiTitle").and_then(Value::as_str))
            .flatten()
            .map(str::to_string)
    }
}

/// True for a user turn the human actually typed (`"promptSource":"typed"`).
fn is_typed(v: &Value) -> bool {
    v.get("promptSource").and_then(Value::as_str) == Some("typed")
}

/// Pull a user turn's text out of its `message.content` — Claude nests the turn
/// under a `message` object (`{"role":"user","content":…}`), not at the top level.
/// The content is typically a plain string; some turns use the block array form
/// `[{"type":"text","text":"…"}]`, so we handle both and join text blocks with
/// newlines. Returns `None` if the `message.content` is absent or empty.
fn content_text(v: &Value) -> Option<String> {
    match v.get("message").and_then(|m| m.get("content")) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::Array(items)) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(content_block_text)
                .map(str::to_string)
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        _ => None,
    }
}

fn content_block_text(item: &Value) -> Option<&str> {
    item.get("text")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::write_jsonl;

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
        // `list` uses the tolerant walk so one bad path can't hide every
        // readable session; on a healthy tree it must still agree with `files`.
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
        // A fresh profile home has no `.claude/projects` yet. Both discovery
        // paths must report empty rather than erroring on the missing tree.
        let dir = tempfile::tempdir().unwrap();

        assert!(Claude.files(dir.path()).unwrap().is_empty());
        let discovery = Claude.list_files(dir.path()).unwrap();
        assert!(discovery.files.is_empty());
        assert!(discovery.errors.is_empty());
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

        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].text, "real prompt");
        assert_eq!(summary.title, "real prompt");
    }

    #[test]
    fn sessions_without_typed_prompts_still_summarize_with_empty_title() {
        // No `promptSource:typed` line, so no title — but the session still
        // summarizes (empty title) so `list`/`delete` can see and clear it.
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
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text, "line1\nline2 caf\u{e9}");
        assert_eq!(ps[0].timestamp, "2026-07-14T09:00:00Z");
    }

    #[test]
    fn content_block_array_form() {
        let v: Value = serde_json::from_str(
            r#"{"message":{"role":"user","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}"#,
        )
        .unwrap();
        assert_eq!(content_text(&v).as_deref(), Some("a\nb"));
    }

    /// A real image-paste turn: the content array mixes a non-text block (an
    /// `image`, which carries no `text` field) with the typed text block. Only
    /// the text survives, through the public `prompts` path — not just the
    /// `content_text` helper. This also pins Claude's divergence from Codex:
    /// Claude pulls `text` from any block regardless of `type`, where Codex
    /// filters to `input_text`/`text`. A change either direction should fail here.
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

        assert_eq!(prompts.len(), 1);
        assert_eq!(
            prompts[0].text, "what is this",
            "the image block contributes no text; only the typed text block remains"
        );
    }
}
