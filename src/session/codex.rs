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
//! turn left with no text after filtering is skipped for previews and detail.
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
pub(super) struct Codex;

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
    /// [`user_turn_record`]. Used by shared summary and detail parsing.
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
#[path = "codex_tests.rs"]
mod tests;
