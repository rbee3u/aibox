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

use crate::session::{
    ConversationMessage, ConversationRole, DetailRecord, PromptRecord, SessionBackend,
    SessionNativeFacts, ToolActivity, ToolActivityStatus, bounded_preview, evidence_for, ts_of,
};
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

    fn detail_records(&self, value: &Value, entry_id: &str, line: u64) -> Vec<DetailRecord> {
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let role = value
            .pointer("/message/role")
            .and_then(Value::as_str)
            .or(match kind {
                "user" => Some("user"),
                "assistant" => Some("assistant"),
                _ => None,
            })
            .unwrap_or("");
        let mut unsupported_content = false;
        let user_text = if role == "user" {
            match self.prompt_record(value) {
                PromptRecord::Typed(text) => Some(text),
                PromptRecord::TypedWithUnsupported(text) => {
                    unsupported_content = true;
                    Some(text)
                }
                PromptRecord::NotTyped | PromptRecord::UnsupportedUserLike => None,
            }
        } else {
            None
        };
        let Some(content) = value.pointer("/message/content") else {
            return vec![DetailRecord::Evidence(evidence_for(
                value,
                entry_id,
                line,
                if kind == "thinking" || kind == "reasoning" {
                    "hidden_internal"
                } else {
                    "filtered"
                },
            ))];
        };
        let items = match content {
            Value::String(text) if !text.is_empty() && role == "assistant" => {
                return vec![DetailRecord::Message(ConversationMessage {
                    entry_ids: vec![entry_id.to_string()],
                    role: ConversationRole::Assistant,
                    timestamp: ts_of(value),
                    text: text.clone(),
                })];
            }
            Value::String(_) if user_text.is_some() => {
                return vec![DetailRecord::Message(ConversationMessage {
                    entry_ids: vec![entry_id.to_string()],
                    role: ConversationRole::User,
                    timestamp: ts_of(value),
                    text: user_text.unwrap_or_default(),
                })];
            }
            Value::Array(items) => items,
            _ => {
                return vec![DetailRecord::Evidence(evidence_for(
                    value,
                    entry_id,
                    line,
                    "unsupported",
                ))];
            }
        };
        let mut output = Vec::new();
        let mut text_parts = Vec::new();
        let mut hidden_internal = false;
        for item in items {
            match item.get("type").and_then(Value::as_str) {
                Some("text" | "output_text") if role == "assistant" => {
                    if let Some(text) = item.get("text").and_then(Value::as_str)
                        && !text.is_empty()
                    {
                        text_parts.push(text.to_string());
                    }
                }
                Some("tool_use") => {
                    output.push(DetailRecord::Tool(ToolActivity {
                        entry_ids: vec![entry_id.to_string()],
                        call_id: item.get("id").and_then(Value::as_str).map(str::to_string),
                        timestamp: ts_of(value),
                        name: item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("Tool")
                            .to_string(),
                        status: ToolActivityStatus::Started,
                        summary: item
                            .get("input")
                            .map(|input| bounded_preview(&input.to_string()))
                            .unwrap_or_default(),
                    }));
                }
                Some("tool_result") => {
                    output.push(DetailRecord::Tool(ToolActivity {
                        entry_ids: vec![entry_id.to_string()],
                        call_id: item
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        timestamp: ts_of(value),
                        name: "Tool result".to_string(),
                        status: if item.get("is_error").and_then(Value::as_bool) == Some(true) {
                            ToolActivityStatus::Failed
                        } else {
                            ToolActivityStatus::Completed
                        },
                        summary: item
                            .get("content")
                            .map(|content| bounded_preview(&content.to_string()))
                            .unwrap_or_default(),
                    }));
                }
                Some("thinking" | "reasoning") => hidden_internal = true,
                Some("image" | "document") => {}
                _ => unsupported_content = true,
            }
        }
        let message_text = if role == "user" {
            user_text
        } else {
            Some(text_parts.join("\n"))
        };
        if let Some(text) = message_text.filter(|text| !text.is_empty()) {
            output.insert(
                0,
                DetailRecord::Message(ConversationMessage {
                    entry_ids: vec![entry_id.to_string()],
                    role: if role == "user" {
                        ConversationRole::User
                    } else {
                        ConversationRole::Assistant
                    },
                    timestamp: ts_of(value),
                    text,
                }),
            );
        }
        if hidden_internal {
            output.push(DetailRecord::Evidence(evidence_for(
                value,
                entry_id,
                line,
                "hidden_internal",
            )));
        }
        if unsupported_content {
            output.push(DetailRecord::Evidence(evidence_for(
                value,
                entry_id,
                line,
                "unsupported",
            )));
        }
        if output.is_empty() {
            output.push(DetailRecord::Evidence(evidence_for(
                value,
                entry_id,
                line,
                if hidden_internal || kind == "thinking" || kind == "reasoning" {
                    "hidden_internal"
                } else if kind == "user" || kind == "assistant" {
                    "unsupported"
                } else {
                    "filtered"
                },
            )));
        }
        output
    }

    fn native_facts(&self, value: &Value, _facts: &mut SessionNativeFacts) {
        let _ = value;
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
#[path = "claude_tests.rs"]
mod tests;
