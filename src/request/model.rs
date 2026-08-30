//! Request evidence and projection value types.
//!
//! This module has no filesystem or network responsibilities. The Request
//! Store persists these values, protocol interpretation enriches them, and
//! assessment derives the Console-facing status from the accumulated evidence.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct RecordedHeader {
    pub name: String,
    pub value_base64: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct RequestMetadata {
    pub format_version: u32,
    pub id: String,
    pub started_at: String,
    pub method: String,
    pub incoming_uri: String,
    pub upstream_url: Option<String>,
    pub http_version: String,
    pub headers: Vec<RecordedHeader>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseSource {
    Upstream,
    Proxy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ResponseMetadata {
    pub format_version: u32,
    pub source: ResponseSource,
    pub headers_at: String,
    pub status: u16,
    pub http_version: String,
    pub headers: Vec<RecordedHeader>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    Completed,
    Rejected,
    UpstreamError,
    ClientDisconnected,
    RecordingFailed,
    ServerShutdown,
}

/// Lifecycle state of a Request, independent from its terminal outcome.
///
/// An interrupted Request has no terminal Outcome because the process stopped
/// before finalization; the Control adapter serializes this same closed set.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub(crate) enum RequestState {
    Active,
    Completed,
    Interrupted,
}

impl RequestState {
    pub(crate) fn from_snapshot(active: bool, terminal: bool) -> Self {
        if active {
            Self::Active
        } else if terminal {
            Self::Completed
        } else {
            Self::Interrupted
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Interrupted => "interrupted",
        }
    }
}

/// Domain-facing name for the terminal result enum. `Outcome` remains as the
/// compatibility spelling used by persistence and protocol code.
pub(crate) type RequestOutcome = Outcome;

impl Outcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Rejected => "rejected",
            Self::UpstreamError => "upstream_error",
            Self::ClientDisconnected => "client_disconnected",
            Self::RecordingFailed => "recording_failed",
            Self::ServerShutdown => "server_shutdown",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ErrorMetadata {
    pub kind: ErrorKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub(crate) enum ErrorKind {
    ClientConfiguration,
    ClientDisconnected,
    ConnectNotSupported,
    ConnectTimeout,
    DnsError,
    EventIndexFailed,
    InvalidTargetUrl,
    NonPublicTarget,
    RecordingFailed,
    RequestBodyFailed,
    RequestRecordingFailed,
    ResponseRecordingFailed,
    ServerShutdown,
    UpgradeNotSupported,
    UpstreamRequestFailed,
    UpstreamResponseFailed,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TimingMetadata {
    pub upstream_request_started_at_ns: Option<String>,
    pub upstream_request_body_first_byte_at_ns: Option<String>,
    pub upstream_request_body_completed_at_ns: Option<String>,
    pub upstream_response_headers_at_ns: Option<String>,
    pub upstream_response_body_first_byte_at_ns: Option<String>,
    pub upstream_response_body_completed_at_ns: Option<String>,
    pub finished_at_ns: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DiagnosticMetadata {
    pub phase: String,
    pub kind: String,
    pub message: String,
    pub at_ns: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct SummaryRequestMetadata {
    pub method: String,
    pub incoming_uri: String,
    pub upstream_url: Option<String>,
    pub http_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct SummaryResponseMetadata {
    pub status: u16,
    pub http_version: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub(crate) enum AssessmentLevel {
    Active,
    Ok,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub(crate) enum AssessmentSource {
    Request,
    Http,
    Provider,
    Diagnostic,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct AssessmentPrimary {
    pub source: AssessmentSource,
    pub kind: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct RequestAssessment {
    pub level: AssessmentLevel,
    pub primary: Option<AssessmentPrimary>,
    pub issue_count: usize,
}

impl RequestAssessment {
    pub(crate) fn active(issue_count: usize) -> Self {
        Self {
            level: AssessmentLevel::Active,
            primary: None,
            issue_count,
        }
    }

    pub(crate) fn ok() -> Self {
        Self {
            level: AssessmentLevel::Ok,
            primary: None,
            issue_count: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct AssessmentFinding {
    pub level: AssessmentLevel,
    pub source: AssessmentSource,
    pub kind: String,
    pub message: String,
    pub phase: Option<String>,
    pub at_ns: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProtocolFamily {
    OpenaiResponses,
    OpenaiChatCompletions,
    ClaudeMessages,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseModeValue {
    Stream,
    Normal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ProtocolDiagnostic {
    pub kind: String,
    pub message: String,
    pub at_ns: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct SummaryMetadata {
    pub schema_version: u32,
    pub request_id: String,
    pub kind: String,
    pub observed_at: String,
    pub request: SummaryRequestMetadata,
    pub response: Option<SummaryResponseMetadata>,
    pub terminal: bool,
    pub timing: TimingMetadata,
    #[serde(default)]
    pub coding_agent_session_id: Option<String>,
    #[serde(default)]
    pub protocol: Option<ProtocolSummary>,
    pub outcome: Option<Outcome>,
    pub errors: Vec<DiagnosticMetadata>,
    pub warnings: Vec<DiagnosticMetadata>,
    pub assessment: RequestAssessment,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ResultMetadata {
    pub format_version: u32,
    pub ended_at: String,
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub request_body_ms: Option<u64>,
    pub total_ms: u64,
    pub outcome: Outcome,
    pub error: Option<ErrorMetadata>,
}

#[derive(Clone, Debug)]
pub(crate) struct TerminalRequestEvent {
    pub(crate) id: String,
    pub(crate) method: String,
    pub(crate) host: String,
    pub(crate) outcome: RequestOutcome,
    pub(crate) assessment_level: AssessmentLevel,
    pub(crate) ended_at: String,
    pub(crate) total_ms: u64,
    pub(crate) error_kind: Option<ErrorKind>,
}

pub(crate) fn utc_now() -> String {
    let format = time::format_description::parse_borrowed::<1>(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:9]Z",
    )
    .expect("static Request timestamp format is valid");
    OffsetDateTime::now_utc()
        .format(&format)
        .unwrap_or_else(|_| "1970-01-01T00:00:00.000000000Z".to_string())
}

pub(crate) fn anchored_at(observed_at: &str, offset_ns: &str) -> Option<String> {
    use time::format_description::well_known::Rfc3339;
    let observed = OffsetDateTime::parse(observed_at, &Rfc3339).ok()?;
    let offset = offset_ns.parse::<i64>().ok()?;
    (observed + time::Duration::nanoseconds(offset))
        .format(&Rfc3339)
        .ok()
}
