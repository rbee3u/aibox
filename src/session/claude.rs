//! Claude transcript format: `<home>/.claude/projects/*/<uuid>.jsonl`.
//!
//! Each line is a JSON object. The fields we read:
//! - a top-level `timestamp` (first one seen = session start);
//! - `{"type":"ai-title","aiTitle":"…"}` — the agent-generated title;
//! - `{"type":"user","promptSource":"typed", …, "content":"…"}` — a prompt the
//!   user actually typed (as opposed to injected/tool turns). `promptSource` is
//!   what lets us list only real chats.
//!
//! The session id is just the transcript filename without `.jsonl`.

use super::SessionBackend;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct Claude;

impl SessionBackend for Claude {
    fn files(&self, home: &Path) -> Vec<PathBuf> {
        let base = home.join(".claude").join("projects");
        super::walk_jsonl(&base, |_| true)
    }

    fn id_of(&self, path: &Path) -> String {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string()
    }

    /// A real prompt is a `type:user` turn the human typed (`promptSource:typed`),
    /// with a non-empty `content`. The shared loops in [`SessionBackend`] use this
    /// for both `list` counting and `get`.
    fn typed_text(&self, v: &Value) -> Option<String> {
        if v.get("type").and_then(Value::as_str) != Some("user") || !is_typed(v) {
            return None;
        }
        content_text(v)
    }

    /// The first line bearing a top-level `timestamp` is the session start.
    fn start_ts(&self, lines: &[Value]) -> String {
        lines
            .iter()
            .find_map(|v| v.get("timestamp").and_then(Value::as_str))
            .unwrap_or("")
            .to_string()
    }

    /// Prefer the agent-generated `ai-title`; fall back to the first typed prompt.
    /// A session can carry several `ai-title` lines (re-titled mid-run); the last
    /// non-empty one wins.
    fn title(&self, lines: &[Value], first_prompt: &str) -> String {
        lines
            .iter()
            .rev()
            .filter_map(|v| {
                (v.get("type").and_then(Value::as_str) == Some("ai-title"))
                    .then(|| v.get("aiTitle").and_then(Value::as_str))
                    .flatten()
            })
            .find(|t| !t.is_empty())
            .unwrap_or(first_prompt)
            .to_string()
    }
}

/// True for a user turn the human actually typed (`"promptSource":"typed"`).
fn is_typed(v: &Value) -> bool {
    v.get("promptSource").and_then(Value::as_str) == Some("typed")
}

/// Pull a user turn's text out of its `content`. Claude typically stores a plain
/// string; some turns use the block array form `[{"type":"text","text":"…"}]`, so
/// we handle both and join text blocks with newlines. Returns `None` if empty.
fn content_text(v: &Value) -> Option<String> {
    match v.get("content") {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::Array(items)) => {
            let mut parts = Vec::new();
            for it in items {
                if let Some(t) = it.get("text").and_then(Value::as_str) {
                    if !t.is_empty() {
                        parts.push(t.to_string());
                    }
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(parts.join("\n"))
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_jsonl(dir: &Path, rel: &str, lines: &[&str]) -> PathBuf {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(&path).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        path
    }

    #[test]
    fn summarize_prefers_ai_title() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let path = write_jsonl(
            home,
            ".claude/projects/p/3f2a1b6c-0000-0000-0000-000000000000.jsonl",
            &[
                r#"{"timestamp":"2026-07-14T02:16:00Z","type":"user","promptSource":"typed","content":"first prompt"}"#,
                r#"{"type":"ai-title","aiTitle":"A Nice Title"}"#,
                r#"{"type":"user","promptSource":"typed","content":"second"}"#,
            ],
        );
        let s = Claude.summarize(&path).unwrap();
        assert_eq!(s.title, "A Nice Title");
        assert_eq!(s.start_ts, "2026-07-14T02:16:00Z");
        assert!(s.id.starts_with("3f2a1b6c"));
    }

    #[test]
    fn summarize_falls_back_to_first_typed() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_jsonl(
            dir.path(),
            ".claude/projects/p/aaaa.jsonl",
            &[
                r#"{"timestamp":"2026-01-01T00:00:00Z","type":"user","promptSource":"typed","content":"only prompt"}"#,
            ],
        );
        let s = Claude.summarize(&path).unwrap();
        assert_eq!(s.title, "only prompt");
    }

    #[test]
    fn sessions_without_typed_prompts_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_jsonl(
            dir.path(),
            ".claude/projects/p/bbbb.jsonl",
            &[
                r#"{"timestamp":"2026-01-01T00:00:00Z","type":"user","content":"injected"}"#,
                r#"{"type":"assistant","content":"hi"}"#,
            ],
        );
        assert!(Claude.summarize(&path).is_none());
    }

    #[test]
    fn prompts_decodes_unicode_and_escapes() {
        let dir = tempfile::tempdir().unwrap();
        // 测试 = 测试; embedded newline escape.
        let path = write_jsonl(
            dir.path(),
            ".claude/projects/p/cccc.jsonl",
            &[
                r#"{"type":"user","promptSource":"typed","timestamp":"2026-07-14T09:00:00Z","content":"line1\nline2 测试"}"#,
            ],
        );
        let ps = Claude.prompts(&path);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text, "line1\nline2 测试");
        assert_eq!(ps[0].timestamp, "2026-07-14T09:00:00Z");
    }

    #[test]
    fn content_block_array_form() {
        let v: Value = serde_json::from_str(
            r#"{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}"#,
        )
        .unwrap();
        assert_eq!(content_text(&v).as_deref(), Some("a\nb"));
    }
}
