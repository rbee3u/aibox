//! Native request and response JSON envelopes.

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub(super) struct RequestEnvelope {
    pub(super) model: Option<String>,
    pub(super) stream: Option<bool>,
    pub(super) reasoning_effort: Option<String>,
    pub(super) reasoning: Option<EffortEnvelope>,
    pub(super) output_config: Option<EffortEnvelope>,
    pub(super) stream_options: Option<StreamOptionsEnvelope>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamOptionsEnvelope {
    pub(super) include_usage: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EffortEnvelope {
    pub(super) effort: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct TokenDetails {
    pub(super) cached_tokens: Option<u64>,
    pub(super) cache_write_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct OutputTokenDetails {
    pub(super) reasoning_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct CacheCreationDetails {
    pub(super) ephemeral_5m_input_tokens: Option<u64>,
    pub(super) ephemeral_1h_input_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct UsageEnvelope {
    pub(super) input_tokens: Option<u64>,
    pub(super) input_tokens_details: Option<TokenDetails>,
    pub(super) prompt_tokens: Option<u64>,
    pub(super) prompt_tokens_details: Option<TokenDetails>,
    pub(super) cache_read_input_tokens: Option<u64>,
    pub(super) cache_creation_input_tokens: Option<u64>,
    pub(super) cache_creation: Option<CacheCreationDetails>,
    pub(super) cache_creation_5m_input_tokens: Option<u64>,
    pub(super) cache_creation_1h_input_tokens: Option<u64>,
    pub(super) output_tokens: Option<u64>,
    pub(super) output_tokens_details: Option<OutputTokenDetails>,
    pub(super) completion_tokens: Option<u64>,
    pub(super) completion_tokens_details: Option<OutputTokenDetails>,
    pub(super) total_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChoiceEnvelope {
    pub(super) finish_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct IncompleteDetails {
    pub(super) reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ResponseEnvelope {
    pub(super) model: Option<String>,
    pub(super) reasoning_effort: Option<String>,
    pub(super) usage: Option<UsageEnvelope>,
    pub(super) error: Option<Value>,
    pub(super) incomplete_details: Option<IncompleteDetails>,
}

#[derive(Debug, Deserialize)]
pub(super) struct StreamEvent {
    #[serde(rename = "type")]
    pub(super) kind: Option<String>,
    pub(super) object: Option<String>,
    pub(super) model: Option<String>,
    pub(super) reasoning_effort: Option<String>,
    pub(super) response: Option<ResponseEnvelope>,
    pub(super) message: Option<Value>,
    pub(super) usage: Option<UsageEnvelope>,
    #[serde(default)]
    pub(super) choices: Vec<ChoiceEnvelope>,
    pub(super) error: Option<Value>,
    pub(super) code: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct JsonResponseEnvelope {
    #[serde(rename = "type")]
    pub(super) kind: Option<String>,
    pub(super) object: Option<String>,
    pub(super) model: Option<String>,
    pub(super) reasoning_effort: Option<String>,
    pub(super) usage: Option<UsageEnvelope>,
    #[serde(default)]
    pub(super) choices: Vec<ChoiceEnvelope>,
    pub(super) error: Option<Value>,
    pub(super) incomplete_details: Option<IncompleteDetails>,
}

pub(super) fn error_parts(
    error: &Value,
    fallback_kind: &str,
    fallback_message: &str,
) -> (String, String) {
    let kind = error
        .get("type")
        .or_else(|| error.get("code"))
        .and_then(Value::as_str)
        .unwrap_or(fallback_kind)
        .to_string();
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or(fallback_message)
        .to_string();
    (kind, message)
}
