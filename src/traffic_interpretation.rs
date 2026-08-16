//! Deriving the Model Protocol Summary from recorded bodies.
//!
//! [`ProtocolObserver`] accumulates one [`ProtocolSummary`] for the OpenAI
//! Responses, OpenAI Chat Completions, and Claude Messages families, keeping
//! requested and effective values separate and holding Token Usage in memory
//! until the protocol response is terminal. Anything else stays
//! [`ProtocolFamily::Unknown`], which short-circuits interpretation entirely.
//!
//! Interpretation is observational, never authoritative: a failure becomes a
//! deduplicated warning on the Summary and leaves the raw bodies, forwarding, and
//! Traffic Outcome untouched. See
//! `docs/adr/0009-traffic-record-evidence-and-projections.md`.

use crate::traffic_store::{RecordedHeader, StoredRecord};
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[cfg(test)]
use std::fs;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProtocolFamily {
    OpenaiResponses,
    OpenaiChatCompletions,
    ClaudeMessages,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseModeValue {
    Stream,
    Normal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BodyContentCoding {
    Identity,
    Zstd,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RequestedEffective<T> {
    pub requested: Option<T>,
    pub effective: Option<T>,
}

impl<T> Default for RequestedEffective<T> {
    fn default() -> Self {
        Self {
            requested: None,
            effective: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RequestedObserved<T> {
    pub requested: Option<T>,
    pub observed: Option<T>,
}

impl<T> Default for RequestedObserved<T> {
    fn default() -> Self {
        Self {
            requested: None,
            observed: None,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct TokenUsage {
    pub total_input_tokens: Option<u64>,
    pub base_input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cache_write_5m_tokens: Option<u64>,
    pub cache_write_1h_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_output_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ProtocolDiagnostic {
    pub kind: String,
    pub message: String,
    pub at_ns: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ProtocolSummary {
    pub family: ProtocolFamily,
    pub response_terminal: bool,
    pub model: RequestedEffective<String>,
    pub reasoning_effort: RequestedEffective<String>,
    pub response_mode: RequestedObserved<ResponseModeValue>,
    pub first_token_at_ns: Option<String>,
    pub token_usage: Option<TokenUsage>,
    pub errors: Vec<ProtocolDiagnostic>,
    pub warnings: Vec<ProtocolDiagnostic>,
}

impl ProtocolSummary {
    pub(crate) fn for_url(url: Option<&str>) -> Self {
        Self {
            family: family_from_url(url),
            ..Self::default()
        }
    }

    fn set_family(&mut self, family: ProtocolFamily, at_ns: Option<String>) -> bool {
        if family == ProtocolFamily::Unknown || family == self.family {
            return false;
        }
        if self.family == ProtocolFamily::Unknown {
            self.family = family;
            return true;
        }
        self.add_warning(
            "protocol_family_conflict",
            format!(
                "The response reported protocol family {family:?} after {:?} was already recorded; the first value was kept",
                self.family
            ),
            at_ns,
        )
    }

    fn set_requested_model(&mut self, value: Option<String>, at_ns: Option<String>) -> bool {
        let result = set_once(&mut self.model.requested, value, "Requested Model");
        self.finish_set(result, "requested_model_conflict", at_ns)
    }

    fn set_effective_model(&mut self, value: Option<String>, at_ns: Option<String>) -> bool {
        let result = set_once(&mut self.model.effective, value, "Effective Model");
        self.finish_set(result, "effective_model_conflict", at_ns)
    }

    fn set_requested_effort(&mut self, value: Option<String>, at_ns: Option<String>) -> bool {
        let result = set_once(
            &mut self.reasoning_effort.requested,
            value,
            "Requested Reasoning Effort",
        );
        self.finish_set(result, "requested_reasoning_effort_conflict", at_ns)
    }

    fn set_effective_effort(&mut self, value: Option<String>, at_ns: Option<String>) -> bool {
        let result = set_once(
            &mut self.reasoning_effort.effective,
            value,
            "Effective Reasoning Effort",
        );
        self.finish_set(result, "effective_reasoning_effort_conflict", at_ns)
    }

    fn set_requested_mode(
        &mut self,
        value: Option<ResponseModeValue>,
        at_ns: Option<String>,
    ) -> bool {
        let result = set_once(
            &mut self.response_mode.requested,
            value,
            "Requested Model Response Mode",
        );
        self.finish_set(result, "requested_response_mode_conflict", at_ns)
    }

    fn set_observed_mode(
        &mut self,
        value: Option<ResponseModeValue>,
        at_ns: Option<String>,
    ) -> bool {
        let result = set_once(
            &mut self.response_mode.observed,
            value,
            "Observed Model Response Mode",
        );
        self.finish_set(result, "observed_response_mode_conflict", at_ns)
    }

    fn finish_set(&mut self, result: SetOnce, warning_kind: &str, at_ns: Option<String>) -> bool {
        match result {
            SetOnce::Changed => true,
            SetOnce::Unchanged => false,
            SetOnce::Conflict(message) => self.add_warning(warning_kind, message, at_ns),
        }
    }

    fn add_error(
        &mut self,
        kind: impl Into<String>,
        message: impl Into<String>,
        at_ns: Option<String>,
    ) -> bool {
        push_unique(
            &mut self.errors,
            ProtocolDiagnostic {
                kind: kind.into(),
                message: message.into(),
                at_ns,
            },
        )
    }

    fn add_warning(
        &mut self,
        kind: impl Into<String>,
        message: impl Into<String>,
        at_ns: Option<String>,
    ) -> bool {
        push_unique(
            &mut self.warnings,
            ProtocolDiagnostic {
                kind: kind.into(),
                message: message.into(),
                at_ns,
            },
        )
    }
}

enum SetOnce {
    Changed,
    Unchanged,
    Conflict(String),
}

fn set_once<T>(target: &mut Option<T>, value: Option<T>, label: &str) -> SetOnce
where
    T: Eq + std::fmt::Debug,
{
    let Some(value) = value else {
        return SetOnce::Unchanged;
    };
    match target {
        None => {
            *target = Some(value);
            SetOnce::Changed
        }
        Some(existing) if *existing == value => SetOnce::Unchanged,
        Some(existing) => SetOnce::Conflict(format!(
            "{label} was already {existing:?}, so conflicting value {value:?} was ignored"
        )),
    }
}

fn push_unique(target: &mut Vec<ProtocolDiagnostic>, value: ProtocolDiagnostic) -> bool {
    if target
        .iter()
        .any(|existing| existing.kind == value.kind && existing.message == value.message)
    {
        return false;
    }
    target.push(value);
    true
}

#[derive(Debug, Default)]
pub(crate) struct ProtocolObserver {
    summary: ProtocolSummary,
    usage: UsageAccumulator,
    has_usage: bool,
    expects_stream_usage: bool,
}

impl ProtocolObserver {
    pub(crate) fn new(url: Option<&str>) -> Self {
        Self {
            summary: ProtocolSummary::for_url(url),
            ..Self::default()
        }
    }

    pub(crate) fn snapshot(&self) -> ProtocolSummary {
        self.summary.clone()
    }

    pub(crate) fn observe_request(
        &mut self,
        path: &Path,
        headers: &[RecordedHeader],
        at_ns: String,
    ) -> bool {
        if self.summary.family == ProtocolFamily::Unknown {
            return false;
        }
        let result = parse_request(path, headers);
        let envelope = match result {
            Ok(envelope) => envelope,
            Err(error) => {
                return self.summary.add_warning(
                    "request_interpretation_failed",
                    format!("Cannot interpret model request metadata: {error:#}"),
                    Some(at_ns),
                );
            }
        };
        let mut changed = self
            .summary
            .set_requested_model(nonempty(envelope.model), Some(at_ns.clone()));
        let effort = match self.summary.family {
            ProtocolFamily::OpenaiResponses => envelope.reasoning.and_then(|value| value.effort),
            ProtocolFamily::OpenaiChatCompletions => envelope.reasoning_effort,
            ProtocolFamily::ClaudeMessages => envelope.output_config.and_then(|value| value.effort),
            ProtocolFamily::Unknown => None,
        };
        changed |= self
            .summary
            .set_requested_effort(nonempty(effort), Some(at_ns.clone()));
        let streaming = envelope.stream.unwrap_or(false);
        if self.summary.family == ProtocolFamily::OpenaiChatCompletions {
            self.expects_stream_usage = streaming
                && envelope
                    .stream_options
                    .is_some_and(|options| options.include_usage == Some(true));
        }
        changed |= self.summary.set_requested_mode(
            Some(if streaming {
                ResponseModeValue::Stream
            } else {
                ResponseModeValue::Normal
            }),
            Some(at_ns),
        );
        changed
    }

    pub(crate) fn observe_response_headers(
        &mut self,
        headers: &[RecordedHeader],
        event_stream: Option<bool>,
        at_ns: String,
    ) -> bool {
        let mut changed = event_stream
            .is_some_and(|event_stream| self.observe_response_mode(event_stream, at_ns.clone()));
        let model =
            header_text(headers, "openai-model").or_else(|| header_text(headers, "x-openai-model"));
        changed |= self
            .summary
            .set_effective_model(nonempty(model), Some(at_ns));
        changed
    }

    pub(crate) fn observe_response_mode(&mut self, event_stream: bool, at_ns: String) -> bool {
        self.summary.set_observed_mode(
            Some(if event_stream {
                ResponseModeValue::Stream
            } else {
                ResponseModeValue::Normal
            }),
            Some(at_ns),
        )
    }

    pub(crate) fn observe_first_token(&mut self, at_ns: String) -> bool {
        if self.summary.family == ProtocolFamily::Unknown
            || self.summary.first_token_at_ns.is_some()
        {
            return false;
        }
        self.summary.first_token_at_ns = Some(at_ns);
        true
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn observe_sse_data(&mut self, data: &[u8], at_ns: String) -> bool {
        self.observe_sse_event(None, data, at_ns)
    }

    pub(crate) fn observe_json_response(
        &mut self,
        path: &Path,
        _status: u16,
        headers: &[RecordedHeader],
        at_ns: String,
    ) -> bool {
        let file = match crate::tenant::open_real_file(path, "Traffic response body")
            .context("open response JSON")
        {
            Ok(file) => file,
            Err(error) => {
                return self.summary.add_warning(
                    "response_interpretation_failed",
                    format!("Cannot interpret model response metadata: {error:#}"),
                    Some(at_ns),
                );
            }
        };
        let parsed = match body_content_coding(headers) {
            Ok(BodyContentCoding::Identity) => {
                serde_json::from_reader::<_, JsonResponseEnvelope>(file)
                    .context("parse response JSON")
            }
            Ok(BodyContentCoding::Zstd) => zstd::stream::read::Decoder::new(file)
                .context("create zstd response decoder")
                .and_then(|decoder| {
                    serde_json::from_reader::<_, JsonResponseEnvelope>(decoder)
                        .context("parse response JSON")
                }),
            Err(error) => Err(error).context("read response Content-Encoding"),
        };
        let mut changed = false;
        match parsed {
            Ok(envelope) => {
                let family = if envelope.object.as_deref() == Some("response") {
                    ProtocolFamily::OpenaiResponses
                } else if envelope.object.as_deref() == Some("chat.completion") {
                    ProtocolFamily::OpenaiChatCompletions
                } else if envelope.kind.as_deref() == Some("message") {
                    ProtocolFamily::ClaudeMessages
                } else {
                    ProtocolFamily::Unknown
                };
                changed |= self.summary.set_family(family, Some(at_ns.clone()));
                changed |= self
                    .summary
                    .set_effective_model(nonempty(envelope.model), Some(at_ns.clone()));
                changed |= self
                    .summary
                    .set_effective_effort(nonempty(envelope.reasoning_effort), Some(at_ns.clone()));
                if let Some(usage) = envelope.usage {
                    changed |= self.apply_usage(&usage, Some(at_ns.clone()));
                }
                if self.summary.family == ProtocolFamily::OpenaiChatCompletions {
                    changed |= self.apply_chat_choices(&envelope.choices, &at_ns);
                }
                if self.summary.family != ProtocolFamily::Unknown {
                    if let Some(error) = envelope.error {
                        let (kind, message) =
                            error_parts(&error, "api_error", "Upstream API error");
                        changed |= self.summary.add_error(kind, message, Some(at_ns.clone()));
                    }
                    if let Some(reason) = envelope.incomplete_details.and_then(|value| value.reason)
                    {
                        changed |= self.summary.add_error(
                            "response_incomplete",
                            format!("OpenAI response was incomplete: {reason}"),
                            Some(at_ns),
                        );
                    }
                }
            }
            Err(error) if self.summary.family != ProtocolFamily::Unknown => {
                changed |= self.summary.add_warning(
                    "response_interpretation_failed",
                    format!("Cannot interpret model response metadata: {error:#}"),
                    Some(at_ns),
                );
            }
            Err(_) => {}
        }
        if self.summary.family != ProtocolFamily::Unknown && !self.summary.response_terminal {
            self.summary.response_terminal = true;
            changed = true;
        }
        changed | self.commit_final_usage()
    }

    pub(crate) fn observe_sse_event(
        &mut self,
        event_name: Option<&[u8]>,
        data: &[u8],
        at_ns: String,
    ) -> bool {
        let data = trim_ascii(data);
        if data.is_empty() {
            return false;
        }
        if data == b"[DONE]" {
            return self.apply_chat_done(at_ns);
        }
        let event: StreamEvent = match serde_json::from_slice(data).context("parse SSE data JSON") {
            Ok(event) => event,
            Err(error) => {
                return self.summary.add_warning(
                    "sse_event_invalid",
                    format!("Cannot interpret SSE event: {error:#}"),
                    Some(at_ns),
                );
            }
        };
        self.apply_event(event_name, event, at_ns)
    }

    fn apply_event(
        &mut self,
        event_name: Option<&[u8]>,
        event: StreamEvent,
        at_ns: String,
    ) -> bool {
        let StreamEvent {
            kind,
            object,
            model,
            reasoning_effort,
            response,
            message,
            usage,
            choices,
            error,
            code,
        } = event;
        let kind = kind.as_deref().unwrap_or_default();
        let event_name = event_name.and_then(|value| std::str::from_utf8(value).ok());
        let family = event_family(kind, event_name, object.as_deref());
        let mut changed = self.summary.set_family(family, Some(at_ns.clone()));

        if self.summary.family == ProtocolFamily::OpenaiChatCompletions {
            changed |= self
                .summary
                .set_effective_model(nonempty(model), Some(at_ns.clone()));
            changed |= self
                .summary
                .set_effective_effort(nonempty(reasoning_effort), Some(at_ns.clone()));
            if let Some(usage) = usage.as_ref() {
                changed |= self.apply_usage(usage, Some(at_ns.clone()));
            }
            changed |= self.apply_chat_choices(&choices, &at_ns);
        }

        if self.summary.family != ProtocolFamily::Unknown
            && let Some(response) = response
        {
            changed |= self.apply_response_event(response, &at_ns);
        }

        if self.summary.family != ProtocolFamily::Unknown
            && let Some(message) = message.as_ref()
        {
            changed |= self.apply_message_event(message, &at_ns);
        }
        if self.summary.family != ProtocolFamily::Unknown
            && self.summary.family != ProtocolFamily::OpenaiChatCompletions
            && let Some(usage) = usage
        {
            changed |= self.apply_usage(&usage, Some(at_ns.clone()));
        }

        let error_terminal = self.summary.family == ProtocolFamily::OpenaiChatCompletions
            && (error.is_some() || kind == "error" || event_name == Some("error"));
        changed |= self.apply_error_event(
            kind,
            event_name,
            error.as_ref(),
            message.as_ref(),
            code.as_deref(),
            &at_ns,
        );

        let terminal_kind = event_name
            .filter(|value| is_terminal_event_kind(value))
            .unwrap_or(kind);
        changed |= self.apply_terminal_event(terminal_kind, at_ns);
        if error_terminal {
            changed |= self.mark_terminal_and_commit_usage();
        }
        changed
    }

    fn apply_chat_done(&mut self, at_ns: String) -> bool {
        if self.summary.family != ProtocolFamily::OpenaiChatCompletions
            || self.summary.response_terminal
        {
            return false;
        }
        let mut changed = false;
        if self.expects_stream_usage && !self.has_usage {
            changed |= self.summary.add_warning(
                "token_usage_missing",
                "OpenAI Chat Completions was asked to include stream usage but reported none",
                Some(at_ns.clone()),
            );
        }
        changed | self.mark_terminal_and_commit_usage()
    }

    fn apply_chat_choices(&mut self, choices: &[ChoiceEnvelope], at_ns: &str) -> bool {
        let mut changed = false;
        for finish_reason in choices
            .iter()
            .filter_map(|choice| choice.finish_reason.as_deref())
        {
            changed |= match finish_reason {
                "stop" | "tool_calls" | "function_call" => false,
                "length" => self.summary.add_error(
                    "response_incomplete",
                    "OpenAI Chat Completions stopped after reaching a length limit",
                    Some(at_ns.to_string()),
                ),
                "content_filter" => self.summary.add_error(
                    "content_filtered",
                    "OpenAI Chat Completions output was omitted by a content filter",
                    Some(at_ns.to_string()),
                ),
                value => self.summary.add_warning(
                    "finish_reason_unknown",
                    format!("OpenAI Chat Completions reported unknown finish reason {value:?}"),
                    Some(at_ns.to_string()),
                ),
            };
        }
        changed
    }

    fn apply_response_event(&mut self, response: ResponseEnvelope, at_ns: &str) -> bool {
        let mut changed = self
            .summary
            .set_effective_model(nonempty(response.model), Some(at_ns.to_string()));
        changed |= self
            .summary
            .set_effective_effort(nonempty(response.reasoning_effort), Some(at_ns.to_string()));
        if let Some(usage) = response.usage {
            changed |= self.apply_usage(&usage, Some(at_ns.to_string()));
        }
        if let Some(error) = response.error {
            let (error_kind, message) = error_parts(&error, "api_error", "OpenAI response error");
            changed |= self
                .summary
                .add_error(error_kind, message, Some(at_ns.to_string()));
        }
        if let Some(reason) = response.incomplete_details.and_then(|value| value.reason) {
            changed |= self.summary.add_error(
                "response_incomplete",
                format!("OpenAI response was incomplete: {reason}"),
                Some(at_ns.to_string()),
            );
        }
        changed
    }

    fn apply_message_event(&mut self, message: &Value, at_ns: &str) -> bool {
        let Some(message) = message.as_object() else {
            return false;
        };
        let mut changed = self.summary.set_effective_model(
            message
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
                .and_then(|value| nonempty(Some(value))),
            Some(at_ns.to_string()),
        );
        if let Some(usage) = message.get("usage") {
            match serde_json::from_value::<UsageEnvelope>(usage.clone()) {
                Ok(usage) => changed |= self.apply_usage(&usage, Some(at_ns.to_string())),
                Err(error) => {
                    changed |= self.summary.add_warning(
                        "token_usage_invalid",
                        format!("Cannot parse message_start usage: {error}"),
                        Some(at_ns.to_string()),
                    );
                }
            }
        }
        changed
    }

    fn apply_error_event(
        &mut self,
        kind: &str,
        event_name: Option<&str>,
        error: Option<&Value>,
        message: Option<&Value>,
        code: Option<&str>,
        at_ns: &str,
    ) -> bool {
        let chat_error =
            self.summary.family == ProtocolFamily::OpenaiChatCompletions && error.is_some();
        if (kind != "error" && event_name != Some("error") && !chat_error)
            || self.summary.family == ProtocolFamily::Unknown
        {
            return false;
        }
        if let Some(error) = error {
            let (error_kind, message) = error_parts(error, "api_error", "Upstream API error");
            self.summary
                .add_error(error_kind, message, Some(at_ns.to_string()))
        } else if let Some(message) = message.and_then(Value::as_str) {
            self.summary.add_error(
                code.unwrap_or("api_error"),
                message,
                Some(at_ns.to_string()),
            )
        } else {
            false
        }
    }

    fn apply_terminal_event(&mut self, terminal_kind: &str, at_ns: String) -> bool {
        let terminal = is_terminal_event_kind(terminal_kind)
            || (terminal_kind == "error" && self.summary.family != ProtocolFamily::Unknown);
        let mut changed = terminal && self.mark_terminal_and_commit_usage();
        let has_error_at = self
            .summary
            .errors
            .iter()
            .any(|error| error.at_ns.as_deref() == Some(at_ns.as_str()));
        if terminal_kind == "response.failed" && !has_error_at {
            changed |= self.summary.add_error(
                "failed",
                "OpenAI stream ended with response.failed",
                Some(at_ns),
            );
        } else if terminal_kind == "response.incomplete" && !has_error_at {
            changed |= self.summary.add_error(
                "response_incomplete",
                "OpenAI stream ended with response.incomplete",
                Some(at_ns),
            );
        } else if terminal_kind == "response.cancelled" && !has_error_at {
            changed |= self.summary.add_warning(
                "cancelled",
                "OpenAI stream ended with response.cancelled",
                Some(at_ns),
            );
        }
        changed
    }

    fn mark_terminal_and_commit_usage(&mut self) -> bool {
        let mut changed = false;
        if !self.summary.response_terminal {
            self.summary.response_terminal = true;
            changed = true;
        }
        changed | self.commit_final_usage()
    }

    fn apply_usage(&mut self, usage: &UsageEnvelope, at_ns: Option<String>) -> bool {
        let chat = self.summary.family == ProtocolFamily::OpenaiChatCompletions;
        merge_option(
            &mut self.usage.input_tokens,
            if chat {
                usage.prompt_tokens
            } else {
                usage.input_tokens
            },
        );
        let input_details = if chat {
            usage.prompt_tokens_details.as_ref()
        } else {
            usage.input_tokens_details.as_ref()
        };
        merge_option(
            &mut self.usage.cached_tokens,
            input_details.and_then(|value| value.cached_tokens),
        );
        merge_option(
            &mut self.usage.cache_write_tokens,
            input_details.and_then(|value| value.cache_write_tokens),
        );
        let output_details = if chat {
            usage.completion_tokens_details.as_ref()
        } else {
            usage.output_tokens_details.as_ref()
        };
        merge_option(
            &mut self.usage.reasoning_tokens,
            output_details.and_then(|value| value.reasoning_tokens),
        );
        merge_option(
            &mut self.usage.output_tokens,
            if chat {
                usage.completion_tokens
            } else {
                usage.output_tokens
            },
        );
        merge_option(&mut self.usage.total_tokens, usage.total_tokens);
        merge_option(
            &mut self.usage.cache_read_tokens,
            usage.cache_read_input_tokens,
        );
        merge_option(
            &mut self.usage.cache_creation_tokens,
            usage.cache_creation_input_tokens,
        );
        let cache_creation = usage.cache_creation.as_ref();
        let five_minute_cache_writes =
            cache_creation.and_then(|value| value.ephemeral_5m_input_tokens);
        let one_hour_cache_writes =
            cache_creation.and_then(|value| value.ephemeral_1h_input_tokens);
        merge_option(
            &mut self.usage.cache_write_5m_tokens,
            five_minute_cache_writes.or(usage.cache_creation_5m_input_tokens),
        );
        merge_option(
            &mut self.usage.cache_write_1h_tokens,
            one_hour_cache_writes.or(usage.cache_creation_1h_input_tokens),
        );
        self.has_usage = true;
        self.validate_usage(at_ns)
    }

    fn validate_usage(&mut self, at_ns: Option<String>) -> bool {
        match self.summary.family {
            ProtocolFamily::OpenaiResponses | ProtocolFamily::OpenaiChatCompletions => {
                let mut changed = false;
                if let Some(total) = self.usage.input_tokens {
                    let cached = self.usage.cached_tokens.unwrap_or(0);
                    let writes = self.usage.cache_write_tokens.unwrap_or(0);
                    if total
                        .checked_sub(cached)
                        .and_then(|value| value.checked_sub(writes))
                        .is_none()
                    {
                        changed |= self.summary.add_warning(
                            "token_usage_inconsistent",
                            "OpenAI input token details exceed the reported total input tokens",
                            at_ns.clone(),
                        );
                    }
                }
                if self.summary.family == ProtocolFamily::OpenaiChatCompletions
                    && let (Some(input), Some(output), Some(total)) = (
                        self.usage.input_tokens,
                        self.usage.output_tokens,
                        self.usage.total_tokens,
                    )
                    && input.checked_add(output) != Some(total)
                {
                    changed |= self.summary.add_warning(
                        "token_usage_inconsistent",
                        format!(
                            "OpenAI Chat Completions total tokens ({total}) do not equal prompt plus completion tokens ({input} + {output})"
                        ),
                        at_ns,
                    );
                }
                return changed;
            }
            ProtocolFamily::ClaudeMessages => {
                let split = self
                    .usage
                    .cache_write_5m_tokens
                    .unwrap_or(0)
                    .checked_add(self.usage.cache_write_1h_tokens.unwrap_or(0));
                if let (Some(total), Some(split)) = (self.usage.cache_creation_tokens, split)
                    && (self.usage.cache_write_5m_tokens.is_some()
                        || self.usage.cache_write_1h_tokens.is_some())
                    && total != split
                {
                    return self.summary.add_warning(
                        "cache_write_breakdown_inconsistent",
                        format!(
                            "Claude cache write total ({total}) does not match the reported 5m/1h breakdown ({split})"
                        ),
                        at_ns,
                    );
                }
            }
            ProtocolFamily::Unknown => {}
        }
        false
    }

    fn commit_final_usage(&mut self) -> bool {
        if self.summary.token_usage.is_some() {
            return false;
        }
        let Some(usage) = self.normalized_usage() else {
            return false;
        };
        self.summary.token_usage = Some(usage);
        true
    }

    fn normalized_usage(&self) -> Option<TokenUsage> {
        if !self.has_usage {
            return None;
        }
        match self.summary.family {
            ProtocolFamily::OpenaiResponses | ProtocolFamily::OpenaiChatCompletions => {
                let total = self.usage.input_tokens;
                let cached = self.usage.cached_tokens;
                let writes = self.usage.cache_write_tokens;
                let base = total.and_then(|value| {
                    value
                        .checked_sub(cached.unwrap_or(0))
                        .and_then(|value| value.checked_sub(writes.unwrap_or(0)))
                });
                Some(TokenUsage {
                    total_input_tokens: total,
                    base_input_tokens: base,
                    cached_input_tokens: cached,
                    cache_write_tokens: writes,
                    output_tokens: self.usage.output_tokens,
                    reasoning_output_tokens: self.usage.reasoning_tokens,
                    ..TokenUsage::default()
                })
            }
            ProtocolFamily::ClaudeMessages => {
                let split_reported = self.usage.cache_write_5m_tokens.is_some()
                    || self.usage.cache_write_1h_tokens.is_some();
                let split_sum = split_reported
                    .then(|| {
                        self.usage
                            .cache_write_5m_tokens
                            .unwrap_or(0)
                            .checked_add(self.usage.cache_write_1h_tokens.unwrap_or(0))
                    })
                    .flatten();
                let writes = self.usage.cache_creation_tokens.or(split_sum);
                let split_valid = split_reported && writes.is_some() && split_sum == writes;
                let total = self.usage.input_tokens.and_then(|base| {
                    base.checked_add(self.usage.cache_read_tokens.unwrap_or(0))?
                        .checked_add(writes.unwrap_or(0))
                });
                Some(TokenUsage {
                    total_input_tokens: total,
                    base_input_tokens: self.usage.input_tokens,
                    cached_input_tokens: self.usage.cache_read_tokens,
                    cache_write_tokens: (!split_valid).then_some(writes).flatten(),
                    cache_write_5m_tokens: split_valid
                        .then_some(self.usage.cache_write_5m_tokens)
                        .flatten(),
                    cache_write_1h_tokens: split_valid
                        .then_some(self.usage.cache_write_1h_tokens)
                        .flatten(),
                    output_tokens: self.usage.output_tokens,
                    reasoning_output_tokens: None,
                })
            }
            ProtocolFamily::Unknown => None,
        }
    }
}

fn event_family(kind: &str, event_name: Option<&str>, object: Option<&str>) -> ProtocolFamily {
    if kind.starts_with("response.")
        || event_name.is_some_and(|value| value.starts_with("response."))
    {
        ProtocolFamily::OpenaiResponses
    } else if matches!(object, Some("chat.completion" | "chat.completion.chunk")) {
        ProtocolFamily::OpenaiChatCompletions
    } else if is_claude_event_kind(kind) || event_name.is_some_and(is_claude_event_kind) {
        ProtocolFamily::ClaudeMessages
    } else {
        ProtocolFamily::Unknown
    }
}

fn is_claude_event_kind(value: &str) -> bool {
    matches!(
        value,
        "message_start"
            | "message_delta"
            | "message_stop"
            | "content_block_start"
            | "content_block_delta"
            | "content_block_stop"
    )
}

fn is_terminal_event_kind(value: &str) -> bool {
    matches!(
        value,
        "response.completed"
            | "response.failed"
            | "response.incomplete"
            | "response.cancelled"
            | "message_stop"
    )
}

#[derive(Clone, Debug, Default)]
struct UsageAccumulator {
    input_tokens: Option<u64>,
    cached_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_creation_tokens: Option<u64>,
    cache_write_5m_tokens: Option<u64>,
    cache_write_1h_tokens: Option<u64>,
    output_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

fn merge_option(target: &mut Option<u64>, value: Option<u64>) {
    if value.is_some() {
        *target = value;
    }
}

fn parse_request(path: &Path, headers: &[RecordedHeader]) -> Result<RequestEnvelope> {
    let file = crate::tenant::open_real_file(path, "Traffic request body")?;
    match body_content_coding(headers)? {
        BodyContentCoding::Identity => serde_json::from_reader(file),
        BodyContentCoding::Zstd => {
            let decoder =
                zstd::stream::read::Decoder::new(file).context("create zstd request decoder")?;
            serde_json::from_reader(decoder)
        }
    }
    .context("parse request JSON")
}

pub(crate) fn body_content_coding(headers: &[RecordedHeader]) -> Result<BodyContentCoding> {
    let mut codings = Vec::new();
    for header in headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("content-encoding"))
    {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&header.value_base64)
            .context("decode Content-Encoding header")?;
        let value = std::str::from_utf8(&bytes).context("Content-Encoding header is not UTF-8")?;
        codings.extend(
            value
                .split(',')
                .map(str::trim)
                .filter(|coding| !coding.is_empty())
                .map(str::to_ascii_lowercase),
        );
    }
    if codings.is_empty() {
        return Ok(BodyContentCoding::Identity);
    }
    match codings.as_slice() {
        [coding] if coding == "identity" => Ok(BodyContentCoding::Identity),
        [coding] if coding == "zstd" => Ok(BodyContentCoding::Zstd),
        _ => bail!("unsupported Content-Encoding {:?}", codings.join(", ")),
    }
}

fn family_from_url(value: Option<&str>) -> ProtocolFamily {
    let Some(path) = value
        .and_then(|value| url::Url::parse(value).ok())
        .map(|url| url.path().trim_end_matches('/').to_string())
    else {
        return ProtocolFamily::Unknown;
    };
    if path.ends_with("/responses") {
        ProtocolFamily::OpenaiResponses
    } else if path.ends_with("/chat/completions") {
        ProtocolFamily::OpenaiChatCompletions
    } else if path.ends_with("/messages") {
        ProtocolFamily::ClaudeMessages
    } else {
        ProtocolFamily::Unknown
    }
}

pub(crate) fn coding_agent_session_id(
    upstream_url: Option<&str>,
    headers: &[RecordedHeader],
) -> Option<String> {
    let names = match family_from_url(upstream_url) {
        ProtocolFamily::OpenaiResponses | ProtocolFamily::OpenaiChatCompletions => {
            ["session-id", "x-claude-code-session-id"]
        }
        ProtocolFamily::ClaudeMessages => ["x-claude-code-session-id", "session-id"],
        ProtocolFamily::Unknown => return None,
    };
    names
        .into_iter()
        .find_map(|name| first_nonempty_header_text(headers, name))
}

fn first_nonempty_header_text(headers: &[RecordedHeader], name: &str) -> Option<String> {
    headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case(name))
        .filter_map(|header| {
            base64::engine::general_purpose::STANDARD
                .decode(&header.value_base64)
                .ok()
        })
        .filter_map(|bytes| String::from_utf8(bytes).ok())
        .find_map(|value| nonempty(Some(value)))
}

fn header_text(headers: &[RecordedHeader], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case(name))
        .and_then(|header| {
            base64::engine::general_purpose::STANDARD
                .decode(&header.value_base64)
                .ok()
        })
        .and_then(|bytes| String::from_utf8(bytes).ok())
}

fn nonempty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

#[derive(Debug, Deserialize)]
struct RequestEnvelope {
    model: Option<String>,
    stream: Option<bool>,
    reasoning_effort: Option<String>,
    reasoning: Option<EffortEnvelope>,
    output_config: Option<EffortEnvelope>,
    stream_options: Option<StreamOptionsEnvelope>,
}

#[derive(Debug, Deserialize)]
struct StreamOptionsEnvelope {
    include_usage: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct EffortEnvelope {
    effort: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct TokenDetails {
    cached_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
struct OutputTokenDetails {
    reasoning_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
struct CacheCreationDetails {
    ephemeral_5m_input_tokens: Option<u64>,
    ephemeral_1h_input_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
struct UsageEnvelope {
    input_tokens: Option<u64>,
    input_tokens_details: Option<TokenDetails>,
    prompt_tokens: Option<u64>,
    prompt_tokens_details: Option<TokenDetails>,
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_creation: Option<CacheCreationDetails>,
    cache_creation_5m_input_tokens: Option<u64>,
    cache_creation_1h_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    output_tokens_details: Option<OutputTokenDetails>,
    completion_tokens: Option<u64>,
    completion_tokens_details: Option<OutputTokenDetails>,
    total_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ChoiceEnvelope {
    finish_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct IncompleteDetails {
    reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ResponseEnvelope {
    model: Option<String>,
    reasoning_effort: Option<String>,
    usage: Option<UsageEnvelope>,
    error: Option<Value>,
    incomplete_details: Option<IncompleteDetails>,
}

#[derive(Debug, Deserialize)]
struct StreamEvent {
    #[serde(rename = "type")]
    kind: Option<String>,
    object: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    response: Option<ResponseEnvelope>,
    message: Option<Value>,
    usage: Option<UsageEnvelope>,
    #[serde(default)]
    choices: Vec<ChoiceEnvelope>,
    error: Option<Value>,
    code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsonResponseEnvelope {
    #[serde(rename = "type")]
    kind: Option<String>,
    object: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    usage: Option<UsageEnvelope>,
    #[serde(default)]
    choices: Vec<ChoiceEnvelope>,
    error: Option<Value>,
    incomplete_details: Option<IncompleteDetails>,
}

fn error_parts(error: &Value, fallback_kind: &str, fallback_message: &str) -> (String, String) {
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

pub(crate) fn timeline_end_at_ns(record: &StoredRecord, live: Option<String>) -> Option<String> {
    if record.active {
        return live;
    }
    if let Some(finished) = &record.summary.timing.finished_at_ns {
        return Some(finished.clone());
    }
    let protocol_offsets = record
        .summary
        .protocol
        .as_ref()
        .into_iter()
        .flat_map(|protocol| {
            protocol.first_token_at_ns.as_ref().into_iter().chain(
                protocol
                    .errors
                    .iter()
                    .chain(&protocol.warnings)
                    .filter_map(|diagnostic| diagnostic.at_ns.as_ref()),
            )
        });
    [
        record
            .summary
            .timing
            .upstream_request_started_at_ns
            .as_ref(),
        record
            .summary
            .timing
            .upstream_request_body_first_byte_at_ns
            .as_ref(),
        record
            .summary
            .timing
            .upstream_request_body_completed_at_ns
            .as_ref(),
        record
            .summary
            .timing
            .upstream_response_headers_at_ns
            .as_ref(),
        record
            .summary
            .timing
            .upstream_response_body_first_byte_at_ns
            .as_ref(),
        record
            .summary
            .timing
            .upstream_response_body_completed_at_ns
            .as_ref(),
    ]
    .into_iter()
    .flatten()
    .chain(protocol_offsets)
    .filter_map(|value| value.parse::<u128>().ok().map(|parsed| (parsed, value)))
    .max_by_key(|(parsed, _)| *parsed)
    .map(|(_, value)| value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traffic_store::{RequestMetadata, SummaryMetadata, TimingMetadata};
    use std::path::PathBuf;

    fn header(name: &str, value: &[u8]) -> RecordedHeader {
        RecordedHeader {
            name: name.to_string(),
            value_base64: base64::engine::general_purpose::STANDARD.encode(value),
        }
    }

    #[test]
    fn body_content_coding_accepts_identity_and_one_case_insensitive_zstd_only() {
        assert_eq!(
            body_content_coding(&[]).unwrap(),
            BodyContentCoding::Identity
        );
        assert_eq!(
            body_content_coding(&[header("Content-Encoding", b"  ")]).unwrap(),
            BodyContentCoding::Identity
        );
        assert_eq!(
            body_content_coding(&[header("CONTENT-ENCODING", b" ZsTd ")]).unwrap(),
            BodyContentCoding::Zstd
        );
        for (value, expected) in [
            (
                b"identity, zstd".as_slice(),
                "unsupported Content-Encoding \"identity, zstd\"",
            ),
            (b"gzip".as_slice(), "unsupported Content-Encoding \"gzip\""),
        ] {
            let error = body_content_coding(&[header("content-encoding", value)])
                .unwrap_err()
                .to_string();
            assert_eq!(error, expected, "{value:?}");
        }

        let invalid_utf8 = header("content-encoding", &[0xff]);
        assert_eq!(
            body_content_coding(&[invalid_utf8])
                .unwrap_err()
                .to_string(),
            "Content-Encoding header is not UTF-8"
        );
    }

    #[test]
    fn coding_agent_session_id_uses_protocol_specific_exact_headers() {
        let headers = [
            header("X-Claude-Code-Session-Id", b"claude-session"),
            header("SESSION-ID", b"codex-session"),
        ];
        assert_eq!(
            coding_agent_session_id(Some("https://example.test/v1/responses"), &headers).as_deref(),
            Some("codex-session")
        );
        assert_eq!(
            coding_agent_session_id(Some("https://example.test/v1/messages"), &headers).as_deref(),
            Some("claude-session")
        );
        assert_eq!(
            coding_agent_session_id(Some("https://example.test/v1/responses"), &headers[..1])
                .as_deref(),
            Some("claude-session")
        );
        assert_eq!(
            coding_agent_session_id(
                Some("https://example.test/openai/deployments/gpt/chat/completions/?api-version=1"),
                &headers,
            )
            .as_deref(),
            Some("codex-session")
        );
        assert_eq!(
            coding_agent_session_id(Some("https://example.test/health"), &headers),
            None
        );
    }

    #[test]
    fn coding_agent_session_id_keeps_the_first_nonempty_utf8_value() {
        let headers = [
            header("session-id", b""),
            header("session-id", b"opaque-session-value"),
            header("session-id", &[0xff]),
            header("x-session-id", b"ignored"),
        ];
        assert_eq!(
            coding_agent_session_id(Some("https://example.test/v1/responses"), &headers).as_deref(),
            Some("opaque-session-value")
        );
    }

    #[test]
    fn request_metadata_maps_provider_specific_reasoning_effort() {
        let temp = tempfile::tempdir().unwrap();
        let openai_path = temp.path().join("openai-request.json");
        fs::write(
            &openai_path,
            br#"{"model":"gpt-requested","stream":true,"reasoning":{"effort":"high"}}"#,
        )
        .unwrap();
        let mut openai = ProtocolObserver::new(Some("https://example.test/v1/responses"));
        assert!(openai.observe_request(&openai_path, &[], "10".to_string()));
        let summary = openai.snapshot();
        assert_eq!(summary.model.requested.as_deref(), Some("gpt-requested"));
        assert_eq!(summary.reasoning_effort.requested.as_deref(), Some("high"));
        assert_eq!(
            summary.response_mode.requested,
            Some(ResponseModeValue::Stream)
        );

        let claude_path = temp.path().join("claude-request.json");
        fs::write(
            &claude_path,
            br#"{"model":"claude-requested","output_config":{"effort":"max"}}"#,
        )
        .unwrap();
        let mut claude = ProtocolObserver::new(Some("https://example.test/v1/messages"));
        assert!(claude.observe_request(&claude_path, &[], "20".to_string()));
        let summary = claude.snapshot();
        assert_eq!(summary.reasoning_effort.requested.as_deref(), Some("max"));
        assert_eq!(
            summary.response_mode.requested,
            Some(ResponseModeValue::Normal)
        );

        let chat_path = temp.path().join("chat-request.json");
        fs::write(
            &chat_path,
            br#"{"model":"gpt-chat","reasoning_effort":"medium","stream":true,"stream_options":{"include_usage":true}}"#,
        )
        .unwrap();
        let mut chat = ProtocolObserver::new(Some(
            "https://example.test/openai/deployments/gpt/chat/completions?api-version=1",
        ));
        assert!(chat.observe_request(&chat_path, &[], "30".to_string()));
        let summary = chat.snapshot();
        assert_eq!(summary.family, ProtocolFamily::OpenaiChatCompletions);
        assert_eq!(summary.model.requested.as_deref(), Some("gpt-chat"));
        assert_eq!(
            summary.reasoning_effort.requested.as_deref(),
            Some("medium")
        );
        assert_eq!(
            summary.response_mode.requested,
            Some(ResponseModeValue::Stream)
        );
        assert!(chat.expects_stream_usage);
    }

    #[test]
    fn zstd_request_metadata_is_interpreted_after_the_recorded_body_is_complete() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("request.zstd");
        let compressed = zstd::stream::encode_all(
            br#"{"model":"gpt-compressed","reasoning":{"effort":"medium"},"stream":true}"#
                .as_slice(),
            0,
        )
        .unwrap();
        fs::write(&path, compressed).unwrap();
        let headers = [RecordedHeader {
            name: "content-encoding".to_string(),
            value_base64: base64::engine::general_purpose::STANDARD.encode(" ZsTd "),
        }];
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));

        assert!(observer.observe_request(&path, &headers, "10".to_string()));
        let summary = observer.snapshot();
        assert_eq!(summary.model.requested.as_deref(), Some("gpt-compressed"));
        assert_eq!(
            summary.reasoning_effort.requested.as_deref(),
            Some("medium")
        );
        assert_eq!(
            summary.response_mode.requested,
            Some(ResponseModeValue::Stream)
        );
    }

    #[test]
    fn response_headers_and_nonstream_body_publish_stable_effective_facts() {
        let temp = tempfile::tempdir().unwrap();
        let response_path = temp.path().join("response.json");
        fs::write(
            &response_path,
            br#"{"object":"response","model":"body-model","reasoning_effort":"medium","usage":{"input_tokens":12,"output_tokens":4}}"#,
        )
        .unwrap();
        let headers = [RecordedHeader {
            name: "openai-model".to_string(),
            value_base64: "aGVhZGVyLW1vZGVs".to_string(),
        }];
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
        assert!(observer.observe_response_headers(&headers, Some(false), "10".to_string()));
        assert!(observer.observe_json_response(&response_path, 200, &[], "20".to_string()));
        let summary = observer.snapshot();
        assert_eq!(summary.model.effective.as_deref(), Some("header-model"));
        assert_eq!(
            summary.reasoning_effort.effective.as_deref(),
            Some("medium")
        );
        assert_eq!(
            summary.response_mode.observed,
            Some(ResponseModeValue::Normal)
        );
        assert!(summary.response_terminal);
        assert!(summary.first_token_at_ns.is_none());
        assert_eq!(summary.token_usage.unwrap().output_tokens, Some(4));
        assert_eq!(summary.warnings.len(), 1);
        assert_eq!(summary.warnings[0].kind, "effective_model_conflict");
    }

    #[test]
    fn malformed_protocol_data_warns_without_synthesizing_provider_errors() {
        let temp = tempfile::tempdir().unwrap();
        let response_path = temp.path().join("response.json");
        fs::write(&response_path, b"not json").unwrap();
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
        assert!(observer.observe_sse_data(b"not json", "10".to_string()));
        assert!(observer.observe_json_response(&response_path, 503, &[], "20".to_string()));
        let summary = observer.snapshot();
        assert!(summary.response_terminal);
        assert_eq!(summary.warnings.len(), 2);
        assert!(summary.errors.is_empty());

        let unknown = ProtocolObserver::new(Some("https://example.test/health"));
        let summary = unknown.snapshot();
        assert_eq!(summary.family, ProtocolFamily::Unknown);
        assert!(!summary.response_terminal);
        assert!(summary.errors.is_empty());
    }

    #[test]
    fn zstd_response_metadata_is_interpreted_after_the_recorded_body_is_complete() {
        let temp = tempfile::tempdir().unwrap();
        let response_path = temp.path().join("response.zstd");
        let compressed = zstd::stream::encode_all(
            br#"{"object":"response","model":"gpt-compressed","usage":{"input_tokens":12,"output_tokens":4}}"#
                .as_slice(),
            0,
        )
        .unwrap();
        fs::write(&response_path, compressed).unwrap();
        let headers = [RecordedHeader {
            name: "content-encoding".to_string(),
            value_base64: base64::engine::general_purpose::STANDARD.encode("zstd"),
        }];
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));

        assert!(observer.observe_json_response(&response_path, 200, &headers, "20".to_string()));
        let summary = observer.snapshot();
        assert_eq!(summary.model.effective.as_deref(), Some("gpt-compressed"));
        assert_eq!(summary.token_usage.unwrap().output_tokens, Some(4));
        assert!(summary.response_terminal);
    }

    #[test]
    fn chat_nonstream_body_infers_family_and_normalizes_usage() {
        let temp = tempfile::tempdir().unwrap();
        let response_path = temp.path().join("chat-response.json");
        fs::write(
            &response_path,
            br#"{"object":"chat.completion","model":"gpt-effective","choices":[{"finish_reason":"length"}],"usage":{"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":40,"cache_write_tokens":10},"completion_tokens":20,"completion_tokens_details":{"reasoning_tokens":5},"total_tokens":120}}"#,
        )
        .unwrap();
        let mut observer = ProtocolObserver::new(Some("https://example.test/gateway"));

        assert!(observer.observe_json_response(&response_path, 200, &[], "20".to_string()));
        let summary = observer.snapshot();
        assert_eq!(summary.family, ProtocolFamily::OpenaiChatCompletions);
        assert_eq!(summary.model.effective.as_deref(), Some("gpt-effective"));
        assert!(summary.response_terminal);
        let usage = summary.token_usage.unwrap();
        assert_eq!(usage.total_input_tokens, Some(100));
        assert_eq!(usage.base_input_tokens, Some(50));
        assert_eq!(usage.cached_input_tokens, Some(40));
        assert_eq!(usage.cache_write_tokens, Some(10));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.reasoning_output_tokens, Some(5));
        assert_eq!(summary.errors[0].kind, "response_incomplete");
    }

    #[test]
    fn openai_usage_is_not_published_before_terminal_event() {
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
        assert!(observer.observe_sse_data(
            br#"{"type":"response.created","response":{"model":"effective","usage":{"input_tokens":100}}}"#,
            "10".to_string(),
        ));
        assert!(observer.snapshot().token_usage.is_none());
        assert!(observer.observe_sse_data(
            br#"{"type":"response.completed","response":{"usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":40,"cache_write_tokens":10},"output_tokens":20,"output_tokens_details":{"reasoning_tokens":5}}}}"#,
            "20".to_string(),
        ));
        let usage = observer.snapshot().token_usage.unwrap();
        assert_eq!(usage.total_input_tokens, Some(100));
        assert_eq!(usage.base_input_tokens, Some(50));
        assert_eq!(usage.reasoning_output_tokens, Some(5));
    }

    #[test]
    fn chat_stream_holds_usage_until_done_and_reports_finish_reasons() {
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/chat/completions"));
        assert!(observer.observe_sse_data(
            br#"{"object":"chat.completion.chunk","model":"gpt-stream","choices":[{"finish_reason":null}]}"#,
            "10".to_string(),
        ));
        assert!(observer.observe_sse_data(
            br#"{"object":"chat.completion.chunk","choices":[{"finish_reason":"content_filter"},{"finish_reason":"vendor_stop"}],"usage":{"prompt_tokens":50,"prompt_tokens_details":{"cached_tokens":20},"completion_tokens":7,"completion_tokens_details":{"reasoning_tokens":2},"total_tokens":57}}"#,
            "20".to_string(),
        ));
        let partial = observer.snapshot();
        assert_eq!(partial.model.effective.as_deref(), Some("gpt-stream"));
        assert!(!partial.response_terminal);
        assert!(partial.token_usage.is_none());
        assert_eq!(partial.errors[0].kind, "content_filtered");
        assert_eq!(partial.warnings[0].kind, "finish_reason_unknown");

        assert!(observer.observe_sse_data(b" \t[DONE]\r\n", "30".to_string()));
        let summary = observer.snapshot();
        assert!(summary.response_terminal);
        let usage = summary.token_usage.unwrap();
        assert_eq!(usage.total_input_tokens, Some(50));
        assert_eq!(usage.base_input_tokens, Some(30));
        assert_eq!(usage.output_tokens, Some(7));
        assert_eq!(usage.reasoning_output_tokens, Some(2));
    }

    #[test]
    fn chat_done_warns_when_requested_stream_usage_is_missing() {
        let temp = tempfile::tempdir().unwrap();
        let request_path = temp.path().join("chat-request.json");
        fs::write(
            &request_path,
            br#"{"model":"gpt-chat","stream":true,"stream_options":{"include_usage":true}}"#,
        )
        .unwrap();
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/chat/completions"));
        observer.observe_request(&request_path, &[], "10".to_string());

        assert!(observer.observe_sse_data(b"[DONE]", "20".to_string()));
        let summary = observer.snapshot();
        assert!(summary.response_terminal);
        assert!(summary.token_usage.is_none());
        assert_eq!(summary.warnings[0].kind, "token_usage_missing");
    }

    #[test]
    fn chat_stream_error_is_terminal_and_usage_inconsistency_warns() {
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/chat/completions"));
        observer.observe_sse_data(
            br#"{"object":"chat.completion.chunk","usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":99},"error":{"type":"server_error","message":"failed"}}"#,
            "10".to_string(),
        );

        let summary = observer.snapshot();
        assert!(summary.response_terminal);
        assert_eq!(summary.errors[0].kind, "server_error");
        assert_eq!(summary.warnings[0].kind, "token_usage_inconsistent");
        assert_eq!(summary.token_usage.unwrap().output_tokens, Some(2));
    }

    #[test]
    fn claude_usage_is_accumulated_until_message_stop() {
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/messages"));
        observer.observe_sse_data(
            br#"{"type":"message_start","message":{"model":"claude","usage":{"input_tokens":37,"cache_read_input_tokens":340,"cache_creation_input_tokens":38}}}"#,
            "10".to_string(),
        );
        observer.observe_sse_data(
            br#"{"type":"message_delta","usage":{"output_tokens":13}}"#,
            "20".to_string(),
        );
        assert!(observer.snapshot().token_usage.is_none());
        observer.observe_sse_data(br#"{"type":"message_stop"}"#, "30".to_string());
        let usage = observer.snapshot().token_usage.unwrap();
        assert_eq!(usage.total_input_tokens, Some(415));
        assert_eq!(usage.output_tokens, Some(13));
    }

    #[test]
    fn claude_missing_cache_counters_stay_unreported() {
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/messages"));
        observer.observe_sse_data(
            br#"{"type":"message_start","message":{"usage":{"input_tokens":37}}}"#,
            "10".to_string(),
        );
        observer.observe_sse_data(br#"{"type":"message_stop"}"#, "20".to_string());

        let usage = observer.snapshot().token_usage.unwrap();
        assert_eq!(usage.total_input_tokens, Some(37));
        assert_eq!(usage.cached_input_tokens, None);
        assert_eq!(usage.cache_write_tokens, None);
        assert_eq!(usage.cache_write_5m_tokens, None);
        assert_eq!(usage.cache_write_1h_tokens, None);
    }

    #[test]
    fn first_model_and_effort_values_win_and_conflicts_are_deduplicated() {
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
        observer.observe_sse_data(
            br#"{"type":"response.created","response":{"model":"first","reasoning_effort":"high"}}"#,
            "10".to_string(),
        );
        observer.observe_sse_data(
            br#"{"type":"response.completed","response":{"model":"second","reasoning_effort":"low"}}"#,
            "20".to_string(),
        );
        observer.observe_sse_data(
            br#"{"type":"response.completed","response":{"model":"second","reasoning_effort":"low"}}"#,
            "30".to_string(),
        );
        let summary = observer.snapshot();
        assert_eq!(summary.model.effective.as_deref(), Some("first"));
        assert_eq!(summary.reasoning_effort.effective.as_deref(), Some("high"));
        assert_eq!(summary.warnings.len(), 2);
        assert_eq!(summary.warnings[0].kind, "effective_model_conflict");
        assert_eq!(
            summary.warnings[1].kind,
            "effective_reasoning_effort_conflict"
        );
    }

    #[test]
    fn failed_openai_terminal_event_commits_final_usage_and_error() {
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
        observer.observe_sse_data(
            br#"{"type":"response.failed","response":{"usage":{"input_tokens":9,"output_tokens":2},"error":{"type":"server_error","message":"failed"}}}"#,
            "10".to_string(),
        );
        let summary = observer.snapshot();
        assert!(summary.response_terminal);
        assert_eq!(summary.token_usage.unwrap().total_input_tokens, Some(9));
        assert_eq!(summary.errors.len(), 1);
        assert_eq!(summary.errors[0].kind, "server_error");
    }

    #[test]
    fn response_failed_event_with_top_level_error_is_a_provider_error() {
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
        observer.observe_sse_event(
            Some(b"response.failed"),
            br#"{"type":"error","error":{"type":"service_unavailable_error","code":"server_error","message":"Our servers are currently overloaded. Please try again later.","param":null},"sequence_number":2}"#,
            "20".to_string(),
        );
        let summary = observer.snapshot();
        assert_eq!(summary.family, ProtocolFamily::OpenaiResponses);
        assert!(summary.response_terminal);
        assert_eq!(summary.errors.len(), 1);
        assert_eq!(summary.errors[0].kind, "service_unavailable_error");
        assert!(summary.errors[0].message.contains("overloaded"));
    }

    #[test]
    fn incomplete_and_cancelled_openai_events_are_terminal_with_final_usage() {
        for (event, expected_error) in [
            (
                br#"{"type":"response.incomplete","response":{"usage":{"input_tokens":5,"output_tokens":1},"incomplete_details":{"reason":"max_output_tokens"}}}"#.as_slice(),
                "response_incomplete",
            ),
            (
                br#"{"type":"response.cancelled","response":{"usage":{"input_tokens":6,"output_tokens":2}}}"#.as_slice(),
                "cancelled",
            ),
        ] {
            let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
            assert!(observer.observe_sse_data(event, "10".to_string()));
            let summary = observer.snapshot();
            assert!(summary.response_terminal);
            assert!(summary.token_usage.is_some());
            if expected_error == "cancelled" {
                assert!(summary.errors.is_empty());
                assert_eq!(summary.warnings[0].kind, expected_error);
            } else {
                assert_eq!(summary.errors[0].kind, expected_error);
            }
        }
    }

    #[test]
    fn first_token_is_recorded_once_only_for_recognized_protocols() {
        for url in [
            "https://example.test/v1/responses",
            "https://example.test/v1/messages",
        ] {
            let mut observer = ProtocolObserver::new(Some(url));
            assert!(observer.observe_first_token("10".to_string()));
            assert!(!observer.observe_first_token("20".to_string()));
            assert_eq!(observer.snapshot().first_token_at_ns.as_deref(), Some("10"));
        }

        let mut unknown = ProtocolObserver::new(Some("https://example.test/health"));
        assert!(!unknown.observe_first_token("10".to_string()));
        assert!(unknown.snapshot().first_token_at_ns.is_none());
    }

    #[test]
    fn timeline_end_uses_last_observed_checkpoint_for_interrupted_records() {
        let timing = TimingMetadata {
            upstream_request_started_at_ns: Some("1".to_string()),
            upstream_response_headers_at_ns: Some("9".to_string()),
            ..TimingMetadata::default()
        };
        let mut protocol = ProtocolSummary::for_url(Some("https://example.test/v1/responses"));
        protocol.first_token_at_ns = Some("15".to_string());
        protocol.warnings.push(ProtocolDiagnostic {
            kind: "late_warning".to_string(),
            message: "Observed after the first token".to_string(),
            at_ns: Some("20".to_string()),
        });
        let mut summary = SummaryMetadata::test(String::new(), Some(protocol));
        summary.timing = timing;
        let record = StoredRecord {
            directory: PathBuf::new(),
            sort_key: String::new(),
            request: RequestMetadata {
                format_version: 1,
                id: uuid::Uuid::now_v7().to_string(),
                started_at: String::new(),
                method: String::new(),
                incoming_uri: String::new(),
                upstream_url: None,
                http_version: String::new(),
                headers: Vec::new(),
            },
            response: None,
            summary,
            result: None,
            request_body_bytes: 0,
            response_body_bytes: 0,
            active: false,
            live_elapsed_ns: None,
        };
        assert_eq!(timeline_end_at_ns(&record, None).as_deref(), Some("20"));
    }
}
