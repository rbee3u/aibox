//! Token Usage accumulation, validation, and normalization.
//!
//! Providers report usage in incompatible shapes, and each family's numbers are
//! related by a different formula: OpenAI reports a total that its details are
//! carved out of (`base = total - cached - writes`), while Claude reports a base
//! that its details add to (`total = base + read + writes`). The two are
//! inverses, so [`UsageAccumulator::normalized`] and its validation keep both
//! branches side by side rather than one per provider module — a sign error is
//! only visible when the opposite formula is on screen next to it.
//!
//! Accumulation is field-wise and last-write-wins: a streamed response reports
//! usage in several events, and a later event's value supersedes an earlier one.
//! Nothing is published to the Summary until the protocol response is terminal,
//! so a stream that dies mid-flight leaves no half-summed Token Usage.

use super::wire::UsageEnvelope;
use crate::request::model::{ProtocolFamily, ProtocolSummary, TokenUsage};

/// Raw per-field token counts as reported, before normalization.
///
/// Every field is `Option` because "not reported" and "reported as zero" are
/// different evidence, and this is diagnostic data: collapsing them would
/// invent a number the provider never sent.
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
    /// Record that the request asked for streamed usage.
    pub(super) fn expect_stream_usage(&mut self, expected: bool) {
        self.expects_stream_usage = expected;
    }

    /// True when streamed usage was requested but none ever arrived.
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

    /// Warn when a family's reported numbers contradict each other.
    ///
    /// Each family is checked against its own formula: OpenAI's details must fit
    /// inside its reported total, and Claude's 5m/1h breakdown must sum to its
    /// reported cache-write total.
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

    /// Publish normalized Token Usage onto the Summary, once.
    ///
    /// The first terminal signal wins: a response can reach a terminal event and
    /// then be finalized again by the stream ending, and the published Summary
    /// must not change between them.
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

    /// Project raw counts onto the family-independent [`TokenUsage`] shape.
    ///
    /// The two branches are inverses of each other. OpenAI reports
    /// `total_input` with cached and cache-write carved out of it, so the base
    /// is a subtraction. Claude reports `input_tokens` as the base with cache
    /// reads and writes alongside, so the total is an addition. Every step is
    /// checked, and a contradiction yields `None` for that field rather than a
    /// wrapped or saturated number.
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
                let split_sum = split_reported
                    .then(|| {
                        self.cache_write_5m_tokens
                            .unwrap_or(0)
                            .checked_add(self.cache_write_1h_tokens.unwrap_or(0))
                    })
                    .flatten();
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
                    cache_write_tokens: (!split_valid).then_some(writes).flatten(),
                    cache_write_5m_tokens: split_valid
                        .then_some(self.cache_write_5m_tokens)
                        .flatten(),
                    cache_write_1h_tokens: split_valid
                        .then_some(self.cache_write_1h_tokens)
                        .flatten(),
                    output_tokens: self.output_tokens,
                    reasoning_output_tokens: None,
                })
            }
            ProtocolFamily::Unknown => None,
        }
    }
}
