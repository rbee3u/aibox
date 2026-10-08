//! Token Usage accumulation and normalization.
//!
//! OpenAI counts include cache details (`base = total - cached - writes`);
//! Claude counts exclude them (`total = base + read + writes`). Keep validation
//! and normalization together to preserve these opposite conventions.
//! Fields are last-write-wins and publish only at protocol termination.

use super::wire::UsageEnvelope;
use crate::request::model::{ProtocolFamily, ProtocolSummary, TokenUsage};

/// Unnormalized provider counts; `None` distinguishes missing evidence from zero.
#[derive(Clone, Debug, Default)]
pub(super) struct UsageAccumulator {
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
    /// Whether any usage evidence has arrived, which is not the same as any
    /// individual field being present.
    reported: bool,
    /// Set when an OpenAI Chat Completions request asked for streamed usage, so
    /// a stream that never reports any is a warning rather than silence.
    expects_stream_usage: bool,
}

fn merge_option(target: &mut Option<u64>, value: Option<u64>) {
    if value.is_some() {
        *target = value;
    }
}

impl UsageAccumulator {
    pub(super) fn expect_stream_usage(&mut self, expected: bool) {
        self.expects_stream_usage = expected;
    }

    pub(super) fn stream_usage_missing(&self) -> bool {
        self.expects_stream_usage && !self.reported
    }

    /// Merge one native usage envelope, then validate the result.
    ///
    /// Returns whether the Summary changed, so a caller can decide to persist.
    pub(super) fn apply(
        &mut self,
        usage: &UsageEnvelope,
        summary: &mut ProtocolSummary,
        at_ns: Option<String>,
    ) -> bool {
        let chat = summary.family == ProtocolFamily::OpenaiChatCompletions;
        merge_option(
            &mut self.input_tokens,
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
            &mut self.cached_tokens,
            input_details.and_then(|value| value.cached_tokens),
        );
        merge_option(
            &mut self.cache_write_tokens,
            input_details.and_then(|value| value.cache_write_tokens),
        );
        let output_details = if chat {
            usage.completion_tokens_details.as_ref()
        } else {
            usage.output_tokens_details.as_ref()
        };
        merge_option(
            &mut self.reasoning_tokens,
            output_details.and_then(|value| value.reasoning_tokens),
        );
        merge_option(
            &mut self.output_tokens,
            if chat {
                usage.completion_tokens
            } else {
                usage.output_tokens
            },
        );
        merge_option(&mut self.total_tokens, usage.total_tokens);
        merge_option(&mut self.cache_read_tokens, usage.cache_read_input_tokens);
        merge_option(
            &mut self.cache_creation_tokens,
            usage.cache_creation_input_tokens,
        );
        let cache_creation = usage.cache_creation.as_ref();
        let five_minute_cache_writes =
            cache_creation.and_then(|value| value.ephemeral_5m_input_tokens);
        let one_hour_cache_writes =
            cache_creation.and_then(|value| value.ephemeral_1h_input_tokens);
        merge_option(
            &mut self.cache_write_5m_tokens,
            five_minute_cache_writes.or(usage.cache_creation_5m_input_tokens),
        );
        merge_option(
            &mut self.cache_write_1h_tokens,
            one_hour_cache_writes.or(usage.cache_creation_1h_input_tokens),
        );
        self.reported = true;
        self.validate(summary, at_ns)
    }

    fn validate(&mut self, summary: &mut ProtocolSummary, at_ns: Option<String>) -> bool {
        match summary.family {
            ProtocolFamily::OpenaiResponses | ProtocolFamily::OpenaiChatCompletions => {
                let mut changed = false;
                if let Some(total) = self.input_tokens {
                    let cached = self.cached_tokens.unwrap_or(0);
                    let writes = self.cache_write_tokens.unwrap_or(0);
                    if total
                        .checked_sub(cached)
                        .and_then(|value| value.checked_sub(writes))
                        .is_none()
                    {
                        changed |= summary.add_warning(
                            "token_usage_inconsistent",
                            "OpenAI input token details exceed the reported total input tokens",
                            at_ns.clone(),
                        );
                    }
                }
                if summary.family == ProtocolFamily::OpenaiChatCompletions
                    && let (Some(input), Some(output), Some(total)) =
                        (self.input_tokens, self.output_tokens, self.total_tokens)
                    && input.checked_add(output) != Some(total)
                {
                    changed |= summary.add_warning(
                        "token_usage_inconsistent",
                        format!(
                            "OpenAI Chat Completions total tokens ({total}) do not equal prompt plus completion tokens ({input} + {output})"
                        ),
                        at_ns,
                    );
                }
                changed
            }
            ProtocolFamily::ClaudeMessages => {
                let split = self
                    .cache_write_5m_tokens
                    .unwrap_or(0)
                    .checked_add(self.cache_write_1h_tokens.unwrap_or(0));
                if let (Some(total), Some(split)) = (self.cache_creation_tokens, split)
                    && (self.cache_write_5m_tokens.is_some()
                        || self.cache_write_1h_tokens.is_some())
                    && total != split
                {
                    return summary.add_warning(
                        "cache_write_breakdown_inconsistent",
                        format!(
                            "Claude cache write total ({total}) does not match the reported 5m/1h breakdown ({split})"
                        ),
                        at_ns,
                    );
                }
                false
            }
            ProtocolFamily::Unknown => false,
        }
    }

    /// Freeze Token Usage at the first terminal signal; a later EOF must not
    /// replace an already-published value.
    pub(super) fn commit(&self, summary: &mut ProtocolSummary) -> bool {
        if summary.token_usage.is_some() {
            return false;
        }
        let Some(usage) = self.normalized(summary.family) else {
            return false;
        };
        summary.token_usage = Some(usage);
        true
    }

    /// Normalize provider counts using the formulas above. Inconsistent or
    /// overflowing arithmetic leaves the affected field unknown.
    fn normalized(&self, family: ProtocolFamily) -> Option<TokenUsage> {
        if !self.reported {
            return None;
        }
        match family {
            ProtocolFamily::OpenaiResponses | ProtocolFamily::OpenaiChatCompletions => {
                let total = self.input_tokens;
                let cached = self.cached_tokens;
                let writes = self.cache_write_tokens;
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
                    output_tokens: self.output_tokens,
                    reasoning_output_tokens: self.reasoning_tokens,
                    ..TokenUsage::default()
                })
            }
            ProtocolFamily::ClaudeMessages => {
                let split_reported =
                    self.cache_write_5m_tokens.is_some() || self.cache_write_1h_tokens.is_some();
                let split_sum = if split_reported {
                    self.cache_write_5m_tokens
                        .unwrap_or(0)
                        .checked_add(self.cache_write_1h_tokens.unwrap_or(0))
                } else {
                    None
                };
                let writes = self.cache_creation_tokens.or(split_sum);
                let split_valid = split_reported && writes.is_some() && split_sum == writes;
                let total = self.input_tokens.and_then(|base| {
                    base.checked_add(self.cache_read_tokens.unwrap_or(0))?
                        .checked_add(writes.unwrap_or(0))
                });
                Some(TokenUsage {
                    total_input_tokens: total,
                    base_input_tokens: self.input_tokens,
                    cached_input_tokens: self.cache_read_tokens,
                    cache_write_tokens: writes.filter(|_| !split_valid),
                    cache_write_5m_tokens: self.cache_write_5m_tokens.filter(|_| split_valid),
                    cache_write_1h_tokens: self.cache_write_1h_tokens.filter(|_| split_valid),
                    output_tokens: self.output_tokens,
                    reasoning_output_tokens: None,
                })
            }
            ProtocolFamily::Unknown => None,
        }
    }
}
