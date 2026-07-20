//! Codex transcript format:
//! `<home>/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`.
//!
//! Mapped from the codex-rs `rollout` crate: each line is a `RolloutLine` that
//! flattens a top-level `timestamp` + `type` + `payload`. The first line is a
//! `session_meta` (its `timestamp` is the session start). User turns are
//! `response_item` messages with `role:"user"` whose `payload.content` is an
//! array of `{type:"input_text"|"text", text:"…"}` items.
//!
//! Codex has no ai-title, so a session's preview is its first *real* prompt. It
//! also records injected wrapper turns (environment/instructions context blocks,
//! `!`-shell commands, the per-project AGENTS.md preamble) as user turns; those
//! are filtered by `is_wrapper` applied to each content item. A turn left with no
//! text after filtering is skipped for previews and `get`.
//!
//! The session id is the trailing uuid of the filename (last 36 chars of the
//! stem after `rollout-<date>-`).

use super::SessionBackend;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// True if `t` is an injected wrapper turn Codex records as a user turn but that
/// the user never typed. Matches a set of literal prefixes at string start, plus
/// one `#… instructions for ` case that must stay on the first line.
fn is_wrapper(t: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "<environment_context>",
        "<user_instructions>",
        "<user_shell",
        "<INSTRUCTIONS>",
        "## My env",
    ];
    if PREFIXES.iter().any(|p| t.starts_with(p)) {
        return true;
    }
    // `^#[^\n]* instructions for `: a `#` at string start, then " instructions
    // for " somewhere on that same first line.
    t.lines()
        .next()
        .is_some_and(|first| first.starts_with('#') && first.contains(" instructions for "))
}

pub struct Codex;

impl SessionBackend for Codex {
    fn files(&self, home: &Path) -> Vec<PathBuf> {
        let base = home.join(".codex").join("sessions");
        super::walk_jsonl(&base, |name| name.starts_with("rollout-"))
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

    /// A real prompt is a wrapper-filtered `response_item` user message; see
    /// `user_turn_text`. Feeds shared summary and `get` paths.
    fn typed_text(&self, v: &Value) -> Option<String> {
        user_turn_text(v)
    }

    /// Line 0 (the `session_meta`) carries the session start timestamp. Always
    /// `Some` for line 0 — even an empty timestamp there settles the lookup,
    /// matching the old "line 0 or nothing" behavior — and `None` after it.
    fn start_ts_of(&self, idx: usize, v: &Value) -> Option<String> {
        (idx == 0).then(|| super::ts_of(v))
    }
}

/// If `v` is a `response_item` user message, join its content items' text with
/// newlines, dropping any item for which [`is_wrapper`] holds (an injected wrapper).
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
            if !t.is_empty() && !is_wrapper(t) {
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
        let s = Codex.summarize(&path);
        assert_eq!(s.start_ts, "2026-07-14T02:16:00Z");
        assert_eq!(s.title, "real question");
    }

    #[test]
    fn is_wrapper_matches_all_branches() {
        // Literal prefixes.
        assert!(is_wrapper(
            "<environment_context>cwd=/work</environment_context>"
        ));
        assert!(is_wrapper("<user_instructions>be nice</user_instructions>"));
        assert!(is_wrapper("<user_shell foo"));
        assert!(is_wrapper("<INSTRUCTIONS>x"));
        assert!(is_wrapper("## My env is linux"));
        // The `#… instructions for ` branch (stays on the first line).
        assert!(is_wrapper("# Base instructions for gpt-5.5\nmore"));
        // A `#` line without the phrase, and the phrase not at string start.
        assert!(!is_wrapper("# just a heading"));
        assert!(!is_wrapper("preamble\n# instructions for x"));
        // A real prompt.
        assert!(!is_wrapper("the real ask"));
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
    fn turn_that_is_all_wrapper_yields_no_prompts_but_still_summarizes() {
        // Every user turn is an injected wrapper, so no real prompt survives —
        // but the session still summarizes (empty title, meta ts) so `list` and
        // no-id `delete` can see and clear it.
        let dir = tempfile::tempdir().unwrap();
        let path = write_jsonl(
            dir.path(),
            ".codex/sessions/2026/07/14/rollout-x-cccccccc-1111-2222-3333-444455556666.jsonl",
            &[
                r#"{"timestamp":"2026-07-14T02:16:00Z","type":"session_meta","payload":{}}"#,
                r#"{"type":"response_item","payload":{"role":"user","content":[{"type":"text","text":"<user_instructions>be nice</user_instructions>"}]}}"#,
            ],
        );
        let s = Codex.summarize(&path);
        assert_eq!(s.title, "");
        assert_eq!(s.start_ts, "2026-07-14T02:16:00Z");
        assert!(Codex.prompts(&path).is_empty());
    }
}
