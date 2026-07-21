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
//! `!`-shell commands, the per-project AGENTS.md preamble) as `text` content
//! items; those are filtered by `is_wrapper_text`. Human `input_text` content is
//! kept verbatim. A turn left with no text after filtering is skipped for previews
//! and `get`.
//!
//! The session id is the trailing uuid of the filename (last 36 chars of the
//! stem after `rollout-<date>-`).

use crate::session::{self, SessionBackend};
use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// True if `t` is an injected wrapper text item Codex records as a user turn but
/// that the user never typed. Only `text` items are tested with this; human
/// `input_text` is kept verbatim even if it begins with the same literal text.
fn is_wrapper_text(t: &str) -> bool {
    const TAGS: &[(&str, &str)] = &[
        ("<environment_context>", "</environment_context>"),
        ("<user_instructions>", "</user_instructions>"),
        ("<INSTRUCTIONS>", "</INSTRUCTIONS>"),
    ];
    if TAGS
        .iter()
        .any(|(open, close)| t.starts_with(open) && t.contains(close))
    {
        return true;
    }
    if t.starts_with("<user_shell") && (t.contains("</user_shell>") || t.ends_with("/>")) {
        return true;
    }
    if t.starts_with("## My env\n") || t == "## My env" {
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
    fn files(&self, home: &Path) -> Result<Vec<PathBuf>> {
        let Some(base) = session::checked_session_dir(home, &[".codex", "sessions"])? else {
            return Ok(Vec::new());
        };
        session::walk_jsonl(&base, |name| name.starts_with("rollout-"))
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

    /// The `session_meta` carries the session start timestamp. Look for it by
    /// type instead of parsed-line index, so a corrupt or skipped first line
    /// cannot make a later event timestamp look like the session start.
    fn start_ts_of(&self, _idx: usize, v: &Value) -> Option<String> {
        (v.get("type").and_then(Value::as_str) == Some("session_meta"))
            .then(|| session::ts_of(v))
            .filter(|ts| !ts.is_empty())
    }

    fn fallback_start_ts_of(&self, _idx: usize, v: &Value) -> Option<String> {
        let ts = session::ts_of(v);
        (!ts.is_empty()).then_some(ts)
    }
}

/// If `v` is a `response_item` user message, join its content items' text with
/// newlines, dropping injected wrapper `text` items. Returns `None` when `v`
/// isn't a user turn or nothing real survives filtering.
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
            let keep = match it.get("type").and_then(Value::as_str) {
                Some("input_text") => true,
                Some("text") => !is_wrapper_text(t),
                _ => false,
            };
            if keep && !t.is_empty() {
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
    fn is_wrapper_text_matches_all_branches() {
        // Complete wrapper shapes.
        assert!(is_wrapper_text(
            "<environment_context>cwd=/work</environment_context>"
        ));
        assert!(is_wrapper_text(
            "<user_instructions>be nice</user_instructions>"
        ));
        assert!(is_wrapper_text("<user_shell name=\"ls\"></user_shell>"));
        assert!(is_wrapper_text("<user_shell name=\"ls\" />"));
        assert!(is_wrapper_text("<INSTRUCTIONS>x</INSTRUCTIONS>"));
        assert!(is_wrapper_text("## My env\nlinux"));
        // The `#… instructions for ` branch (stays on the first line).
        assert!(is_wrapper_text("# Base instructions for gpt-5.5\nmore"));
        // A `#` line without the phrase, and the phrase not at string start.
        assert!(!is_wrapper_text("# just a heading"));
        assert!(!is_wrapper_text("preamble\n# instructions for x"));
        // Prefix-only text is not enough to hide a prompt.
        assert!(!is_wrapper_text("<environment_context>literal prompt"));
        assert!(!is_wrapper_text("## My env is literal text"));
        // A real prompt.
        assert!(!is_wrapper_text("the real ask"));
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
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text, "the real ask");
    }

    #[test]
    fn input_text_that_looks_like_a_wrapper_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_jsonl(
            dir.path(),
            ".codex/sessions/2026/07/14/rollout-x-dddddddd-1111-2222-3333-444455556666.jsonl",
            &[
                r#"{"timestamp":"2026-07-14T02:16:00Z","type":"session_meta","payload":{}}"#,
                r#"{"type":"response_item","payload":{"role":"user","content":[{"type":"input_text","text":"<environment_context>please explain this literal tag</environment_context>"}]}}"#,
            ],
        );

        let ps = Codex.prompts(&path).unwrap();

        assert_eq!(ps.len(), 1);
        assert_eq!(
            ps[0].text,
            "<environment_context>please explain this literal tag</environment_context>"
        );
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
        let s = Codex.summarize(&path).unwrap();
        assert_eq!(s.title, "");
        assert_eq!(s.start_ts, "2026-07-14T02:16:00Z");
        assert!(Codex.prompts(&path).unwrap().is_empty());
    }
}
