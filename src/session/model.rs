//! Session, conversation, tool, and evidence projection types.

use super::filesystem::terminal_safe;
use serde_json::Value;

/// A line's top-level timestamp, shared by both transcript formats, or empty.
pub(crate) fn ts_of(value: &Value) -> String {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Classification of one parsed transcript record for list titles and parser tests.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PromptRecord {
    /// A prompt that the user actually typed.
    Typed(String),
    /// A readable user record whose content also contains unsupported parts.
    TypedWithUnsupported(String),
    /// A recognized non-prompt record, including injected and tool records.
    NotTyped,
    /// A user-like record whose shape is unsupported or malformed.
    UnsupportedUserLike,
}

impl PromptRecord {
    pub(crate) fn from_text_parts(parts: &[String], unsupported: bool) -> Self {
        if parts.is_empty() {
            if unsupported {
                Self::UnsupportedUserLike
            } else {
                Self::NotTyped
            }
        } else {
            let text = parts.join("\n");
            if unsupported {
                Self::TypedWithUnsupported(text)
            } else {
                Self::Typed(text)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TranscriptDiagnostics {
    pub(super) malformed_lines: usize,
    pub(super) unsupported_user_records: usize,
}

impl TranscriptDiagnostics {
    pub(super) fn observe_prompt_record(&mut self, record: PromptRecord) -> Option<String> {
        match record {
            PromptRecord::Typed(text) => Some(text),
            PromptRecord::TypedWithUnsupported(text) => {
                self.unsupported_user_records += 1;
                Some(text)
            }
            PromptRecord::UnsupportedUserLike => {
                self.unsupported_user_records += 1;
                None
            }
            PromptRecord::NotTyped => None,
        }
    }
}

/// One Session's list-row data.
///
/// Every Transcript yields a summary, so Sessions with no readable message remain
/// visible and deletable.
pub(crate) struct SessionSummary {
    /// Full session id (the row shows the final UUID group for canonical UUIDs).
    pub id: String,
    /// Session start timestamp (ISO-8601), or empty if none was found.
    pub start_ts: String,
    /// The agent-generated title when available, otherwise the first readable
    /// user message, or empty for a tool/injected-only session.
    pub title: String,
    pub latest_message: String,
    pub message_count: usize,
    pub tool_count: usize,
    pub native_facts: SessionNativeFacts,
    pub(super) diagnostics: TranscriptDiagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub(crate) enum ConversationRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ConversationMessage {
    pub(crate) entry_ids: Vec<String>,
    pub(crate) role: ConversationRole,
    pub(crate) timestamp: String,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub(crate) enum ToolActivityStatus {
    Started,
    Completed,
    Failed,
    Incomplete,
    Unknown,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ToolActivity {
    pub(crate) entry_ids: Vec<String>,
    pub(crate) call_id: Option<String>,
    pub(crate) timestamp: String,
    pub(crate) name: String,
    pub(crate) status: ToolActivityStatus,
    pub(crate) summary: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TranscriptEvidenceSummary {
    pub(crate) entry_id: String,
    pub(crate) line: u64,
    pub(crate) timestamp: String,
    pub(crate) native_type: String,
    pub(crate) role: Option<String>,
    pub(crate) content_types: Vec<String>,
    pub(crate) status: String,
    pub(crate) preview: String,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct SessionDetailStats {
    pub(crate) start_ts: String,
    pub(crate) last_event_ts: String,
    pub(crate) observed_duration_ms: Option<i64>,
    pub(crate) message_count: usize,
    pub(crate) tool_count: usize,
    pub(crate) entry_count: usize,
    pub(crate) malformed_count: usize,
    pub(crate) unsupported_count: usize,
    pub(crate) hidden_internal_count: usize,
    pub(crate) file_size: u64,
    pub(crate) snapshot: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct SessionDetailMeta {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) start_ts: String,
    pub(crate) transcript_path: String,
    pub(crate) cwd: Option<String>,
    pub(crate) model_provider: Option<String>,
    pub(crate) cli_version: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SessionNativeFacts {
    pub(crate) cwd: Option<String>,
    pub(crate) model_provider: Option<String>,
    pub(crate) cli_version: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum DetailRecord {
    Message(ConversationMessage),
    Tool(ToolActivity),
    Evidence(TranscriptEvidenceSummary),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) enum EvidenceEncoding {
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "base64")]
    Base64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TranscriptEvidence {
    pub(crate) entry_id: String,
    pub(crate) encoding: EvidenceEncoding,
    pub(crate) content: String,
    pub(crate) snapshot: String,
}

pub(crate) fn bounded_preview(value: &str) -> String {
    const MAX: usize = 240;
    let safe = terminal_safe(value);
    safe.chars().take(MAX).collect()
}

fn content_types(value: &Value) -> Vec<String> {
    value
        .pointer("/message/content")
        .or_else(|| value.pointer("/payload/content"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("type").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn role_of(value: &Value) -> Option<String> {
    value
        .pointer("/message/role")
        .or_else(|| value.pointer("/payload/role"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn text_at(value: &Value, pointer: &str) -> Option<String> {
    match value.pointer(pointer) {
        Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
        Some(Value::Array(items)) => {
            let parts = items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}

pub(crate) fn evidence_for(
    value: &Value,
    entry_id: &str,
    line: u64,
    status: &str,
) -> TranscriptEvidenceSummary {
    TranscriptEvidenceSummary {
        entry_id: entry_id.to_string(),
        line,
        timestamp: ts_of(value),
        native_type: value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        role: role_of(value),
        content_types: content_types(value),
        status: status.to_string(),
        preview: text_at(value, "/message/content")
            .or_else(|| text_at(value, "/payload/content"))
            .or_else(|| {
                value
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .map(|text| bounded_preview(&text))
            .unwrap_or_default(),
    }
}

/// Test-only compatibility projection for backend parser tests.
#[cfg(test)]
#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct Prompt {
    /// The turn's timestamp (ISO-8601), or empty.
    pub timestamp: String,
    /// The full prompt text (all supported text content joined; injected
    /// wrappers already filtered by the backend).
    pub text: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct SessionListRow {
    pub(crate) id: String,
    pub(crate) display_id: String,
    pub(crate) start_ts: String,
    pub(crate) title: String,
    pub(crate) latest_message: String,
    pub(crate) message_count: usize,
    pub(crate) tool_count: usize,
    pub(crate) warnings: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct SessionListData {
    pub(crate) sessions: Vec<SessionListRow>,
    pub(crate) warnings: Vec<String>,
    pub(crate) partial: bool,
}
