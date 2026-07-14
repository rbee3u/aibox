//! Codex transcript format:
//! `<home>/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`.
//!
//! Mapped from the codex-rs `rollout` crate: each line is a `RolloutLine` that
//! flattens a top-level `timestamp` + `type` + `payload`. Line 1 is a
//! `session_meta` (its `timestamp` is the session start). User turns are
//! `response_item` messages with `role:"user"` whose `payload.content` is an
//! array of `{type:"input_text"|"text", text:"…"}` items.
//!
//! Codex has no ai-title, so a session's preview is its first *real* prompt. It
//! also records injected wrapper turns (environment/instructions context blocks,
//! `!`-shell commands, the per-project AGENTS.md preamble) as user turns; those
//! are filtered by [`SKIP_RE`] applied to each content item. A turn left with no
//! text after filtering is skipped, so `list`/`get` stay the user's actual chats.
//!
//! The session id is the trailing uuid of the filename (last 36 chars of the
//! stem after `rollout-<date>-`).

use super::{Prompt, SessionBackend, SessionSummary};
use regex::Regex;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Injected wrapper turns Codex records as user turns but that the user never
/// typed. Applied to each content item; matching items are dropped. Ported
/// verbatim from the Bash `CODEX_SKIP_RE`.
static SKIP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(<environment_context>|<user_instructions>|<user_shell|<INSTRUCTIONS>|#[^\n]* instructions for |## My env)",
    )
    .expect("valid skip regex")
});

pub struct Codex;

impl SessionBackend for Codex {
    fn files(&self, home: &Path) -> Vec<PathBuf> {
        let base = home.join(".codex").join("sessions");
        if !base.is_dir() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for entry in walkdir::WalkDir::new(&base).into_iter().flatten() {
            let p = entry.path();
            if p.is_file()
                && p.extension().is_some_and(|e| e == "jsonl")
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("rollout-"))
            {
                out.push(p.to_path_buf());
            }
        }
        out
    }

    fn id_of(&self, path: &Path) -> String {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        // The uuid is the trailing 36 chars of the stem (rollout-<date>-<uuid>).
        let chars: Vec<char> = stem.chars().collect();
        if chars.len() >= 36 {
            chars[chars.len() - 36..].iter().collect()
        } else {
            stem.to_string()
        }
    }

    fn summarize(&self, path: &Path) -> Option<SessionSummary> {
        let text = std::fs::read_to_string(path).ok()?;
        let mut start_ts = String::new();
        let mut first_typed = String::new();
        let mut typed = 0u32;

        for (i, line) in text.lines().enumerate() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if i == 0 {
                start_ts = v
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
            }
            if let Some(t) = user_turn_text(&v) {
                typed += 1;
                if first_typed.is_empty() {
                    first_typed = t;
                }
            }
        }

        if typed == 0 {
            return None;
        }
        Some(SessionSummary {
            id: self.id_of(path),
            start_ts,
            title: first_typed,
        })
    }

    fn prompts(&self, path: &Path) -> Vec<Prompt> {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            if let Some(t) = user_turn_text(&v) {
                let timestamp = v
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                out.push(Prompt { timestamp, text: t });
            }
        }
        out
    }
}

/// If `v` is a `response_item` user message, join its content items' text with
/// newlines, dropping any item that matches [`SKIP_RE`] (an injected wrapper).
/// Returns `None` when `v` isn't a user turn or nothing real survives filtering.
fn user_turn_text(v: &Value) -> Option<String> {
    if v.get("type").and_then(Value::as_str) != Some("response_item") {
        return None;
    }
    let payload = v.get("payload")?;
    if payload.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let items = payload.get("content").and_then(Value::as_array)?;
    let mut parts = Vec::new();
    for it in items {
        if let Some(t) = it.get("text").and_then(Value::as_str) {
            if !t.is_empty() && !SKIP_RE.is_match(t) {
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
    fn id_is_trailing_uuid() {
        let p = Path::new(
            "/h/.codex/sessions/2026/07/14/rollout-2026-07-14T02-16-00-3f2a1b6c-1111-2222-3333-444455556666.jsonl",
        );
        assert_eq!(Codex.id_of(p), "3f2a1b6c-1111-2222-3333-444455556666");
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
        let ps = Codex.prompts(&path);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text, "the real ask");
    }

    #[test]
    fn turn_that_is_all_wrapper_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_jsonl(
            dir.path(),
            ".codex/sessions/2026/07/14/rollout-x-cccccccc-1111-2222-3333-444455556666.jsonl",
            &[
                r#"{"timestamp":"2026-07-14T02:16:00Z","type":"session_meta","payload":{}}"#,
                r#"{"type":"response_item","payload":{"role":"user","content":[{"type":"text","text":"<user_instructions>be nice</user_instructions>"}]}}"#,
            ],
        );
        assert!(Codex.summarize(&path).is_none());
        assert!(Codex.prompts(&path).is_empty());
    }
}
