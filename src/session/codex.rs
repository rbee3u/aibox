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
//! `!`-shell commands, skill payloads, the per-Workspace AGENTS.md preamble) as
//! text-like content items; [`real_text_fragment`] removes those prefixes. A
//! turn left with no text after filtering is skipped for previews and `get`.
//!
//! The session id is the trailing uuid of the filename (last 36 chars of the
//! stem after `rollout-<date>-`).

use crate::session::{
    self, ConversationMessage, ConversationRole, DetailRecord, PromptRecord, SessionBackend,
    SessionNativeFacts, ToolActivity, ToolActivityStatus, bounded_preview, evidence_for, ts_of,
};
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
    /// [`user_turn_record`]. Feeds shared summary and `get` paths.
    fn prompt_record(&self, value: &Value) -> PromptRecord {
        user_turn_record(value)
    }

    fn detail_records(&self, value: &Value, entry_id: &str, line: u64) -> Vec<DetailRecord> {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let payload = value.get("payload").unwrap_or(value);
        let payload_kind = payload.get("type").and_then(Value::as_str).unwrap_or(kind);
        let role = payload.get("role").and_then(Value::as_str).unwrap_or("");

        if kind == "response_item"
            && role == "user"
            && let record @ (PromptRecord::Typed(_) | PromptRecord::TypedWithUnsupported(_)) =
                user_turn_record(value)
        {
            let unsupported = matches!(record, PromptRecord::TypedWithUnsupported(_));
            let text = match record {
                PromptRecord::Typed(text) | PromptRecord::TypedWithUnsupported(text) => text,
                PromptRecord::NotTyped | PromptRecord::UnsupportedUserLike => unreachable!(),
            };
            let mut output = vec![DetailRecord::Message(ConversationMessage {
                entry_ids: vec![entry_id.to_string()],
                role: ConversationRole::User,
                timestamp: ts_of(value),
                text,
            })];
            if unsupported {
                output.push(DetailRecord::Evidence(evidence_for(
                    value,
                    entry_id,
                    line,
                    "unsupported",
                )));
            }
            return output;
        }

        if (kind == "response_item" && role == "assistant")
            || payload_kind == "agent_message"
            || (payload_kind == "message" && role == "assistant")
        {
            let (text, unsupported) = assistant_text(payload);
            if !text.is_empty() {
                let mut output = vec![DetailRecord::Message(ConversationMessage {
                    entry_ids: vec![entry_id.to_string()],
                    role: ConversationRole::Assistant,
                    timestamp: ts_of(value),
                    text,
                })];
                if unsupported {
                    output.push(DetailRecord::Evidence(evidence_for(
                        value,
                        entry_id,
                        line,
                        "unsupported",
                    )));
                }
                return output;
            }
        }

        if matches!(payload_kind, "function_call" | "custom_tool_call") {
            return vec![DetailRecord::Tool(ToolActivity {
                entry_ids: vec![entry_id.to_string()],
                call_id: payload
                    .get("call_id")
                    .or_else(|| payload.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                timestamp: ts_of(value),
                name: payload
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("Tool")
                    .to_string(),
                status: ToolActivityStatus::Started,
                summary: payload
                    .get("arguments")
                    .or_else(|| payload.get("input"))
                    .map(|input| bounded_preview(&input.to_string()))
                    .unwrap_or_default(),
            })];
        }
        if matches!(
            payload_kind,
            "function_call_output" | "custom_tool_call_output"
        ) {
            return vec![DetailRecord::Tool(ToolActivity {
                entry_ids: vec![entry_id.to_string()],
                call_id: payload
                    .get("call_id")
                    .or_else(|| payload.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                timestamp: ts_of(value),
                name: "Tool result".to_string(),
                status: if payload.get("is_error").and_then(Value::as_bool) == Some(true)
                    || payload.get("error").is_some()
                {
                    ToolActivityStatus::Failed
                } else {
                    ToolActivityStatus::Completed
                },
                summary: payload
                    .get("output")
                    .or_else(|| payload.get("content"))
                    .map(|output| bounded_preview(&output.to_string()))
                    .unwrap_or_default(),
            })];
        }
        let status = if payload_kind == "reasoning" || kind == "reasoning" {
            "hidden_internal"
        } else if kind == "response_item" && matches!(role, "user" | "assistant") {
            "unsupported"
        } else {
            "filtered"
        };
        vec![DetailRecord::Evidence(evidence_for(
            value, entry_id, line, status,
        ))]
    }

    fn native_facts(&self, value: &Value, facts: &mut SessionNativeFacts) {
        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            return;
        }
        if let Some(payload) = value.get("payload") {
            facts.cwd = payload
                .get("cwd")
                .and_then(Value::as_str)
                .map(str::to_string);
            facts.model_provider = payload
                .get("model_provider")
                .and_then(Value::as_str)
                .map(str::to_string);
            facts.cli_version = payload
                .get("cli_version")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
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

    /// Use the first event timestamp when no readable `session_meta` exists.
    fn fallback_start_ts_of(&self, value: &Value) -> Option<String> {
        let timestamp = session::ts_of(value);
        (!timestamp.is_empty()).then_some(timestamp)
    }
}

fn assistant_text(value: &Value) -> (String, bool) {
    for key in ["content", "text", "message"] {
        let Some(content) = value.get(key) else {
            continue;
        };
        match content {
            Value::String(text) if !text.is_empty() => return (text.clone(), false),
            Value::Array(items) => {
                let unsupported = items.iter().any(|item| {
                    !matches!(
                        item.get("type").and_then(Value::as_str),
                        Some("text" | "output_text")
                    )
                });
                let text = items
                    .iter()
                    .filter(|item| {
                        matches!(
                            item.get("type").and_then(Value::as_str),
                            Some("text" | "output_text")
                        )
                    })
                    .filter_map(|item| item.get("text").and_then(Value::as_str))
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    return (text, unsupported);
                }
            }
            _ => {}
        }
    }
    (String::new(), false)
}

fn trailing_uuid(stem: &str) -> Option<&str> {
    let suffix = stem.get(stem.len().checked_sub(session::UUID_TEXT_LEN)?..)?;
    session::is_canonical_uuid(suffix).then_some(suffix)
}

/// Classify a `response_item` user message after joining its text items and
/// dropping injected wrappers. Non-user records and turns with no surviving
/// content are `NotTyped`; unsupported user-like shapes remain diagnosable.
fn user_turn_record(value: &Value) -> PromptRecord {
    if value.get("type").and_then(Value::as_str) != Some("response_item") {
        return PromptRecord::NotTyped;
    }
    let Some(payload) = value.get("payload") else {
        return PromptRecord::NotTyped;
    };
    if payload.get("role").and_then(Value::as_str) != Some("user") {
        return PromptRecord::NotTyped;
    }
    let Some(items) = payload.get("content").and_then(Value::as_array) else {
        return PromptRecord::UnsupportedUserLike;
    };
    let mut parts = Vec::new();
    let mut unsupported = false;
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("input_text" | "text") => match item.get("text").and_then(Value::as_str) {
                Some(text) => {
                    if let Some(text) = real_text_fragment(text) {
                        parts.push(text);
                    }
                }
                None => unsupported = true,
            },
            Some("output_text" | "input_image" | "tool_result") => {}
            _ => unsupported = true,
        }
    }
    PromptRecord::from_text_parts(&parts, unsupported)
}

#[cfg(test)]
mod tests {
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
}
