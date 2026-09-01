//! Pure validation and projection over a stored Summary document.
//!
//! These functions operate on [`SummaryMetadata`] without filesystem access and
//! are shared by the store layout, read, and write paths.

use super::{ErrorKind, ErrorMetadata, FORMAT_VERSION, Outcome, ResultMetadata, SummaryMetadata};
use crate::request::assessment::calculate_assessment;
use anyhow::{Result, bail};
use time::OffsetDateTime;

pub(super) fn validate_schema(version: u32, kind: &str, expected: &str) -> Result<()> {
    if version != FORMAT_VERSION {
        bail!("unsupported Request schema version {version}");
    }
    if kind != expected {
        bail!("Request metadata kind is not {expected}");
    }
    Ok(())
}

pub(super) fn validate_summary(summary: &SummaryMetadata) -> Result<()> {
    if summary.terminal != summary.outcome.is_some() {
        bail!("Request summary terminal and outcome fields are inconsistent");
    }
    if summary.terminal && summary.timing.finished_at_ns.is_none() {
        bail!("terminal Request summary has no finished timing");
    }
    if summary.request.method.is_empty() || summary.request.http_version.is_empty() {
        bail!("Request summary request projection is incomplete");
    }
    if summary
        .protocol
        .as_ref()
        .is_some_and(|protocol| protocol.token_usage.is_some() && !protocol.response_terminal)
    {
        bail!("Request protocol summary has final Token Usage before a terminal response");
    }
    let expected_assessment = calculate_assessment(summary, !summary.terminal, false);
    if summary.assessment != expected_assessment {
        bail!("Request summary assessment is inconsistent with its evidence");
    }
    let protocol_offsets = summary.protocol.as_ref().into_iter().flat_map(|protocol| {
        std::iter::once(protocol.first_token_at_ns.as_deref())
            .chain(
                protocol
                    .errors
                    .iter()
                    .chain(&protocol.warnings)
                    .map(|diagnostic| diagnostic.at_ns.as_deref()),
            )
            .flatten()
    });
    for value in [
        summary.timing.upstream_request_started_at_ns.as_deref(),
        summary
            .timing
            .upstream_request_body_first_byte_at_ns
            .as_deref(),
        summary
            .timing
            .upstream_request_body_completed_at_ns
            .as_deref(),
        summary.timing.upstream_response_headers_at_ns.as_deref(),
        summary
            .timing
            .upstream_response_body_first_byte_at_ns
            .as_deref(),
        summary
            .timing
            .upstream_response_body_completed_at_ns
            .as_deref(),
        summary.timing.finished_at_ns.as_deref(),
    ]
    .into_iter()
    .flatten()
    .chain(protocol_offsets)
    {
        if value.parse::<u128>().is_err() {
            bail!("Request summary timing offset is not a decimal string");
        }
    }
    Ok(())
}

pub(super) fn summary_to_result(summary: &SummaryMetadata) -> ResultMetadata {
    let outcome = summary.outcome.unwrap_or(Outcome::RecordingFailed);
    let total_ms = summary
        .timing
        .finished_at_ns
        .as_deref()
        .and_then(|value| value.parse::<u128>().ok())
        .map(|ns| (ns / 1_000_000) as u64)
        .unwrap_or_default();
    let error = summary.errors.last().map(|error| ErrorMetadata {
        kind: parse_error_kind(&error.kind),
        message: error.message.clone(),
    });
    ResultMetadata {
        format_version: FORMAT_VERSION,
        ended_at: summary_ended_at(summary),
        request_bytes: 0,
        response_bytes: 0,
        request_body_ms: None,
        total_ms,
        outcome,
        error,
    }
}

pub(super) fn summary_ended_at(summary: &SummaryMetadata) -> String {
    let Some(offset) = summary
        .timing
        .finished_at_ns
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok())
    else {
        return summary.observed_at.clone();
    };
    let format = time::format_description::well_known::Rfc3339;
    let Some(observed) = OffsetDateTime::parse(&summary.observed_at, &format).ok() else {
        return summary.observed_at.clone();
    };
    (observed + time::Duration::nanoseconds(offset))
        .format(&format)
        .unwrap_or_else(|_| summary.observed_at.clone())
}

fn parse_error_kind(kind: &str) -> ErrorKind {
    serde_json::from_str(&format!("\"{kind}\"")).unwrap_or(ErrorKind::RecordingFailed)
}

pub(super) fn error_phase(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::ClientConfiguration
        | ErrorKind::ConnectNotSupported
        | ErrorKind::ConnectTimeout
        | ErrorKind::DnsError
        | ErrorKind::InvalidTargetUrl
        | ErrorKind::NonPublicTarget
        | ErrorKind::RequestBodyFailed
        | ErrorKind::RequestRecordingFailed
        | ErrorKind::UpgradeNotSupported
        | ErrorKind::UpstreamRequestFailed => "request",
        ErrorKind::ClientDisconnected
        | ErrorKind::ResponseRecordingFailed
        | ErrorKind::UpstreamResponseFailed => "response",
        ErrorKind::EventIndexFailed | ErrorKind::RecordingFailed => "recording",
        ErrorKind::ServerShutdown => "lifecycle",
    }
}
