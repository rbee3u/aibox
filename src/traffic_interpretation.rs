use crate::traffic_store::{RecordedHeader, StoredRecord};
use anyhow::{bail, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

#[cfg(test)]
use std::fs;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ProtocolFamily {
    OpenaiResponses,
    ClaudeMessages,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResponseModeValue {
    Stream,
    Normal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct RequestedEffective<T> {
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
pub(super) struct RequestedObserved<T> {
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
pub(super) struct TokenUsage {
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
pub(super) struct ProtocolDiagnostic {
    pub kind: String,
    pub message: String,
    pub at_ns: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct ProtocolSummary {
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
    pub(super) fn for_url(url: Option<&str>) -> Self {
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
pub(super) struct ProtocolObserver {
    summary: ProtocolSummary,
    usage: UsageAccumulator,
    has_usage: bool,
}

impl ProtocolObserver {
    pub(super) fn new(url: Option<&str>) -> Self {
        Self {
            summary: ProtocolSummary::for_url(url),
            ..Self::default()
        }
    }

    pub(super) fn snapshot(&self) -> ProtocolSummary {
        self.summary.clone()
    }

    pub(super) fn observe_request(
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
            ProtocolFamily::ClaudeMessages => envelope.output_config.and_then(|value| value.effort),
            ProtocolFamily::Unknown => None,
        };
        changed |= self
            .summary
            .set_requested_effort(nonempty(effort), Some(at_ns.clone()));
        changed |= self.summary.set_requested_mode(
            Some(if envelope.stream.unwrap_or(false) {
                ResponseModeValue::Stream
            } else {
                ResponseModeValue::Normal
            }),
            Some(at_ns),
        );
        changed
    }

    pub(super) fn observe_response_headers(
        &mut self,
        headers: &[RecordedHeader],
        event_stream: bool,
        at_ns: String,
    ) -> bool {
        let mut changed = self.summary.set_observed_mode(
            Some(if event_stream {
                ResponseModeValue::Stream
            } else {
                ResponseModeValue::Normal
            }),
            Some(at_ns.clone()),
        );
        let model =
            header_text(headers, "openai-model").or_else(|| header_text(headers, "x-openai-model"));
        changed |= self
            .summary
            .set_effective_model(nonempty(model), Some(at_ns));
        changed
    }

    pub(super) fn observe_sse_data(&mut self, data: &[u8], at_ns: String) -> bool {
        if data.is_empty() || data == b"[DONE]" {
            return false;
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
        self.apply_event(event, at_ns)
    }

    pub(super) fn observe_json_response(
        &mut self,
        path: &Path,
        status: u16,
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
        let parsed =
            serde_json::from_reader::<_, JsonResponseEnvelope>(file).context("parse response JSON");
        let mut changed = false;
        match parsed {
            Ok(envelope) => {
                let family = if envelope.object.as_deref() == Some("response") {
                    ProtocolFamily::OpenaiResponses
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
                    changed |= self.apply_usage(usage, Some(at_ns.clone()));
                }
                if let Some(error) = envelope.error {
                    let (kind, message) = error_parts(&error, "api_error", "Upstream API error");
                    changed |= self.summary.add_error(kind, message, Some(at_ns.clone()));
                }
                if let Some(reason) = envelope.incomplete_details.and_then(|value| value.reason) {
                    changed |= self.summary.add_error(
                        "response_incomplete",
                        format!("OpenAI response was incomplete: {reason}"),
                        Some(at_ns.clone()),
                    );
                }
            }
            Err(error) if self.summary.family != ProtocolFamily::Unknown => {
                changed |= self.summary.add_warning(
                    "response_interpretation_failed",
                    format!("Cannot interpret model response metadata: {error:#}"),
                    Some(at_ns.clone()),
                );
            }
            Err(_) => {}
        }
        if self.summary.family != ProtocolFamily::Unknown && !self.summary.response_terminal {
            self.summary.response_terminal = true;
            changed = true;
        }
        changed |= self.commit_final_usage();
        changed | self.observe_http_status(status, at_ns)
    }

    pub(super) fn observe_http_status(&mut self, status: u16, at_ns: String) -> bool {
        if status < 400 || !self.summary.errors.is_empty() {
            return false;
        }
        self.summary.add_error(
            format!("http_{status}"),
            format!(
                "Upstream returned HTTP {status}; the response body was not recognized as an OpenAI Responses or Claude Messages error. See the Response tab for the raw body."
            ),
            Some(at_ns),
        )
    }

    fn apply_event(&mut self, event: StreamEvent, at_ns: String) -> bool {
        let kind = event.kind.as_deref().unwrap_or_default();
        let is_output_bearing = output_bearing(&event);
        let family = if kind.starts_with("response.") {
            ProtocolFamily::OpenaiResponses
        } else if matches!(
            kind,
            "message_start"
                | "message_delta"
                | "message_stop"
                | "content_block_start"
                | "content_block_delta"
                | "content_block_stop"
        ) {
            ProtocolFamily::ClaudeMessages
        } else {
            ProtocolFamily::Unknown
        };
        let mut changed = self.summary.set_family(family, Some(at_ns.clone()));

        if let Some(response) = event.response {
            changed |= self
                .summary
                .set_effective_model(nonempty(response.model), Some(at_ns.clone()));
            changed |= self
                .summary
                .set_effective_effort(nonempty(response.reasoning_effort), Some(at_ns.clone()));
            if let Some(usage) = response.usage {
                changed |= self.apply_usage(usage, Some(at_ns.clone()));
            }
            if let Some(error) = response.error {
                let (error_kind, message) =
                    error_parts(&error, "api_error", "OpenAI response error");
                changed |= self
                    .summary
                    .add_error(error_kind, message, Some(at_ns.clone()));
            }
            if let Some(reason) = response.incomplete_details.and_then(|value| value.reason) {
                changed |= self.summary.add_error(
                    "response_incomplete",
                    format!("OpenAI response was incomplete: {reason}"),
                    Some(at_ns.clone()),
                );
            }
        }

        if let Some(message) = event.message.as_ref().and_then(Value::as_object) {
            changed |= self.summary.set_effective_model(
                message
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .and_then(|value| nonempty(Some(value))),
                Some(at_ns.clone()),
            );
            if let Some(usage) = message.get("usage") {
                match serde_json::from_value::<UsageEnvelope>(usage.clone()) {
                    Ok(usage) => changed |= self.apply_usage(usage, Some(at_ns.clone())),
                    Err(error) => {
                        changed |= self.summary.add_warning(
                            "token_usage_invalid",
                            format!("Cannot parse message_start usage: {error}"),
                            Some(at_ns.clone()),
                        );
                    }
                }
            }
        }
        if let Some(usage) = event.usage {
            changed |= self.apply_usage(usage, Some(at_ns.clone()));
        }

        if self.summary.first_token_at_ns.is_none() && is_output_bearing {
            self.summary.first_token_at_ns = Some(at_ns.clone());
            changed = true;
        }

        if kind == "error" {
            if let Some(error) = event.error.as_ref() {
                let (error_kind, message) = error_parts(error, "api_error", "Upstream API error");
                changed |= self
                    .summary
                    .add_error(error_kind, message, Some(at_ns.clone()));
            } else if let Some(message) = event.message.as_ref().and_then(Value::as_str) {
                changed |= self.summary.add_error(
                    event.code.as_deref().unwrap_or("api_error"),
                    message,
                    Some(at_ns.clone()),
                );
            }
        }

        let terminal = matches!(
            kind,
            "response.completed"
                | "response.failed"
                | "response.incomplete"
                | "response.cancelled"
                | "message_stop"
        );
        if terminal && !self.summary.response_terminal {
            self.summary.response_terminal = true;
            changed = true;
        }
        if terminal {
            changed |= self.commit_final_usage();
        }
        if matches!(kind, "response.failed" | "response.cancelled")
            && !self
                .summary
                .errors
                .iter()
                .any(|error| error.at_ns.as_deref() == Some(at_ns.as_str()))
        {
            changed |= self.summary.add_error(
                kind.trim_start_matches("response."),
                format!("OpenAI stream ended with {kind}"),
                Some(at_ns),
            );
        }
        changed
    }

    fn apply_usage(&mut self, usage: UsageEnvelope, at_ns: Option<String>) -> bool {
        merge_option(&mut self.usage.input_tokens, usage.input_tokens);
        merge_option(
            &mut self.usage.cached_tokens,
            usage
                .input_tokens_details
                .as_ref()
                .and_then(|value| value.cached_tokens),
        );
        merge_option(
            &mut self.usage.cache_write_tokens,
            usage
                .input_tokens_details
                .as_ref()
                .and_then(|value| value.cache_write_tokens),
        );
        merge_option(
            &mut self.usage.reasoning_tokens,
            usage
                .output_tokens_details
                .as_ref()
                .and_then(|value| value.reasoning_tokens),
        );
        merge_option(&mut self.usage.output_tokens, usage.output_tokens);
        merge_option(
            &mut self.usage.cache_read_tokens,
            usage.cache_read_input_tokens,
        );
        merge_option(
            &mut self.usage.cache_creation_tokens,
            usage.cache_creation_input_tokens,
        );
        let nested_5m = usage
            .cache_creation
            .as_ref()
            .and_then(|value| value.ephemeral_5m_input_tokens);
        let nested_1h = usage
            .cache_creation
            .as_ref()
            .and_then(|value| value.ephemeral_1h_input_tokens);
        merge_option(
            &mut self.usage.cache_write_5m_tokens,
            nested_5m.or(usage.cache_creation_5m_input_tokens),
        );
        merge_option(
            &mut self.usage.cache_write_1h_tokens,
            nested_1h.or(usage.cache_creation_1h_input_tokens),
        );
        self.has_usage = true;
        self.validate_usage(at_ns)
    }

    fn validate_usage(&mut self, at_ns: Option<String>) -> bool {
        match self.summary.family {
            ProtocolFamily::OpenaiResponses => {
                if let Some(total) = self.usage.input_tokens {
                    let cached = self.usage.cached_tokens.unwrap_or(0);
                    let writes = self.usage.cache_write_tokens.unwrap_or(0);
                    if total
                        .checked_sub(cached)
                        .and_then(|value| value.checked_sub(writes))
                        .is_none()
                    {
                        return self.summary.add_warning(
                            "token_usage_inconsistent",
                            "OpenAI input token details exceed the reported total input tokens",
                            at_ns,
                        );
                    }
                }
            }
            ProtocolFamily::ClaudeMessages => {
                let split = self
                    .usage
                    .cache_write_5m_tokens
                    .unwrap_or(0)
                    .checked_add(self.usage.cache_write_1h_tokens.unwrap_or(0));
                if let (Some(total), Some(split)) = (self.usage.cache_creation_tokens, split) {
                    if (self.usage.cache_write_5m_tokens.is_some()
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
            ProtocolFamily::OpenaiResponses => {
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
                let writes = self.usage.cache_creation_tokens.or_else(|| {
                    self.usage
                        .cache_write_5m_tokens
                        .unwrap_or(0)
                        .checked_add(self.usage.cache_write_1h_tokens.unwrap_or(0))
                });
                let split_sum = self
                    .usage
                    .cache_write_5m_tokens
                    .unwrap_or(0)
                    .checked_add(self.usage.cache_write_1h_tokens.unwrap_or(0));
                let split_valid = (self.usage.cache_write_5m_tokens.is_some()
                    || self.usage.cache_write_1h_tokens.is_some())
                    && writes.is_some()
                    && split_sum == writes;
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
}

fn merge_option(target: &mut Option<u64>, value: Option<u64>) {
    if value.is_some() {
        *target = value;
    }
}

fn parse_request(path: &Path, headers: &[RecordedHeader]) -> Result<RequestEnvelope> {
    let file = crate::tenant::open_real_file(path, "Traffic request body")?;
    let encoding = header_text(headers, "content-encoding");
    match encoding.as_deref().map(str::trim) {
        None | Some("") | Some("identity") => serde_json::from_reader(file),
        Some("zstd") => {
            let decoder =
                zstd::stream::read::Decoder::new(file).context("create zstd request decoder")?;
            serde_json::from_reader(decoder)
        }
        Some(value) => bail!("unsupported request Content-Encoding {value:?}"),
    }
    .context("parse request JSON")
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
    } else if path.ends_with("/messages") {
        ProtocolFamily::ClaudeMessages
    } else {
        ProtocolFamily::Unknown
    }
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

#[derive(Debug, Deserialize)]
struct RequestEnvelope {
    model: Option<String>,
    stream: Option<bool>,
    reasoning: Option<EffortEnvelope>,
    output_config: Option<EffortEnvelope>,
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
    cache_read_input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_creation: Option<CacheCreationDetails>,
    cache_creation_5m_input_tokens: Option<u64>,
    cache_creation_1h_input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    output_tokens_details: Option<OutputTokenDetails>,
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
    response: Option<ResponseEnvelope>,
    message: Option<Value>,
    usage: Option<UsageEnvelope>,
    delta: Option<Value>,
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
    error: Option<Value>,
    incomplete_details: Option<IncompleteDetails>,
}

fn output_bearing(event: &StreamEvent) -> bool {
    let kind = event.kind.as_deref().unwrap_or_default();
    if kind == "content_block_delta" {
        let Some(delta) = event.delta.as_ref() else {
            return false;
        };
        let delta_kind = delta.get("type").and_then(Value::as_str);
        return match delta_kind {
            Some("text_delta") => nonempty_value(delta.get("text")),
            Some("thinking_delta") => nonempty_value(delta.get("thinking")),
            Some("input_json_delta") => nonempty_value(delta.get("partial_json")),
            _ => false,
        };
    }
    matches!(
        kind,
        "response.output_text.delta"
            | "response.refusal.delta"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_text.delta"
            | "response.function_call_arguments.delta"
            | "response.custom_tool_call_input.delta"
            | "response.mcp_call_arguments.delta"
            | "response.code_interpreter_call_code.delta"
            | "response.audio.delta"
            | "response.audio.transcript.delta"
    ) && nonempty_value(event.delta.as_ref())
}

fn nonempty_value(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(value)) => !value.is_empty(),
        Some(Value::Object(value)) => !value.is_empty(),
        Some(Value::Null) | None => false,
        Some(_) => true,
    }
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

pub(super) fn timeline_end_at_ns(record: &StoredRecord, live: Option<String>) -> Option<String> {
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
            value_base64: base64::engine::general_purpose::STANDARD.encode("zstd"),
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
        assert!(observer.observe_response_headers(&headers, false, "10".to_string()));
        assert!(observer.observe_json_response(&response_path, 200, "20".to_string()));
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
    fn malformed_protocol_data_warns_without_hiding_http_error() {
        let temp = tempfile::tempdir().unwrap();
        let response_path = temp.path().join("response.json");
        fs::write(&response_path, b"not json").unwrap();
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
        assert!(observer.observe_sse_data(b"not json", "10".to_string()));
        assert!(observer.observe_json_response(&response_path, 503, "20".to_string()));
        let summary = observer.snapshot();
        assert!(summary.response_terminal);
        assert_eq!(summary.warnings.len(), 2);
        assert_eq!(summary.errors.len(), 1);
        assert_eq!(summary.errors[0].kind, "http_503");

        let mut unknown = ProtocolObserver::new(Some("https://example.test/health"));
        assert!(unknown.observe_http_status(502, "30".to_string()));
        let summary = unknown.snapshot();
        assert_eq!(summary.family, ProtocolFamily::Unknown);
        assert!(!summary.response_terminal);
        assert_eq!(summary.errors[0].kind, "http_502");
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
            assert_eq!(summary.errors[0].kind, expected_error);
        }
    }

    #[test]
    fn only_nonempty_model_deltas_are_output_bearing() {
        for (json, expected) in [
            (
                r#"{"type":"response.output_text.delta","delta":"hi"}"#,
                true,
            ),
            (r#"{"type":"response.output_text.delta","delta":""}"#, false),
            (
                r#"{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"work"}}"#,
                true,
            ),
            (
                r#"{"type":"content_block_delta","delta":{"type":"signature_delta","signature":"sig"}}"#,
                false,
            ),
            (
                r#"{"type":"response.audio.transcript.delta","delta":"hello"}"#,
                true,
            ),
        ] {
            let event: StreamEvent = serde_json::from_str(json).unwrap();
            assert_eq!(output_bearing(&event), expected, "{json}");
        }

        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
        observer.observe_sse_data(
            br#"{"type":"response.output_text.delta","delta":""}"#,
            "10".to_string(),
        );
        observer.observe_sse_data(
            br#"{"type":"response.output_text.delta","delta":"hello"}"#,
            "20".to_string(),
        );
        observer.observe_sse_data(
            br#"{"type":"response.output_text.delta","delta":"again"}"#,
            "30".to_string(),
        );
        assert_eq!(observer.snapshot().first_token_at_ns.as_deref(), Some("20"));
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
        let record = StoredRecord {
            directory: PathBuf::new(),
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
            summary: SummaryMetadata {
                schema_version: 1,
                record_id: String::new(),
                kind: "summary".to_string(),
                observed_at: String::new(),
                terminal: false,
                timing,
                protocol: Some(protocol),
                outcome: None,
                errors: Vec::new(),
                warnings: Vec::new(),
            },
            result: None,
            request_body_bytes: 0,
            response_body_bytes: 0,
            active: false,
        };
        assert_eq!(timeline_end_at_ns(&record, None).as_deref(), Some("20"));
    }
}
