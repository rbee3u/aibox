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
//! Request Outcome untouched. See
//! `docs/adr/0009-request-evidence-and-projections.md`.

mod claude;
mod http;
mod openai;
mod usage;
mod wire;

use self::http::{family_from_url, header_text, nonempty, parse_request, trim_ascii};
use self::usage::{UsageAccumulator, merge_option};
use self::wire::{
    ChoiceEnvelope, JsonResponseEnvelope, ResponseEnvelope, StreamEvent, UsageEnvelope, error_parts,
};
pub(crate) use crate::request::model::{
    ProtocolDiagnostic, ProtocolFamily, ProtocolSummary, ResponseModeValue,
};
use crate::request::model::{RecordedHeader, TokenUsage};
use anyhow::Context as _;
pub(crate) use http::{BodyContentCoding, body_content_coding, coding_agent_session_id};
use serde_json::Value;
use std::path::Path;

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
        let file = match crate::foundation::safe_fs::open_real_file(path, "Upstream Response body")
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
    openai::event_family(kind, event_name, object)
        .or_else(|| claude::event_family(kind, event_name))
        .unwrap_or(ProtocolFamily::Unknown)
}

fn is_terminal_event_kind(value: &str) -> bool {
    openai::is_terminal_event_kind(value) || claude::is_terminal_event_kind(value)
}

#[cfg(test)]
#[path = "interpretation_tests.rs"]
mod tests;
