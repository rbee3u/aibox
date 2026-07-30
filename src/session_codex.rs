//! Codex transcript format:
//! `<home>/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`.
//!
//! Mapped from the codex-rs `rollout` crate: each line is a `RolloutLine` that
//! flattens a top-level `timestamp` + `type` + `payload`. The first line is a
//! `session_meta` (its `payload.timestamp` is the session start). User turns are
//! `response_item` messages with `role:"user"` whose `payload.content` is an
//! array of `{type:"input_text"|"text", text:"…"}` items.
//!
//! Codex has no ai-title, so a session's preview is its first *real* prompt. It
//! also records injected wrapper turns (environment/instructions context blocks,
//! `!`-shell commands, skill payloads, the per-project AGENTS.md preamble) as
//! text-like content items; [`real_text_fragment`] removes those prefixes. A
//! turn left with no text after filtering is skipped for previews and `get`.
//!
//! The session id is the trailing uuid of the filename (last 36 chars of the
//! stem after `rollout-<date>-`).

use crate::session::{self, SessionBackend};
use serde_json::Value;
use std::path::Path;

const WRAPPER_TAGS: &[(&str, &str)] = &[
    ("<environment_context>", "</environment_context>"),
    ("<user_instructions>", "</user_instructions>"),
    ("<app-context>", "</app-context>"),
    ("<apps_instructions>", "</apps_instructions>"),
    ("<INSTRUCTIONS>", "</INSTRUCTIONS>"),
    ("<skill>", "</skill>"),
    ("<permissions instructions>", "</permissions instructions>"),
    ("<plugins_instructions>", "</plugins_instructions>"),
    ("<skills_instructions>", "</skills_instructions>"),
    ("<collaboration_mode>", "</collaboration_mode>"),
    ("<recommended_plugins>", "</recommended_plugins>"),
];

/// True if `text` is an injected wrapper item Codex records as a user turn but
/// that the user never typed.
#[cfg(test)]
fn is_wrapper_text(text: &str) -> bool {
    real_text_fragment(text).is_none()
}

fn real_text_fragment(text: &str) -> Option<String> {
    let mut rest = text.trim_start();
    let mut stripped_wrapper = false;

    loop {
        if rest.is_empty() {
            return None;
        }
        if let Some(after) = strip_tagged_wrapper_prefix(rest) {
            rest = after.trim_start();
            stripped_wrapper = true;
            continue;
        }
        if let Some(after) = strip_user_shell_prefix(rest) {
            rest = after.trim_start();
            stripped_wrapper = true;
            continue;
        }
        if rest.starts_with("## My env\n") || rest == "## My env" {
            return None;
        }
        if first_line_is_instructions_preamble(rest) {
            if let Some(after) = strip_through(rest, "</INSTRUCTIONS>") {
                rest = after.trim_start();
                stripped_wrapper = true;
                continue;
            }
            return None;
        }

        return if stripped_wrapper {
            Some(rest.to_string())
        } else {
            Some(text.to_string())
        };
    }
}

fn strip_tagged_wrapper_prefix(text: &str) -> Option<&str> {
    WRAPPER_TAGS.iter().find_map(|(open, close)| {
        if text.starts_with(open) {
            strip_through(text, close)
        } else {
            None
        }
    })
}

fn strip_user_shell_prefix(text: &str) -> Option<&str> {
    if !text.starts_with("<user_shell") {
        return None;
    }
    strip_through(text, "</user_shell>").or_else(|| text.find("/>").map(|index| &text[index + 2..]))
}

fn strip_through<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    text.find(marker).map(|index| &text[index + marker.len()..])
}

fn first_line_is_instructions_preamble(text: &str) -> bool {
    // `^#[^\n]* instructions for `: a `#` at string start, then " instructions
    // for " somewhere on that same first line.
    text.lines()
        .next()
        .is_some_and(|first| first.starts_with('#') && first.contains(" instructions for "))
}

/// Parser for OpenAI Codex's on-disk rollout format.
pub struct Codex;

impl SessionBackend for Codex {
    fn session_dir_components(&self) -> &'static [&'static str] {
        &[".codex", "sessions"]
    }

    /// Only `rollout-*.jsonl` files are transcripts; Codex writes other
    /// `.jsonl` state under the same tree.
    fn keep_transcript_name(&self, name: &str) -> bool {
        name.starts_with("rollout-")
    }

    fn id_of(&self, path: &Path) -> String {
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy())
            .unwrap_or_default();
        trailing_uuid(&stem).unwrap_or(&stem).to_string()
    }

    /// A real prompt is a wrapper-filtered `response_item` user message; see
    /// `user_turn_text`. Feeds shared summary and `get` paths.
    fn typed_text(&self, value: &Value) -> Option<String> {
        user_turn_text(value)
    }

    /// The `session_meta` carries the session start timestamp. Look for it by
    /// type rather than line position, so a corrupt or skipped first line
    /// cannot make a later event timestamp look like the session start.
    fn start_ts_of(&self, value: &Value) -> Option<String> {
        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            return None;
        }
        value
            .get("payload")
            .and_then(|payload| payload.get("timestamp"))
            .and_then(Value::as_str)
            .filter(|timestamp| !timestamp.is_empty())
            .map(str::to_string)
            .or_else(|| {
                let timestamp = session::ts_of(value);
                (!timestamp.is_empty()).then_some(timestamp)
            })
    }

    /// Fall back to the first event timestamp for legacy or damaged rollouts
    /// that have no readable `session_meta`.
    fn fallback_start_ts_of(&self, value: &Value) -> Option<String> {
        let timestamp = session::ts_of(value);
        (!timestamp.is_empty()).then_some(timestamp)
    }
}

fn trailing_uuid(stem: &str) -> Option<&str> {
    let suffix = stem.get(stem.len().checked_sub(36)?..)?;
    is_uuid(suffix).then_some(suffix)
}

fn is_uuid(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

/// If `value` is a `response_item` user message, join its content items' text
/// with newlines, dropping injected wrapper items. Returns `None` when `value`
/// isn't a user turn or nothing real survives filtering.
fn user_turn_text(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str) != Some("response_item") {
        return None;
    }
    let payload = value.get("payload")?;
    if payload.get("role").and_then(Value::as_str) != Some("user") {
        return None;
    }
    let items = payload.get("content").and_then(Value::as_array)?;
    let mut parts = Vec::new();
    for item in items {
        if let Some(text) = real_content_item_text(item) {
            parts.push(text);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn real_content_item_text(item: &Value) -> Option<String> {
    if !matches!(
        item.get("type").and_then(Value::as_str),
        Some("input_text" | "text")
    ) {
        return None;
    }
    item.get("text")
        .and_then(Value::as_str)
        .and_then(real_text_fragment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::write_jsonl;

    #[test]
    fn files_keep_only_rollout_jsonl_transcripts() {
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

        let files = Codex.files(dir.path()).unwrap();

        assert_eq!(files, vec![rollout]);
    }

    #[test]
    fn list_files_apply_the_same_rollout_filter_as_files() {
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

        let discovery = Codex.list_files(dir.path()).unwrap();

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
    fn id_is_trailing_uuid() {
        let p = Path::new(
            "/h/.codex/sessions/2026/07/14/rollout-2026-07-14T02-16-00-3f2a1b6c-1111-2222-3333-444455556666.jsonl",
        );
        assert_eq!(Codex.id_of(p), "3f2a1b6c-1111-2222-3333-444455556666");
    }

    #[test]
    fn id_of_short_stem_falls_back_to_the_whole_stem() {
        assert_eq!(
            Codex.id_of(Path::new("/h/.codex/sessions/rollout-short.jsonl")),
            "rollout-short"
        );
        assert_eq!(Codex.id_of(Path::new("/h/.codex/sessions/x.jsonl")), "x");
    }

    #[test]
    fn id_of_long_non_uuid_stem_falls_back_to_the_whole_stem() {
        let stem = "rollout-this-name-is-longer-than-a-uuid-but-has-no-session-id";

        assert_eq!(
            Codex.id_of(Path::new(&format!("/h/.codex/sessions/{stem}.jsonl"))),
            stem
        );
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
        // Complete wrapper shapes.
        assert!(is_wrapper_text(
            "<environment_context>cwd=/work</environment_context>"
        ));
        assert!(is_wrapper_text(
            "\n  <environment_context>cwd=/work</environment_context>"
        ));
        assert!(is_wrapper_text(
            "<user_instructions>be nice</user_instructions>"
        ));
        assert!(is_wrapper_text("<app-context>x</app-context>"));
        assert!(is_wrapper_text("<apps_instructions>x</apps_instructions>"));
        assert!(is_wrapper_text("<user_shell name=\"ls\"></user_shell>"));
        assert!(is_wrapper_text("<user_shell name=\"ls\" />"));
        assert!(is_wrapper_text("<INSTRUCTIONS>x</INSTRUCTIONS>"));
        assert!(is_wrapper_text("<skill>x</skill>"));
        assert!(is_wrapper_text(
            "<permissions instructions>x</permissions instructions>"
        ));
        assert!(is_wrapper_text(
            "<plugins_instructions>x</plugins_instructions>"
        ));
        assert!(is_wrapper_text(
            "<skills_instructions>x</skills_instructions>"
        ));
        assert!(is_wrapper_text(
            "<collaboration_mode>x</collaboration_mode>"
        ));
        assert!(is_wrapper_text(
            "<recommended_plugins>x</recommended_plugins>"
        ));
        assert!(is_wrapper_text("## My env\nlinux"));
        assert!(is_wrapper_text("\n  ## My env\nlinux"));
        // The `#… instructions for ` branch (stays on the first line).
        assert!(is_wrapper_text("# Base instructions for gpt-5.5\nmore"));
        assert!(is_wrapper_text("  # Base instructions for gpt-5.5\nmore"));
        // A `#` line without the phrase, and the phrase not at string start.
        assert!(!is_wrapper_text("# just a heading"));
        assert!(!is_wrapper_text("preamble\n# instructions for x"));
        // Prefix-only text is not enough to hide a prompt.
        assert!(!is_wrapper_text("<environment_context>literal prompt"));
        assert!(!is_wrapper_text(
            "<environment_context>cwd=/work</environment_context>\nreal ask"
        ));
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

        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text, "real ask");
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

        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text, "real ask after shell");
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

        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text, "plain text prompt\ntyped prompt");
        assert_eq!(ps[0].timestamp, "2026-07-14T02:16:00Z");
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

        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text, "real ask");
        assert_eq!(summary.title, "real ask");
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

        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text, "real ask");
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

        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].text, "first real ask");
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
