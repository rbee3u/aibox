use crate::traffic_interpretation::{
    ProtocolFamily, ProtocolSummary, ResponseModeValue, coding_agent_session_id,
};
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use uuid::Uuid;

pub(super) const FORMAT_VERSION: u32 = 2;
const REQUEST_JSON: &str = "request.json";
const REQUEST_BODY: &str = "request.body";
const RESPONSE_JSON: &str = "response.json";
const RESPONSE_BODY: &str = "response.body";
const RESPONSE_EVENTS_JSONL: &str = "response.events.jsonl";
const SUMMARY_JSON: &str = "summary.json";
const RESULT_JSON: &str = "result.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct RecordedHeader {
    pub name: String,
    pub value_base64: String,
}

impl RecordedHeader {
    pub fn from_headers(headers: &axum::http::HeaderMap) -> Vec<Self> {
        let connection_named: HashSet<String> = headers
            .get_all(axum::http::header::CONNECTION)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_ascii_lowercase)
            })
            .collect();
        headers
            .iter()
            .filter(|(name, _)| {
                !is_hop_by_hop(name.as_str()) && !connection_named.contains(name.as_str())
            })
            .map(|(name, value)| Self {
                name: name.as_str().to_string(),
                value_base64: base64::engine::general_purpose::STANDARD.encode(value.as_bytes()),
            })
            .collect()
    }
}

fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "connection"
            | "proxy-connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct RequestMetadata {
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
#[serde(rename_all = "snake_case")]
pub(super) enum ResponseSource {
    Upstream,
    Proxy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ResponseMetadata {
    pub format_version: u32,
    pub source: ResponseSource,
    pub headers_at: String,
    pub status: u16,
    pub http_version: String,
    pub headers: Vec<RecordedHeader>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Outcome {
    Completed,
    Rejected,
    UpstreamError,
    ClientDisconnected,
    RecordingFailed,
    ServerShutdown,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
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
pub(super) struct ErrorMetadata {
    pub kind: ErrorKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ErrorKind {
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
pub(super) struct TimingMetadata {
    pub upstream_request_started_at_ns: Option<String>,
    pub upstream_request_body_first_byte_at_ns: Option<String>,
    pub upstream_request_body_completed_at_ns: Option<String>,
    pub upstream_response_headers_at_ns: Option<String>,
    pub upstream_response_body_first_byte_at_ns: Option<String>,
    pub upstream_response_body_completed_at_ns: Option<String>,
    pub finished_at_ns: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct DiagnosticMetadata {
    pub phase: String,
    pub kind: String,
    pub message: String,
    pub at_ns: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct SummaryRequestMetadata {
    pub method: String,
    pub incoming_uri: String,
    pub upstream_url: Option<String>,
    pub http_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct SummaryResponseMetadata {
    pub status: u16,
    pub http_version: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AssessmentLevel {
    Active,
    Ok,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum AssessmentSource {
    Traffic,
    Http,
    Provider,
    Diagnostic,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct AssessmentPrimary {
    pub source: AssessmentSource,
    pub kind: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct RecordAssessment {
    pub level: AssessmentLevel,
    pub primary: Option<AssessmentPrimary>,
    pub issue_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct AssessmentFinding {
    pub level: AssessmentLevel,
    pub source: AssessmentSource,
    pub kind: String,
    pub message: String,
    pub phase: Option<String>,
    pub at_ns: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct SummaryMetadata {
    pub schema_version: u32,
    pub record_id: String,
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
    pub assessment: RecordAssessment,
}

#[cfg(test)]
impl SummaryMetadata {
    pub(super) fn test(record_id: impl Into<String>, protocol: Option<ProtocolSummary>) -> Self {
        Self {
            schema_version: FORMAT_VERSION,
            record_id: record_id.into(),
            kind: "summary".to_string(),
            observed_at: "2026-08-06T04:00:00Z".to_string(),
            request: SummaryRequestMetadata {
                method: "GET".to_string(),
                incoming_uri: "/test".to_string(),
                upstream_url: None,
                http_version: "HTTP/1.1".to_string(),
            },
            response: None,
            terminal: false,
            timing: TimingMetadata::default(),
            coding_agent_session_id: None,
            protocol,
            outcome: None,
            errors: Vec::new(),
            warnings: Vec::new(),
            assessment: RecordAssessment::active(0),
        }
    }
}

#[derive(Clone)]
pub(super) struct SummaryHandle {
    inner: Arc<Mutex<SummaryMetadata>>,
}

impl SummaryHandle {
    pub(super) fn new(summary: SummaryMetadata) -> Self {
        Self {
            inner: Arc::new(Mutex::new(summary)),
        }
    }

    pub(super) fn update<R>(&self, update: impl FnOnce(&mut SummaryMetadata) -> R) -> R {
        let mut summary = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        update(&mut summary)
    }

    pub(super) fn read<R>(&self, read: impl FnOnce(&SummaryMetadata) -> R) -> R {
        let summary = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        read(&summary)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ResultMetadata {
    pub format_version: u32,
    pub ended_at: String,
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub request_body_ms: Option<u64>,
    pub total_ms: u64,
    pub outcome: Outcome,
    pub error: Option<ErrorMetadata>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct RuntimeMeasurements {
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub request_body_duration: Option<Duration>,
}

#[derive(Clone)]
pub(super) struct TrafficStore {
    root: PathBuf,
    active: Arc<Mutex<HashMap<String, Instant>>>,
    namespace: Arc<RwLock<()>>,
}

#[derive(Clone, Debug)]
pub(super) struct RecordLocator {
    inner: Arc<Mutex<PathBuf>>,
    host: Arc<str>,
}

impl RecordLocator {
    fn new(path: PathBuf, host: String) -> Self {
        Self {
            inner: Arc::new(Mutex::new(path)),
            host: host.into(),
        }
    }

    pub(super) fn path(&self) -> PathBuf {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn set_path(&self, path: PathBuf) {
        *self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = path;
    }
}

pub(super) struct NewRecord {
    pub id: String,
    // The creation path is retained for tests and diagnostics. Runtime path-based
    // operations must use `locator`, because terminalization renames the directory.
    #[allow(dead_code)]
    pub directory: PathBuf,
    pub locator: RecordLocator,
    pub request_body: fs::File,
    pub response_body: fs::File,
    pub summary: SummaryHandle,
    pub origin: Instant,
}

#[derive(Clone, Debug)]
pub(super) struct StoredRecord {
    pub directory: PathBuf,
    pub sort_key: String,
    pub request: RequestMetadata,
    pub response: Option<ResponseMetadata>,
    pub summary: SummaryMetadata,
    pub result: Option<ResultMetadata>,
    pub request_body_bytes: u64,
    pub response_body_bytes: u64,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub(super) struct StoredRecordSummary {
    pub sort_key: String,
    pub summary: SummaryMetadata,
    pub active: bool,
}

impl RecordAssessment {
    fn active(issue_count: usize) -> Self {
        Self {
            level: AssessmentLevel::Active,
            primary: None,
            issue_count,
        }
    }

    fn ok() -> Self {
        Self {
            level: AssessmentLevel::Ok,
            primary: None,
            issue_count: 0,
        }
    }
}

pub(super) fn effective_assessment(summary: &SummaryMetadata, active: bool) -> RecordAssessment {
    calculate_assessment(summary, active, !summary.terminal && !active)
}

pub(super) fn diagnostic_findings(
    summary: &SummaryMetadata,
    interrupted: bool,
) -> Vec<AssessmentFinding> {
    let mut findings = Vec::new();

    for error in &summary.errors {
        let level = if matches!(
            error.kind.as_str(),
            "client_disconnected" | "request_body_failed" | "event_index_failed"
        ) {
            AssessmentLevel::Warning
        } else {
            AssessmentLevel::Error
        };
        push_finding(
            &mut findings,
            AssessmentFinding {
                level,
                source: AssessmentSource::Traffic,
                kind: error.kind.clone(),
                message: error.message.clone(),
                phase: Some(error.phase.clone()),
                at_ns: Some(error.at_ns.clone()),
            },
        );
    }

    if summary.errors.is_empty()
        && let Some(outcome) = summary.outcome
        && outcome != Outcome::Completed
    {
        let level = if outcome == Outcome::ClientDisconnected {
            AssessmentLevel::Warning
        } else {
            AssessmentLevel::Error
        };
        push_finding(
            &mut findings,
            AssessmentFinding {
                level,
                source: AssessmentSource::Traffic,
                kind: outcome.as_str().to_string(),
                message: outcome_fallback_message(outcome).to_string(),
                phase: None,
                at_ns: summary.timing.finished_at_ns.clone(),
            },
        );
    }

    if let Some(response) = &summary.response
        && response.status >= 400
    {
        push_finding(
            &mut findings,
            AssessmentFinding {
                level: AssessmentLevel::Error,
                source: AssessmentSource::Http,
                kind: format!("http_{}", response.status),
                message: format!("Upstream returned HTTP {}", response.status),
                phase: Some("response".to_string()),
                at_ns: summary.timing.upstream_response_headers_at_ns.clone(),
            },
        );
    }

    if let Some(protocol) = &summary.protocol {
        for error in &protocol.errors {
            push_finding(
                &mut findings,
                AssessmentFinding {
                    level: AssessmentLevel::Error,
                    source: AssessmentSource::Provider,
                    kind: error.kind.clone(),
                    message: error.message.clone(),
                    phase: Some("model_api".to_string()),
                    at_ns: error.at_ns.clone(),
                },
            );
        }
        for warning in &protocol.warnings {
            push_finding(
                &mut findings,
                AssessmentFinding {
                    level: AssessmentLevel::Warning,
                    source: if warning.kind == "cancelled" {
                        AssessmentSource::Provider
                    } else {
                        AssessmentSource::Diagnostic
                    },
                    kind: warning.kind.clone(),
                    message: warning.message.clone(),
                    phase: Some("model_api".to_string()),
                    at_ns: warning.at_ns.clone(),
                },
            );
        }
        let streaming = protocol.response_mode.observed == Some(ResponseModeValue::Stream)
            || (protocol.response_mode.observed.is_none()
                && protocol.response_mode.requested == Some(ResponseModeValue::Stream));
        if summary.terminal
            && summary.outcome == Some(Outcome::Completed)
            && protocol.family != ProtocolFamily::Unknown
            && streaming
            && !protocol.response_terminal
        {
            push_finding(
                &mut findings,
                AssessmentFinding {
                    level: AssessmentLevel::Warning,
                    source: AssessmentSource::Diagnostic,
                    kind: "model_response_terminal_not_observed".to_string(),
                    message: "The recognized model stream ended without a terminal protocol event"
                        .to_string(),
                    phase: Some("model_api".to_string()),
                    at_ns: summary
                        .timing
                        .upstream_response_body_completed_at_ns
                        .clone(),
                },
            );
        }
    }

    for warning in &summary.warnings {
        push_finding(
            &mut findings,
            AssessmentFinding {
                level: AssessmentLevel::Warning,
                source: AssessmentSource::Diagnostic,
                kind: warning.kind.clone(),
                message: warning.message.clone(),
                phase: Some(warning.phase.clone()),
                at_ns: Some(warning.at_ns.clone()),
            },
        );
    }

    if interrupted {
        push_finding(
            &mut findings,
            AssessmentFinding {
                level: AssessmentLevel::Warning,
                source: AssessmentSource::Traffic,
                kind: "interrupted".to_string(),
                message: "Traffic Proxy stopped before the Traffic Record was finalized"
                    .to_string(),
                phase: None,
                at_ns: None,
            },
        );
    }

    findings
}

fn calculate_assessment(
    summary: &SummaryMetadata,
    active: bool,
    interrupted: bool,
) -> RecordAssessment {
    let findings = diagnostic_findings(summary, interrupted);
    if active {
        return RecordAssessment::active(findings.len());
    }
    let Some(primary) = findings
        .iter()
        .min_by_key(|finding| finding_sort_key(finding))
    else {
        return RecordAssessment::ok();
    };
    RecordAssessment {
        level: primary.level,
        primary: Some(AssessmentPrimary {
            source: primary.source,
            kind: primary.kind.clone(),
            message: primary.message.clone(),
        }),
        issue_count: findings.len(),
    }
}

fn refresh_assessment(summary: &mut SummaryMetadata) {
    summary.assessment = calculate_assessment(summary, !summary.terminal, false);
}

fn push_finding(findings: &mut Vec<AssessmentFinding>, finding: AssessmentFinding) {
    if let Some(existing) = findings.iter_mut().find(|existing| {
        existing.source == finding.source
            && existing.kind == finding.kind
            && existing.message == finding.message
    }) {
        if offset_key(finding.at_ns.as_deref()) < offset_key(existing.at_ns.as_deref()) {
            *existing = finding;
        }
        return;
    }
    findings.push(finding);
}

fn finding_sort_key(finding: &AssessmentFinding) -> (u8, u8, u128) {
    let severity = match finding.level {
        AssessmentLevel::Error => 0,
        AssessmentLevel::Warning => 1,
        AssessmentLevel::Active | AssessmentLevel::Ok => 2,
    };
    let source = if finding.source == AssessmentSource::Traffic
        && matches!(
            finding.kind.as_str(),
            "recording_failed" | "request_recording_failed" | "response_recording_failed"
        ) {
        0
    } else {
        match finding.source {
            AssessmentSource::Provider => 1,
            AssessmentSource::Traffic => 2,
            AssessmentSource::Http => 3,
            AssessmentSource::Diagnostic => 4,
        }
    };
    (severity, source, offset_key(finding.at_ns.as_deref()))
}

fn offset_key(value: Option<&str>) -> u128 {
    value
        .and_then(|value| value.parse().ok())
        .unwrap_or(u128::MAX)
}

fn outcome_fallback_message(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Completed => "The proxy attempt completed",
        Outcome::Rejected => "The proxy rejected the upstream request",
        Outcome::UpstreamError => "The upstream request or response failed",
        Outcome::ClientDisconnected => "The client disconnected before the proxy attempt completed",
        Outcome::RecordingFailed => "The Traffic Record could not be recorded completely",
        Outcome::ServerShutdown => "Traffic Proxy stopped before the attempt completed",
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StoredEventTiming {
    pub sequence: u64,
    pub completed_at_ns: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StoredEventTimings {
    pub available: bool,
    pub partial: bool,
    pub events: Vec<StoredEventTiming>,
    pub next_sequence: u64,
    pub warning: Option<String>,
}

pub(super) enum RecordDetailReadError {
    Lookup(anyhow::Error),
    EventIndex(anyhow::Error),
}

impl TrafficStore {
    pub fn open(aibox_root: &Path) -> Result<Self> {
        crate::tenant::ensure_real_dir(aibox_root, "aibox root")?;
        let root = aibox_root.join("traffic");
        crate::tenant::ensure_real_dir(&root, "Traffic Record collection")?;
        restrict_dir(&root)?;
        Ok(Self {
            root,
            active: Arc::new(Mutex::new(HashMap::new())),
            namespace: Arc::new(RwLock::new(())),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn begin(
        &self,
        method: &str,
        incoming_uri: &str,
        upstream_url: Option<&str>,
        version: &str,
        headers: Vec<RecordedHeader>,
        host_hint: Option<&str>,
    ) -> Result<(NewRecord, RequestMetadata)> {
        let _namespace = self
            .namespace
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        crate::tenant::real_dir_exists(&self.root, "Traffic Record collection")?;
        let id = Uuid::now_v7().to_string();
        let observed_at = utc_now();
        let origin = Instant::now();
        let coding_agent_session_id = coding_agent_session_id(upstream_url, &headers);
        let host = sanitize_host(host_hint.unwrap_or("invalid"));
        let directory_name = format!("active-{}-{host}-{id}", utc_basic_at(&observed_at)?);
        let directory = self.root.join(directory_name);
        let locator = RecordLocator::new(directory.clone(), host);
        fs::create_dir(&directory)
            .with_context(|| format!("create Traffic Record {}", directory.display()))?;
        restrict_dir(&directory)?;
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone(), origin);

        let created = (|| -> Result<_> {
            let request_body = create_private_file(&directory.join(REQUEST_BODY))?;
            let response_body = create_private_file(&directory.join(RESPONSE_BODY))?;
            let request = RequestMetadata {
                format_version: FORMAT_VERSION,
                id: id.clone(),
                started_at: observed_at.clone(),
                method: method.to_string(),
                incoming_uri: incoming_uri.to_string(),
                upstream_url: upstream_url.map(str::to_string),
                http_version: version.to_string(),
                headers,
            };
            let file = RequestFile {
                schema_version: FORMAT_VERSION,
                record_id: id.clone(),
                kind: "request".to_string(),
                method: request.method.clone(),
                upstream_url: request.upstream_url.clone(),
                headers: request.headers.clone(),
            };
            let summary = SummaryMetadata {
                schema_version: FORMAT_VERSION,
                record_id: id.clone(),
                kind: "summary".to_string(),
                observed_at,
                request: SummaryRequestMetadata {
                    method: request.method.clone(),
                    incoming_uri: request.incoming_uri.clone(),
                    upstream_url: request.upstream_url.clone(),
                    http_version: request.http_version.clone(),
                },
                response: None,
                terminal: false,
                timing: TimingMetadata::default(),
                coding_agent_session_id,
                protocol: Some(ProtocolSummary::for_url(upstream_url)),
                outcome: None,
                errors: Vec::new(),
                warnings: Vec::new(),
                assessment: RecordAssessment::active(0),
            };
            atomic_write_json(&directory, REQUEST_JSON, &file)?;
            atomic_write_json(&directory, SUMMARY_JSON, &summary)?;
            crate::tenant::sync_dir(&directory)?;
            crate::tenant::sync_dir(&self.root)?;
            Ok((
                request_body,
                response_body,
                request,
                SummaryHandle::new(summary),
            ))
        })();
        let (request_body, response_body, request, summary) = match created {
            Ok(value) => value,
            Err(error) => {
                self.active
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&id);
                let _ = remove_controlled_record_dir(&directory);
                return Err(error);
            }
        };
        Ok((
            NewRecord {
                id,
                directory,
                locator,
                request_body,
                response_body,
                summary,
                origin,
            },
            request,
        ))
    }

    pub fn update_summary(
        &self,
        locator: &RecordLocator,
        handle: &SummaryHandle,
        update: impl FnOnce(&mut SummaryMetadata) -> bool,
    ) -> Result<bool> {
        let _namespace = self
            .namespace
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = locator.path();
        validate_record_ancestor(&self.root, &directory)?;
        let mut summary = handle
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if summary.terminal {
            return Ok(false);
        }
        let changed = update(&mut summary);
        if changed {
            refresh_assessment(&mut summary);
            atomic_write_json(&directory, SUMMARY_JSON, &*summary)?;
        }
        Ok(changed)
    }

    pub fn write_response(
        &self,
        locator: &RecordLocator,
        handle: &SummaryHandle,
        metadata: &ResponseMetadata,
    ) -> Result<()> {
        let _namespace = self
            .namespace
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = locator.path();
        validate_record_ancestor(&self.root, &directory)?;
        let mut summary = handle
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !summary.terminal {
            summary.response = Some(SummaryResponseMetadata {
                status: metadata.status,
                http_version: metadata.http_version.clone(),
            });
            refresh_assessment(&mut summary);
            atomic_write_json(&directory, SUMMARY_JSON, &*summary)?;
        }
        let record_id = summary.record_id.clone();
        drop(summary);
        let file = ResponseFile {
            schema_version: FORMAT_VERSION,
            record_id,
            kind: "response".to_string(),
            http_version: metadata.http_version.clone(),
            status: metadata.status,
            headers: metadata.headers.clone(),
        };
        atomic_write_json(&directory, RESPONSE_JSON, &file)
    }

    pub fn create_event_index(&self, record: &NewRecord) -> Result<fs::File> {
        let _namespace = self
            .namespace
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = record.locator.path();
        validate_record_ancestor(&self.root, &directory)?;
        create_private_file(&directory.join(RESPONSE_EVENTS_JSONL))
    }

    pub fn with_record_path<R>(
        &self,
        locator: &RecordLocator,
        operation: impl FnOnce(&Path) -> R,
    ) -> Result<R> {
        let _namespace = self
            .namespace
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = locator.path();
        validate_record_ancestor(&self.root, &directory)?;
        Ok(operation(&directory))
    }

    pub fn finish(
        &self,
        record: &NewRecord,
        started: Instant,
        measurements: &RuntimeMeasurements,
        outcome: Outcome,
        error: Option<ErrorMetadata>,
    ) -> Result<ResultMetadata> {
        let _namespace = self
            .namespace
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let directory = record.locator.path();
        validate_record_ancestor(&self.root, &directory)?;
        let at_ns = offset_ns(record.origin);
        let mut summary = record
            .summary
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if summary.terminal {
            let snapshot = summary.clone();
            drop(summary);
            let ended_at = summary_ended_at(&snapshot);
            self.finalize_directory_unlocked(record, &ended_at);
            self.active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&record.id);
            let mut result = summary_to_result(&snapshot);
            result.request_bytes = measurements.request_bytes;
            result.response_bytes = measurements.response_bytes;
            result.request_body_ms = measurements.request_body_duration.map(duration_ms);
            return Ok(result);
        }
        let previous = summary.clone();
        summary.timing.finished_at_ns = Some(at_ns.clone());
        summary.terminal = true;
        summary.outcome = Some(outcome);
        if let Some(error) = &error {
            summary.errors.push(DiagnosticMetadata {
                phase: error_phase(error.kind).to_string(),
                kind: serde_json::to_string(&error.kind)
                    .unwrap_or_else(|_| "recording_failed".to_string())
                    .trim_matches('"')
                    .to_string(),
                message: error.message.clone(),
                at_ns: at_ns.clone(),
            });
        }
        refresh_assessment(&mut summary);
        if let Err(error) = atomic_write_json(&directory, SUMMARY_JSON, &*summary) {
            if terminal_summary_matches(&directory, &record.id, outcome, &at_ns) {
                eprintln!(
                    "warning: finalized Traffic Record {} but cannot sync terminal summary: {error:#}",
                    record.id
                );
            } else {
                *summary = previous;
                return Err(error);
            }
        }
        let snapshot = summary.clone();
        drop(summary);
        let ended_at = summary_ended_at(&snapshot);
        self.finalize_directory_unlocked(record, &ended_at);
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&record.id);
        let total_ms = snapshot
            .timing
            .finished_at_ns
            .as_deref()
            .and_then(|value| value.parse::<u128>().ok())
            .map(|ns| (ns / 1_000_000) as u64)
            .unwrap_or_else(|| duration_ms(started.elapsed()));
        Ok(ResultMetadata {
            format_version: FORMAT_VERSION,
            ended_at,
            request_bytes: measurements.request_bytes,
            response_bytes: measurements.response_bytes,
            request_body_ms: measurements.request_body_duration.map(duration_ms),
            total_ms,
            outcome,
            error,
        })
    }

    fn finalize_directory_unlocked(&self, record: &NewRecord, ended_at: &str) {
        let directory = record.locator.path();
        let target = match utc_basic_at(ended_at) {
            Ok(timestamp) => self
                .root
                .join(format!("{timestamp}-{}-{}", record.locator.host, record.id)),
            Err(error) => {
                eprintln!(
                    "warning: finalized Traffic Record {} but cannot format its terminal directory: {error:#}",
                    record.id
                );
                return;
            }
        };
        if directory == target {
            return;
        }
        match rename_noreplace(&directory, &target) {
            Ok(()) => {
                record.locator.set_path(target.clone());
                if let Err(error) = crate::tenant::sync_dir(&self.root) {
                    eprintln!(
                        "warning: finalized Traffic Record {} but cannot sync renamed directory {}: {error:#}",
                        record.id,
                        target.display()
                    );
                }
            }
            Err(error) => eprintln!(
                "warning: finalized Traffic Record {} but cannot rename {} to {}: {error:#}",
                record.id,
                directory.display(),
                target.display()
            ),
        }
    }

    pub fn abandon_active(&self, id: &str) {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn scan(&self) -> Result<Vec<StoredRecord>> {
        let _namespace = self
            .namespace
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.scan_unlocked()
    }

    pub fn scan_summaries(&self) -> Result<Vec<StoredRecordSummary>> {
        let _namespace = self
            .namespace
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.scan_summaries_unlocked()
    }

    fn scan_summaries_unlocked(&self) -> Result<Vec<StoredRecordSummary>> {
        if !crate::tenant::real_dir_exists(&self.root, "Traffic Record collection")? {
            return Ok(Vec::new());
        }
        let active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("read Traffic Record collection {}", self.root.display()))?
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    eprintln!("warning: cannot inspect Traffic Record entry: {error}");
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    eprintln!(
                        "warning: cannot inspect Traffic Record {}: {error}",
                        path.display()
                    );
                    continue;
                }
            };
            if !metadata.file_type().is_dir() {
                eprintln!(
                    "warning: ignoring unexpected Traffic entry {}",
                    path.display()
                );
                continue;
            }
            match read_record_summary(&path, &active) {
                Ok(record) => records.push(record),
                Err(error) => eprintln!(
                    "warning: ignoring incomplete or invalid Traffic Record {}: {error:#}",
                    path.display()
                ),
            }
        }
        records.sort_by(|left, right| right.sort_key.cmp(&left.sort_key));
        Ok(records)
    }

    fn scan_unlocked(&self) -> Result<Vec<StoredRecord>> {
        if !crate::tenant::real_dir_exists(&self.root, "Traffic Record collection")? {
            return Ok(Vec::new());
        }
        let active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("read Traffic Record collection {}", self.root.display()))?
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    eprintln!("warning: cannot inspect Traffic Record entry: {error}");
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    eprintln!(
                        "warning: cannot inspect Traffic Record {}: {error}",
                        path.display()
                    );
                    continue;
                }
            };
            if !metadata.file_type().is_dir() {
                eprintln!(
                    "warning: ignoring unexpected Traffic entry {}",
                    path.display()
                );
                continue;
            }
            match read_record(&path, &active) {
                Ok(record) => records.push(record),
                Err(error) => eprintln!(
                    "warning: ignoring incomplete or invalid Traffic Record {}: {error:#}",
                    path.display()
                ),
            }
        }
        records.sort_by(|left, right| right.sort_key.cmp(&left.sort_key));
        Ok(records)
    }

    // Explicit record operations only inspect directory names carrying the
    // requested UUID. A malformed matching entry is an error, while unrelated
    // collection entries retain the tolerant listing behavior above.
    fn find_unlocked(&self, id: &str) -> Result<StoredRecord> {
        let mut ids = HashSet::new();
        ids.insert(id);
        self.find_many_unlocked(&ids)?
            .remove(id)
            .with_context(|| format!("Traffic Record not found: {id}"))
    }

    fn find_many_unlocked(&self, ids: &HashSet<&str>) -> Result<HashMap<String, StoredRecord>> {
        if !crate::tenant::real_dir_exists(&self.root, "Traffic Record collection")? {
            return Ok(HashMap::new());
        }
        let active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut records = HashMap::with_capacity(ids.len());
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("read Traffic Record collection {}", self.root.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(id_start) = name.len().checked_sub(36) else {
                continue;
            };
            if name
                .as_bytes()
                .get(id_start.checked_sub(1).unwrap_or(usize::MAX))
                != Some(&b'-')
            {
                continue;
            }
            let Some(candidate) = name.get(id_start..) else {
                continue;
            };
            if !ids.contains(candidate) {
                continue;
            }
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("inspect selected Traffic Record {}", path.display()))?;
            if !metadata.file_type().is_dir() {
                bail!(
                    "selected Traffic Record is not a real directory: {}",
                    path.display()
                );
            }
            let record = read_record(&path, &active)
                .with_context(|| format!("read selected Traffic Record {}", path.display()))?;
            if record.request.id != candidate {
                bail!("selected Traffic Record metadata id does not match its directory name");
            }
            if records.insert(candidate.to_string(), record).is_some() {
                bail!("multiple Traffic Record directories match id {candidate}");
            }
        }
        Ok(records)
    }

    pub fn find(&self, id: &str) -> Result<StoredRecord> {
        let _namespace = self
            .namespace
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        validate_id(id)?;
        self.find_unlocked(id)
    }

    pub fn live_elapsed_ns(&self, id: &str) -> Option<String> {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .copied()
            .map(offset_ns)
    }

    pub fn open_body(&self, id: &str, response: bool, offset: u64) -> Result<(fs::File, u64)> {
        let _namespace = self
            .namespace
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        validate_id(id)?;
        let record = self.find_unlocked(id)?;
        self.open_record_body_unlocked(&record, response, offset)
    }

    pub fn open_record_body(
        &self,
        record: &StoredRecord,
        response: bool,
        offset: u64,
    ) -> Result<(fs::File, u64)> {
        let _namespace = self
            .namespace
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = self.find_unlocked(&record.request.id)?;
        self.open_record_body_unlocked(&current, response, offset)
    }

    fn open_record_body_unlocked(
        &self,
        record: &StoredRecord,
        response: bool,
        offset: u64,
    ) -> Result<(fs::File, u64)> {
        validate_record_ancestor(&self.root, &record.directory)?;
        let path = record.directory.join(if response {
            RESPONSE_BODY
        } else {
            REQUEST_BODY
        });
        validate_regular_file(&path, "Traffic body")?;
        let mut file = crate::tenant::open_real_file(&path, "Traffic body")?;
        let length = file.metadata()?.len();
        if offset > length {
            bail!("body offset {offset} exceeds current length {length}");
        }
        file.seek(SeekFrom::Start(offset))?;
        Ok((file, length))
    }

    pub fn read_event_timings(&self, id: &str, after_sequence: u64) -> Result<StoredEventTimings> {
        let _namespace = self
            .namespace
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        validate_id(id)?;
        let record = self.find_unlocked(id)?;
        validate_record_ancestor(&self.root, &record.directory)?;
        let path = record.directory.join(RESPONSE_EVENTS_JSONL);
        if !crate::tenant::real_file_exists(&path, "Traffic SSE event index")? {
            return Ok(StoredEventTimings {
                available: false,
                partial: false,
                events: Vec::new(),
                next_sequence: after_sequence,
                warning: Some("SSE Event timing index is unavailable".to_string()),
            });
        }

        let file = crate::tenant::open_real_file(&path, "Traffic SSE event index")?;
        let mut reader = std::io::BufReader::new(file);
        let mut events = Vec::new();
        let mut warnings = Vec::new();
        let mut next_sequence = after_sequence;
        let mut line_number = 0usize;
        loop {
            let mut line = Vec::new();
            let read = reader.read_until(b'\n', &mut line)?;
            if read == 0 {
                break;
            }
            line_number += 1;
            let terminated = line.last() == Some(&b'\n');
            if terminated {
                line.pop();
            } else if record.active {
                break;
            }
            if line.is_empty() {
                continue;
            }
            match serde_json::from_slice::<EventIndexLine>(&line) {
                Ok(entry) if event_index_entry_valid(&entry, &record.request.id) => {
                    next_sequence = next_sequence.max(entry.sequence.saturating_add(1));
                    if entry.sequence >= after_sequence {
                        events.push(StoredEventTiming {
                            sequence: entry.sequence,
                            completed_at_ns: entry.completed_at_ns,
                        });
                    }
                }
                Ok(_) => warnings.push(format!(
                    "line {line_number}: SSE Event timing index line has invalid metadata"
                )),
                Err(error) => warnings.push(format!(
                    "line {line_number}: cannot parse SSE Event timing index line: {error}"
                )),
            }
        }
        let warning = match warnings.as_slice() {
            [] => None,
            [warning] => Some(warning.clone()),
            [first, ..] => Some(format!(
                "{first}; {} additional timing index lines are invalid",
                warnings.len() - 1
            )),
        };
        Ok(StoredEventTimings {
            available: true,
            partial: warning.is_some(),
            events,
            next_sequence,
            warning,
        })
    }

    pub fn find_with_event_index_warnings(
        &self,
        id: &str,
    ) -> std::result::Result<StoredRecord, RecordDetailReadError> {
        let _namespace = self
            .namespace
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        validate_id(id).map_err(RecordDetailReadError::Lookup)?;
        let mut record = self
            .find_unlocked(id)
            .map_err(RecordDetailReadError::Lookup)?;
        append_event_index_warnings(&record.directory, &mut record.summary, record.active)
            .map_err(RecordDetailReadError::EventIndex)?;
        Ok(record)
    }

    pub fn delete_ids(&self, ids: &[String]) -> Result<usize> {
        let _namespace = self
            .namespace
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if ids.is_empty() {
            bail!("at least one Traffic Record id is required");
        }
        let unique: HashSet<_> = ids.iter().collect();
        if unique.len() != ids.len() {
            bail!("Traffic Record ids must not be repeated");
        }
        for id in ids {
            validate_id(id)?;
        }
        let active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if ids.iter().any(|id| active.contains_key(id)) {
            bail!("active Traffic Records cannot be deleted");
        }
        let requested: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let records = self.find_many_unlocked(&requested)?;
        let mut selected = Vec::new();
        for id in ids {
            let record = records
                .get(id)
                .with_context(|| format!("Traffic Record not found: {id}"))?;
            if record.active {
                bail!("active Traffic Records cannot be deleted");
            }
            validate_record_ancestor(&self.root, &record.directory)?;
            selected.push(record.directory.clone());
        }
        for path in &selected {
            remove_controlled_record_dir(path)?;
        }
        crate::tenant::sync_dir(&self.root)?;
        Ok(selected.len())
    }

    pub fn delete_all(&self, expected: usize) -> Result<usize> {
        let _namespace = self
            .namespace
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let records: Vec<_> = self
            .scan_unlocked()?
            .into_iter()
            .filter(|record| !record.active)
            .collect();
        if records.len() != expected {
            bail!(
                "deletable Traffic Record count changed (expected {expected}, now {})",
                records.len()
            );
        }
        for record in &records {
            validate_record_ancestor(&self.root, &record.directory)?;
        }
        for record in &records {
            remove_controlled_record_dir(&record.directory)?;
        }
        crate::tenant::sync_dir(&self.root)?;
        Ok(records.len())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RequestFile {
    schema_version: u32,
    record_id: String,
    kind: String,
    method: String,
    upstream_url: Option<String>,
    headers: Vec<RecordedHeader>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ResponseFile {
    schema_version: u32,
    record_id: String,
    kind: String,
    http_version: String,
    status: u16,
    headers: Vec<RecordedHeader>,
}

fn read_record_summary(
    path: &Path,
    active: &HashMap<String, Instant>,
) -> Result<StoredRecordSummary> {
    let summary: SummaryMetadata = read_json(&path.join(SUMMARY_JSON), "Traffic summary metadata")?;
    validate_schema(summary.schema_version, &summary.kind, "summary")?;
    validate_id(&summary.record_id)?;
    let directory = parse_record_directory_name(path, &summary.record_id)?;
    validate_summary(&summary)?;
    let active_record = !summary.terminal && active.contains_key(&summary.record_id);
    Ok(StoredRecordSummary {
        sort_key: canonical_sort_key(&summary, &directory.host, &summary.record_id)?,
        summary,
        active: active_record,
    })
}

fn read_record(path: &Path, active: &HashMap<String, Instant>) -> Result<StoredRecord> {
    let request_file: RequestFile =
        read_json(&path.join(REQUEST_JSON), "Traffic request metadata")?;
    validate_schema(request_file.schema_version, &request_file.kind, "request")?;
    validate_id(&request_file.record_id)?;
    let directory = parse_record_directory_name(path, &request_file.record_id)?;
    let summary: SummaryMetadata = read_json(&path.join(SUMMARY_JSON), "Traffic summary metadata")?;
    validate_schema(summary.schema_version, &summary.kind, "summary")?;
    if summary.record_id != request_file.record_id {
        bail!("Traffic metadata record ids do not match");
    }
    validate_summary(&summary)?;
    if summary.request.method != request_file.method
        || summary.request.upstream_url != request_file.upstream_url
    {
        bail!("Traffic request metadata does not match its Summary projection");
    }
    let _ = crate::tenant::real_file_exists(
        &path.join(RESPONSE_EVENTS_JSONL),
        "Traffic SSE event index",
    )?;
    if crate::tenant::real_file_exists(path.join(RESULT_JSON).as_path(), "legacy result metadata")?
    {
        bail!("legacy result.json is unsupported");
    }
    let response_file: Option<ResponseFile> =
        optional_json(&path.join(RESPONSE_JSON), "Traffic response metadata")?;
    if let Some(response) = &response_file {
        validate_schema(response.schema_version, &response.kind, "response")?;
        if response.record_id != request_file.record_id {
            bail!("Traffic response record id does not match");
        }
    }
    match (&summary.response, &response_file) {
        (None, None) => {}
        (Some(projected), Some(response))
            if projected.status == response.status
                && projected.http_version == response.http_version => {}
        _ => bail!("Traffic response metadata does not match its Summary projection"),
    }
    let request_body_bytes = regular_file_length(&path.join(REQUEST_BODY), "Traffic request body")?;
    let response_body_bytes =
        regular_file_length(&path.join(RESPONSE_BODY), "Traffic response body")?;
    let request = RequestMetadata {
        format_version: FORMAT_VERSION,
        id: request_file.record_id.clone(),
        started_at: summary.observed_at.clone(),
        method: summary.request.method.clone(),
        incoming_uri: summary.request.incoming_uri.clone(),
        upstream_url: summary.request.upstream_url.clone(),
        http_version: summary.request.http_version.clone(),
        headers: request_file.headers,
    };
    let response = response_file.map(|metadata| ResponseMetadata {
        format_version: FORMAT_VERSION,
        source: ResponseSource::Upstream,
        headers_at: summary
            .timing
            .upstream_response_headers_at_ns
            .as_deref()
            .and_then(|offset| anchored_at(&summary.observed_at, offset))
            .unwrap_or_else(|| summary.observed_at.clone()),
        status: metadata.status,
        http_version: metadata.http_version,
        headers: metadata.headers,
    });
    let active_record = !summary.terminal && active.contains_key(&request.id);
    let result = summary.terminal.then(|| {
        let mut result = summary_to_result(&summary);
        result.request_bytes = request_body_bytes;
        result.response_bytes = response_body_bytes;
        result
    });
    Ok(StoredRecord {
        directory: path.to_path_buf(),
        sort_key: canonical_sort_key(&summary, &directory.host, &request.id)?,
        request,
        response,
        summary,
        result,
        request_body_bytes,
        response_body_bytes,
        active: active_record,
    })
}

fn validate_schema(version: u32, kind: &str, expected: &str) -> Result<()> {
    if version != FORMAT_VERSION {
        bail!("unsupported Traffic schema version {version}");
    }
    if kind != expected {
        bail!("Traffic metadata kind is not {expected}");
    }
    Ok(())
}

fn validate_summary(summary: &SummaryMetadata) -> Result<()> {
    if summary.terminal != summary.outcome.is_some() {
        bail!("Traffic summary terminal and outcome fields are inconsistent");
    }
    if summary.terminal && summary.timing.finished_at_ns.is_none() {
        bail!("terminal Traffic summary has no finished timing");
    }
    if summary.request.method.is_empty() || summary.request.http_version.is_empty() {
        bail!("Traffic summary request projection is incomplete");
    }
    if summary
        .protocol
        .as_ref()
        .is_some_and(|protocol| protocol.token_usage.is_some() && !protocol.response_terminal)
    {
        bail!("Traffic protocol summary has final Token Usage before a terminal response");
    }
    let expected_assessment = calculate_assessment(summary, !summary.terminal, false);
    if summary.assessment != expected_assessment {
        bail!("Traffic summary assessment is inconsistent with its evidence");
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
            bail!("Traffic summary timing offset is not a decimal string");
        }
    }
    Ok(())
}

fn summary_to_result(summary: &SummaryMetadata) -> ResultMetadata {
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

fn terminal_summary_matches(path: &Path, id: &str, outcome: Outcome, finished_at_ns: &str) -> bool {
    read_json::<SummaryMetadata>(&path.join(SUMMARY_JSON), "Traffic summary metadata").is_ok_and(
        |summary| {
            summary.record_id == id
                && summary.terminal
                && summary.outcome == Some(outcome)
                && summary.timing.finished_at_ns.as_deref() == Some(finished_at_ns)
        },
    )
}

fn summary_ended_at(summary: &SummaryMetadata) -> String {
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

fn error_phase(kind: ErrorKind) -> &'static str {
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

#[derive(Deserialize)]
struct EventIndexLine {
    schema_version: u32,
    record_id: String,
    kind: String,
    sequence: u64,
    body_start: u64,
    body_end: u64,
    first_arrival_at_ns: String,
    completed_at_ns: String,
}

fn event_index_entry_valid(entry: &EventIndexLine, record_id: &str) -> bool {
    entry.schema_version == FORMAT_VERSION
        && entry.record_id == record_id
        && entry.kind == "sse_event"
        && entry.body_start <= entry.body_end
        && entry.first_arrival_at_ns.parse::<u128>().is_ok()
        && entry.completed_at_ns.parse::<u128>().is_ok()
}

fn append_event_index_warnings(
    path: &Path,
    summary: &mut SummaryMetadata,
    active: bool,
) -> Result<()> {
    let index_path = path.join(RESPONSE_EVENTS_JSONL);
    if !crate::tenant::real_file_exists(&index_path, "Traffic SSE event index")? {
        return Ok(());
    }
    let file = crate::tenant::open_real_file(&index_path, "Traffic SSE event index")?;
    let mut reader = std::io::BufReader::new(file);
    let mut line_number = 0usize;
    loop {
        let mut line = Vec::new();
        let read = match reader.read_until(b'\n', &mut line) {
            Ok(read) => read,
            Err(error) => {
                let warning = event_index_warning(
                    summary,
                    line_number + 1,
                    format!("cannot read SSE event index line: {error}"),
                );
                summary.warnings.push(warning);
                break;
            }
        };
        if read == 0 {
            break;
        }
        line_number += 1;
        let terminated = line.last() == Some(&b'\n');
        if terminated {
            line.pop();
        } else if active {
            break;
        }
        if line.is_empty() {
            continue;
        }
        let warning = match serde_json::from_slice::<EventIndexLine>(&line) {
            Ok(entry) if event_index_entry_valid(&entry, &summary.record_id) => {
                let _ = entry.sequence;
                continue;
            }
            Ok(_) => "SSE event index line has invalid metadata".to_string(),
            Err(error) => format!("cannot parse SSE event index line: {error}"),
        };
        let warning = event_index_warning(summary, line_number, warning);
        summary.warnings.push(warning);
    }
    Ok(())
}

fn event_index_warning(
    summary: &SummaryMetadata,
    line_number: usize,
    message: String,
) -> DiagnosticMetadata {
    DiagnosticMetadata {
        phase: "recording".to_string(),
        kind: "event_index_failed".to_string(),
        message: format!("line {line_number}: {message}"),
        at_ns: summary
            .timing
            .finished_at_ns
            .clone()
            .unwrap_or_else(|| "0".to_string()),
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, kind: &str) -> Result<T> {
    let file = crate::tenant::open_real_file(path, kind)?;
    serde_json::from_reader(file).with_context(|| format!("parse {kind} {}", path.display()))
}

fn optional_json<T: serde::de::DeserializeOwned>(path: &Path, kind: &str) -> Result<Option<T>> {
    if !crate::tenant::real_file_exists(path, kind)? {
        return Ok(None);
    }
    read_json(path, kind).map(Some)
}

fn regular_file_length(path: &Path, kind: &str) -> Result<u64> {
    validate_regular_file(path, kind)?;
    Ok(fs::symlink_metadata(path)?.len())
}

fn validate_regular_file(path: &Path, kind: &str) -> Result<()> {
    if !crate::tenant::real_file_exists(path, kind)? {
        bail!("{kind} does not exist: {}", path.display());
    }
    Ok(())
}

fn validate_record_ancestor(root: &Path, directory: &Path) -> Result<()> {
    if directory.parent() != Some(root) {
        bail!("Traffic Record is not a direct child of the Traffic collection");
    }
    if !crate::tenant::real_dir_exists(root, "Traffic Record collection")?
        || !crate::tenant::real_dir_exists(directory, "Traffic Record")?
    {
        bail!("Traffic Record disappeared: {}", directory.display());
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    let parsed = Uuid::parse_str(id).with_context(|| format!("invalid Traffic Record id: {id}"))?;
    if parsed.get_version_num() != 7 {
        bail!("Traffic Record id is not UUID v7: {id}");
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RecordDirectoryName {
    host: String,
}

fn parse_record_directory_name(path: &Path, id: &str) -> Result<RecordDirectoryName> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Traffic Record directory name is not valid UTF-8")?;
    let suffix = format!("-{id}");
    let prefix = name
        .strip_suffix(&suffix)
        .context("Traffic Record directory name does not match its UUID")?;
    let prefix = prefix.strip_prefix("active-").unwrap_or(prefix);
    let (timestamp, host) = prefix
        .split_once('-')
        .context("Traffic Record directory name has no host slug")?;
    let timestamp = timestamp.as_bytes();
    let timestamp_is_valid = timestamp.len() == 20
        && timestamp[8] == b'T'
        && timestamp[15] == b'.'
        && timestamp[19] == b'Z'
        && timestamp
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 15 | 19) || byte.is_ascii_digit());
    let host_is_valid = (1..=48).contains(&host.len())
        && !host.starts_with(['.', '-'])
        && !host.ends_with(['.', '-'])
        && host.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        });
    if !timestamp_is_valid || !host_is_valid {
        bail!("Traffic Record directory name is not structurally valid");
    }
    Ok(RecordDirectoryName {
        host: host.to_string(),
    })
}

fn canonical_sort_key(summary: &SummaryMetadata, host: &str, id: &str) -> Result<String> {
    if summary.terminal {
        Ok(format!(
            "{}-{host}-{id}",
            utc_basic_at(&summary_ended_at(summary))?
        ))
    } else {
        Ok(format!(
            "active-{}-{host}-{id}",
            utc_basic_at(&summary.observed_at)?
        ))
    }
}

fn create_private_file(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).read(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .with_context(|| format!("create private Traffic file {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn atomic_write_json(path: &Path, name: &str, value: &impl Serialize) -> Result<()> {
    let temporary = path.join(format!(".{name}.{}.tmp", Uuid::new_v4()));
    let final_path = path.join(name);
    let result = (|| -> Result<()> {
        let mut file = create_private_file(&temporary)?;
        serde_json::to_writer_pretty(&mut file, value)
            .with_context(|| format!("serialize Traffic metadata {}", final_path.display()))?;
        file.write_all(b"\n")?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temporary, &final_path).with_context(|| {
            format!(
                "publish Traffic metadata {} as {}",
                temporary.display(),
                final_path.display()
            )
        })?;
        crate::tenant::sync_dir(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn remove_controlled_record_dir(path: &Path) -> Result<()> {
    if !crate::tenant::real_dir_exists(path, "Traffic Record")? {
        bail!("Traffic Record disappeared: {}", path.display());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.file_type().is_file() {
            bail!(
                "refusing to delete Traffic Record with unsafe entry: {}",
                entry.path().display()
            );
        }
        files.push(entry.path());
    }
    for file in files {
        fs::remove_file(&file)
            .with_context(|| format!("delete Traffic file {}", file.display()))?;
    }
    fs::remove_dir(path).with_context(|| format!("delete Traffic Record {}", path.display()))
}

fn restrict_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("chmod 0700 {}", path.display()))?;
    }
    Ok(())
}

pub(super) fn utc_now() -> String {
    let format = time::format_description::parse_borrowed::<1>(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:9]Z",
    )
    .expect("static Traffic timestamp format is valid");
    OffsetDateTime::now_utc()
        .format(&format)
        .unwrap_or_else(|_| "1970-01-01T00:00:00.000000000Z".to_string())
}

pub(super) fn anchored_at(observed_at: &str, offset_ns: &str) -> Option<String> {
    use time::format_description::well_known::Rfc3339;
    let observed = OffsetDateTime::parse(observed_at, &Rfc3339).ok()?;
    let offset = offset_ns.parse::<i64>().ok()?;
    (observed + time::Duration::nanoseconds(offset))
        .format(&Rfc3339)
        .ok()
}

fn utc_basic_at(timestamp: &str) -> Result<String> {
    let observed = OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
        .with_context(|| format!("parse Traffic timestamp {timestamp}"))?;
    let format = time::format_description::parse_borrowed::<1>(
        "[year][month][day]T[hour][minute][second].[subsecond digits:3]Z",
    )
    .expect("static UTC filename format is valid");
    observed
        .format(&format)
        .context("format Traffic filename timestamp")
}

fn rename_noreplace(source: &Path, target: &Path) -> Result<()> {
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    {
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            source,
            rustix::fs::CWD,
            target,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .with_context(|| format!("rename {} to {}", source.display(), target.display()))
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )))]
    {
        let _ = (source, target);
        bail!("atomic no-clobber Traffic Record rename is unsupported on this platform")
    }
}

fn sanitize_host(host: &str) -> String {
    let mut slug = String::with_capacity(host.len().min(48));
    for character in host.chars() {
        if slug.len() >= 48 {
            break;
        }
        let character = character.to_ascii_lowercase();
        if character.is_ascii_alphanumeric() || character == '.' || character == '-' {
            slug.push(character);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches(['.', '-']);
    if slug.is_empty() {
        "invalid".to_string()
    } else {
        slug.to_string()
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub(super) fn offset_ns(origin: Instant) -> String {
    origin.elapsed().as_nanos().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traffic_interpretation::ProtocolDiagnostic;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn host_slug_and_flat_record_layout_are_safe() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, request) = store
            .begin(
                "POST",
                "/https://example.com/v1",
                Some("https://example.com/v1"),
                "HTTP/1.1",
                Vec::new(),
                Some("example.com"),
            )
            .unwrap();
        assert_eq!(record.directory.parent(), Some(store.root()));
        assert_eq!(request.format_version, FORMAT_VERSION);
        assert!(
            record
                .directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("active-")
        );
        assert!(
            record
                .directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("example.com")
        );
        assert_eq!(
            fs::metadata(store.root()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(record.directory.join(REQUEST_BODY))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(record.directory.join(SUMMARY_JSON).exists());
        assert!(!record.directory.join(RESULT_JSON).exists());
    }

    #[test]
    fn summary_is_terminal_and_legacy_result_is_derived() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin("GET", "/bad", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        store
            .finish(
                &record,
                Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Rejected,
                None,
            )
            .unwrap();
        let found = store.find(&record.id).unwrap();
        assert!(found.summary.terminal);
        let result = found.result.unwrap();
        assert_eq!(result.outcome, Outcome::Rejected);
        assert!(!result.ended_at.is_empty());
        let terminal_name = found.directory.file_name().unwrap().to_string_lossy();
        assert!(!terminal_name.starts_with("active-"));
        assert_eq!(terminal_name, found.sort_key);
        assert_eq!(record.locator.path(), found.directory);
    }

    #[test]
    fn every_terminal_outcome_has_an_end_time_and_terminal_directory() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        for outcome in [
            Outcome::Completed,
            Outcome::Rejected,
            Outcome::UpstreamError,
            Outcome::ClientDisconnected,
            Outcome::RecordingFailed,
            Outcome::ServerShutdown,
        ] {
            let (record, _) = store
                .begin(
                    "GET",
                    "/outcome",
                    None,
                    "HTTP/1.1",
                    Vec::new(),
                    Some("example.test"),
                )
                .unwrap();
            let result = store
                .finish(
                    &record,
                    Instant::now(),
                    &RuntimeMeasurements::default(),
                    outcome,
                    None,
                )
                .unwrap();
            let stored = store.find(&record.id).unwrap();
            assert_eq!(stored.result.as_ref().unwrap().ended_at, result.ended_at);
            assert_eq!(stored.result.as_ref().unwrap().outcome, outcome);
            assert!(
                !stored
                    .directory
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("active-")
            );
        }
    }

    #[test]
    fn terminal_summary_is_immutable_to_late_checkpoints() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin("GET", "/late", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        store
            .finish(
                &record,
                Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Completed,
                None,
            )
            .unwrap();
        let before = serde_json::to_value(store.find(&record.id).unwrap().summary).unwrap();

        let changed = store
            .update_summary(&record.locator, &record.summary, |summary| {
                summary.timing.upstream_request_body_completed_at_ns = Some("999".to_string());
                true
            })
            .unwrap();

        assert!(!changed);
        assert_eq!(
            serde_json::to_value(store.find(&record.id).unwrap().summary).unwrap(),
            before
        );
    }

    #[test]
    fn safe_unprefixed_nonterminal_directory_remains_readable_without_migration() {
        let temp = tempfile::tempdir().unwrap();
        let first = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = first
            .begin(
                "GET",
                "/legacy",
                None,
                "HTTP/1.1",
                Vec::new(),
                Some("legacy.test"),
            )
            .unwrap();
        let active_name = record.directory.file_name().unwrap().to_string_lossy();
        let legacy_name = active_name.strip_prefix("active-").unwrap();
        let legacy_path = first.root().join(legacy_name);
        fs::rename(&record.directory, &legacy_path).unwrap();
        drop(first);

        let reopened = TrafficStore::open(temp.path()).unwrap();
        let stored = reopened.find(&record.id).unwrap();
        assert!(!stored.active);
        assert!(stored.result.is_none());
        assert_eq!(stored.directory, legacy_path);
        assert!(stored.sort_key.starts_with("active-"));
    }

    #[test]
    fn terminal_summary_under_active_name_stays_terminal_and_uses_expected_sort_key() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin(
                "GET",
                "/stranded",
                None,
                "HTTP/1.1",
                Vec::new(),
                Some("example.test"),
            )
            .unwrap();
        let active_path = record.directory.clone();
        store
            .finish(
                &record,
                Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::UpstreamError,
                None,
            )
            .unwrap();
        let terminal_path = record.locator.path();
        fs::rename(&terminal_path, &active_path).unwrap();

        let reopened = TrafficStore::open(temp.path()).unwrap();
        let stored = reopened.find(&record.id).unwrap();
        assert_eq!(stored.result.unwrap().outcome, Outcome::UpstreamError);
        assert!(
            stored
                .directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("active-")
        );
        assert!(!stored.sort_key.starts_with("active-"));
    }

    #[test]
    fn no_clobber_rename_failure_preserves_terminal_outcome_and_source_directory() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin(
                "GET",
                "/collision",
                None,
                "HTTP/1.1",
                Vec::new(),
                Some("example.test"),
            )
            .unwrap();
        let active_path = record.directory.clone();
        let first = store
            .finish(
                &record,
                Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::ServerShutdown,
                None,
            )
            .unwrap();
        let target = record.locator.path();
        fs::rename(&target, &active_path).unwrap();
        record.locator.set_path(active_path.clone());
        fs::create_dir(&target).unwrap();

        let repeated = store
            .finish(
                &record,
                Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Completed,
                None,
            )
            .unwrap();

        assert_eq!(repeated.outcome, Outcome::ServerShutdown);
        assert_eq!(repeated.ended_at, first.ended_at);
        assert!(active_path.exists());
        assert!(target.exists());
        let listed = store.scan().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].request.id, record.id);
        assert!(!listed[0].active);
        assert!(store.find(&record.id).is_err());
    }

    #[test]
    fn normal_directory_order_matches_scanned_sort_keys_exactly() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (first_active, _) = store
            .begin(
                "GET",
                "/first",
                None,
                "HTTP/1.1",
                Vec::new(),
                Some("z.test"),
            )
            .unwrap();
        let (terminal, _) = store
            .begin(
                "GET",
                "/terminal",
                None,
                "HTTP/1.1",
                Vec::new(),
                Some("a.test"),
            )
            .unwrap();
        store
            .finish(
                &terminal,
                Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Completed,
                None,
            )
            .unwrap();
        let (last_active, _) = store
            .begin("GET", "/last", None, "HTTP/1.1", Vec::new(), Some("a.test"))
            .unwrap();

        let scan = store.scan().unwrap();
        let scanned: Vec<_> = scan.iter().map(|record| record.sort_key.clone()).collect();
        let mut names: Vec<_> = fs::read_dir(store.root())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort_by(|left, right| right.cmp(left));

        assert_eq!(scanned, names);
        assert_eq!(scan[0].request.id, last_active.id);
        assert_eq!(scan[1].request.id, first_active.id);
        assert_eq!(scan[2].request.id, terminal.id);
    }

    #[test]
    fn derived_result_uses_the_finished_monotonic_offset() {
        let mut summary = SummaryMetadata::test("018f4c8e-4b6b-7c13-8a22-2e4d6d6b6e12", None);
        summary.terminal = true;
        summary.timing.finished_at_ns = Some("1500000000".to_string());
        summary.outcome = Some(Outcome::Completed);
        refresh_assessment(&mut summary);

        let result = summary_to_result(&summary);
        assert!(result.ended_at.starts_with("2026-08-06T04:00:01"));
    }

    #[test]
    fn assessment_preserves_evidence_and_prioritizes_recording_provider_transport_http_then_warning()
     {
        let mut summary = SummaryMetadata::test(
            "018f4c8e-4b6b-7c13-8a22-2e4d6d6b6e12",
            Some(ProtocolSummary::for_url(Some(
                "https://api.example.test/v1/responses",
            ))),
        );
        summary.terminal = true;
        summary.outcome = Some(Outcome::UpstreamError);
        summary.timing.finished_at_ns = Some("90".to_string());
        summary.response = Some(SummaryResponseMetadata {
            status: 401,
            http_version: "HTTP/2".to_string(),
        });
        summary.errors.extend([
            DiagnosticMetadata {
                phase: "response".to_string(),
                kind: "response_recording_failed".to_string(),
                message: "response bytes could not be recorded".to_string(),
                at_ns: "90".to_string(),
            },
            DiagnosticMetadata {
                phase: "response".to_string(),
                kind: "response_recording_failed".to_string(),
                message: "response bytes could not be recorded".to_string(),
                at_ns: "70".to_string(),
            },
        ]);
        summary
            .protocol
            .as_mut()
            .unwrap()
            .errors
            .push(ProtocolDiagnostic {
                kind: "service_unavailable_error".to_string(),
                message: "provider overloaded".to_string(),
                at_ns: Some("10".to_string()),
            });
        summary.warnings.push(DiagnosticMetadata {
            phase: "recording".to_string(),
            kind: "event_index_failed".to_string(),
            message: "timing index unavailable".to_string(),
            at_ns: "20".to_string(),
        });

        refresh_assessment(&mut summary);
        assert_eq!(summary.assessment.level, AssessmentLevel::Error);
        assert_eq!(summary.assessment.issue_count, 4);
        let primary = summary.assessment.primary.as_ref().unwrap();
        assert_eq!(primary.source, AssessmentSource::Traffic);
        assert_eq!(primary.kind, "response_recording_failed");
        assert_eq!(
            diagnostic_findings(&summary, false)
                .iter()
                .find(|finding| finding.kind == "response_recording_failed")
                .unwrap()
                .at_ns
                .as_deref(),
            Some("70")
        );

        summary
            .errors
            .retain(|error| error.kind != "response_recording_failed");
        refresh_assessment(&mut summary);
        assert_eq!(
            summary.assessment.primary.as_ref().unwrap().source,
            AssessmentSource::Provider
        );

        summary.protocol.as_mut().unwrap().errors.clear();
        summary.errors.push(DiagnosticMetadata {
            phase: "response".to_string(),
            kind: "upstream_response_failed".to_string(),
            message: "connection reset".to_string(),
            at_ns: "30".to_string(),
        });
        refresh_assessment(&mut summary);
        assert_eq!(
            summary.assessment.primary.as_ref().unwrap().source,
            AssessmentSource::Traffic
        );

        summary.errors.clear();
        summary.outcome = Some(Outcome::Completed);
        refresh_assessment(&mut summary);
        assert_eq!(
            summary.assessment.primary.as_ref().unwrap().source,
            AssessmentSource::Http
        );

        summary.response = None;
        refresh_assessment(&mut summary);
        assert_eq!(
            summary.assessment.primary.as_ref().unwrap().source,
            AssessmentSource::Diagnostic
        );
        assert_eq!(summary.assessment.level, AssessmentLevel::Warning);
    }

    #[test]
    fn client_disconnect_and_request_abort_are_warnings_but_recording_failure_is_error() {
        for (kind, outcome, level) in [
            (
                "client_disconnected",
                Outcome::ClientDisconnected,
                AssessmentLevel::Warning,
            ),
            (
                "request_body_failed",
                Outcome::ClientDisconnected,
                AssessmentLevel::Warning,
            ),
            (
                "request_recording_failed",
                Outcome::RecordingFailed,
                AssessmentLevel::Error,
            ),
        ] {
            let mut summary = SummaryMetadata::test("018f4c8e-4b6b-7c13-8a22-2e4d6d6b6e12", None);
            summary.terminal = true;
            summary.outcome = Some(outcome);
            summary.timing.finished_at_ns = Some("10".to_string());
            summary.errors.push(DiagnosticMetadata {
                phase: "request".to_string(),
                kind: kind.to_string(),
                message: "request stream ended".to_string(),
                at_ns: "10".to_string(),
            });
            refresh_assessment(&mut summary);
            assert_eq!(summary.assessment.level, level, "{kind}");
        }
    }

    #[test]
    fn summary_scan_ignores_body_and_metadata_corruption_but_detail_is_strict() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let outside = temp.path().join("outside");
        fs::write(&outside, b"outside").unwrap();
        let mut ids = Vec::new();
        for corruption in ["request_metadata", "response_metadata", "request_body"] {
            let (mut record, _) = store
                .begin("GET", "/corrupt", None, "HTTP/1.1", Vec::new(), None)
                .unwrap();
            record.request_body.write_all(b"raw request").unwrap();
            record.response_body.write_all(b"raw response").unwrap();
            store
                .write_response(
                    &record.locator,
                    &record.summary,
                    &ResponseMetadata {
                        format_version: FORMAT_VERSION,
                        source: ResponseSource::Upstream,
                        headers_at: utc_now(),
                        status: 200,
                        http_version: "HTTP/1.1".to_string(),
                        headers: Vec::new(),
                    },
                )
                .unwrap();
            store
                .finish(
                    &record,
                    Instant::now(),
                    &RuntimeMeasurements::default(),
                    Outcome::Completed,
                    None,
                )
                .unwrap();
            let directory = record.locator.path();
            match corruption {
                "request_metadata" => {
                    fs::write(directory.join(REQUEST_JSON), b"not json").unwrap();
                }
                "response_metadata" => {
                    fs::remove_file(directory.join(RESPONSE_JSON)).unwrap();
                    std::os::unix::fs::symlink(&outside, directory.join(RESPONSE_JSON)).unwrap();
                }
                "request_body" => {
                    fs::remove_file(directory.join(REQUEST_BODY)).unwrap();
                    std::os::unix::fs::symlink(&outside, directory.join(REQUEST_BODY)).unwrap();
                }
                _ => unreachable!(),
            }
            ids.push(record.id);
        }

        assert_eq!(store.scan_summaries().unwrap().len(), ids.len());
        for id in ids {
            assert!(store.find(&id).is_err());
        }
    }

    #[test]
    fn recorded_headers_drop_connection_named_fields() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("connection", "x-hop, keep-alive".parse().unwrap());
        headers.insert("x-hop", "secret".parse().unwrap());
        headers.insert("x-app", "kept".parse().unwrap());

        let recorded = RecordedHeader::from_headers(&headers);

        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].name, "x-app");
    }

    #[test]
    fn persisted_metadata_uses_the_stable_schema_names() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin(
                "POST",
                "/https://example.com/v1/responses",
                Some("https://example.com/v1/responses"),
                "HTTP/2",
                vec![RecordedHeader {
                    name: "Session-Id".to_string(),
                    value_base64: base64::engine::general_purpose::STANDARD
                        .encode("opaque-session"),
                }],
                Some("example.com"),
            )
            .unwrap();
        store
            .write_response(
                &record.locator,
                &record.summary,
                &ResponseMetadata {
                    format_version: FORMAT_VERSION,
                    source: ResponseSource::Upstream,
                    headers_at: utc_now(),
                    status: 200,
                    http_version: "HTTP/2".to_string(),
                    headers: vec![],
                },
            )
            .unwrap();
        let request: serde_json::Value =
            serde_json::from_reader(fs::File::open(record.directory.join(REQUEST_JSON)).unwrap())
                .unwrap();
        let response: serde_json::Value =
            serde_json::from_reader(fs::File::open(record.directory.join(RESPONSE_JSON)).unwrap())
                .unwrap();
        let summary: serde_json::Value =
            serde_json::from_reader(fs::File::open(record.directory.join(SUMMARY_JSON)).unwrap())
                .unwrap();
        assert_eq!(request["schema_version"], FORMAT_VERSION);
        assert_eq!(request["record_id"], record.id);
        assert_eq!(request["kind"], "request");
        assert!(request.get("format_version").is_none());
        assert_eq!(response["kind"], "response");
        assert!(response.get("source").is_none());
        assert_eq!(summary["kind"], "summary");
        assert_eq!(summary["request"]["method"], "POST");
        assert_eq!(
            summary["request"]["incoming_uri"],
            "/https://example.com/v1/responses"
        );
        assert_eq!(summary["request"]["http_version"], "HTTP/2");
        assert_eq!(summary["response"]["status"], 200);
        assert_eq!(summary["assessment"]["level"], "active");
        assert_eq!(summary["coding_agent_session_id"], "opaque-session");
        assert_eq!(summary["protocol"]["family"], "openai_responses");
        assert_eq!(summary["protocol"]["response_terminal"], false);
        assert!(summary["protocol"]["model"]["requested"].is_null());
        assert!(record.directory.join(RESPONSE_BODY).is_file());
        assert!(!record.directory.join(RESULT_JSON).exists());
    }

    #[test]
    fn version_one_summaries_are_unsupported() {
        let error = validate_schema(1, "summary", "summary").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("unsupported Traffic schema version 1")
        );
    }

    #[test]
    fn protocol_checkpoints_survive_restart_without_lazy_backfill() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin(
                "POST",
                "/https://example.com/v1/responses",
                Some("https://example.com/v1/responses"),
                "HTTP/2",
                vec![],
                Some("example.com"),
            )
            .unwrap();
        store
            .update_summary(&record.locator, &record.summary, |summary| {
                summary.protocol.as_mut().unwrap().model.requested =
                    Some("gpt-requested".to_string());
                true
            })
            .unwrap();

        let restarted = TrafficStore::open(temp.path()).unwrap();
        let found = restarted.find(&record.id).unwrap();
        assert_eq!(
            found
                .summary
                .protocol
                .as_ref()
                .unwrap()
                .model
                .requested
                .as_deref(),
            Some("gpt-requested")
        );

        let summary_path = record.directory.join(SUMMARY_JSON);
        let mut legacy: serde_json::Value =
            serde_json::from_reader(fs::File::open(&summary_path).unwrap()).unwrap();
        legacy.as_object_mut().unwrap().remove("protocol");
        fs::write(&summary_path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        let before_read = fs::read(&summary_path).unwrap();

        let legacy_store = TrafficStore::open(temp.path()).unwrap();
        assert!(
            legacy_store
                .find(&record.id)
                .unwrap()
                .summary
                .protocol
                .is_none()
        );
        assert_eq!(fs::read(summary_path).unwrap(), before_read);
    }

    #[test]
    fn concurrent_summary_updates_publish_a_single_monotonic_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin(
                "POST",
                "/https://example.com/v1/responses",
                Some("https://example.com/v1/responses"),
                "HTTP/2",
                vec![],
                Some("example.com"),
            )
            .unwrap();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let locator = record.locator.clone();
        let summary = record.summary.clone();
        let first_store = store.clone();
        let first_barrier = barrier.clone();
        let first = std::thread::spawn(move || {
            first_barrier.wait();
            first_store
                .update_summary(&locator, &summary, |value| {
                    value.timing.upstream_request_body_completed_at_ns = Some("10".to_string());
                    true
                })
                .unwrap();
        });
        let locator = record.locator.clone();
        let summary = record.summary.clone();
        let second_store = store.clone();
        let second_barrier = barrier.clone();
        let second = std::thread::spawn(move || {
            second_barrier.wait();
            second_store
                .update_summary(&locator, &summary, |value| {
                    value.protocol.as_mut().unwrap().model.effective =
                        Some("gpt-effective".to_string());
                    true
                })
                .unwrap();
        });
        barrier.wait();
        first.join().unwrap();
        second.join().unwrap();

        let persisted = TrafficStore::open(temp.path())
            .unwrap()
            .find(&record.id)
            .unwrap()
            .summary;
        assert_eq!(
            persisted
                .timing
                .upstream_request_body_completed_at_ns
                .as_deref(),
            Some("10")
        );
        assert_eq!(
            persisted.protocol.unwrap().model.effective.as_deref(),
            Some("gpt-effective")
        );
    }

    #[test]
    fn missing_terminal_metadata_is_interrupted_unless_currently_active() {
        let temp = tempfile::tempdir().unwrap();
        let first = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = first
            .begin("GET", "/bad", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        assert!(first.find(&record.id).unwrap().active);
        let restarted = TrafficStore::open(temp.path()).unwrap();
        assert!(!restarted.find(&record.id).unwrap().active);
        assert!(restarted.find(&record.id).unwrap().result.is_none());
    }

    #[test]
    fn collection_ignores_unknown_and_misnamed_direct_children() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        fs::write(store.root().join("unknown-file"), b"leave me alone").unwrap();
        fs::create_dir(store.root().join("unknown-directory")).unwrap();
        let (record, _) = store
            .begin("GET", "/bad", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let id = record.id.clone();
        let renamed = store.root().join(format!("wrong-name-{id}"));
        fs::rename(&record.directory, &renamed).unwrap();
        assert!(store.scan().unwrap().is_empty());
        assert!(store.find(&id).is_err());
        store.abandon_active(&id);
        assert!(store.delete_ids(&[id]).is_err());
        assert!(renamed.exists());
    }

    #[test]
    fn explicit_lookup_rejects_duplicate_record_directories() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin("GET", "/duplicate", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let original_name = record.directory.file_name().unwrap().to_str().unwrap();
        let duplicate = store.root().join(original_name.replace(
            &format!("-invalid-{}", record.id),
            &format!("-duplicate-{}", record.id),
        ));
        fs::create_dir(&duplicate).unwrap();
        for entry in fs::read_dir(&record.directory).unwrap() {
            let entry = entry.unwrap();
            fs::copy(entry.path(), duplicate.join(entry.file_name())).unwrap();
        }

        let error = store.find(&record.id).unwrap_err().to_string();
        assert!(
            error.contains("multiple Traffic Record directories"),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn opening_and_scanning_never_follow_symlinked_traffic_paths() {
        use std::os::unix::fs::symlink;

        let linked_root = tempfile::tempdir().unwrap();
        let outside_collection = tempfile::tempdir().unwrap();
        fs::write(outside_collection.path().join("keep"), b"outside").unwrap();
        symlink(
            outside_collection.path(),
            linked_root.path().join("traffic"),
        )
        .unwrap();
        let error = TrafficStore::open(linked_root.path())
            .err()
            .expect("a symlinked collection must be rejected")
            .to_string();
        assert!(error.contains("not a real directory"), "{error}");
        assert_eq!(
            fs::read(outside_collection.path().join("keep")).unwrap(),
            b"outside"
        );

        let root = tempfile::tempdir().unwrap();
        let outside_body = tempfile::tempdir().unwrap();
        let target = outside_body.path().join("request.body");
        fs::write(&target, b"secret").unwrap();
        let store = TrafficStore::open(root.path()).unwrap();
        let (record, _) = store
            .begin("POST", "/unsafe", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let body = record.directory.join(REQUEST_BODY);
        fs::remove_file(&body).unwrap();
        symlink(&target, &body).unwrap();

        assert!(store.scan().unwrap().is_empty());
        assert_eq!(fs::read(target).unwrap(), b"secret");
    }

    #[cfg(unix)]
    #[test]
    fn deletion_rejects_symlinked_record_entries_without_touching_targets() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("secret");
        fs::write(&target, b"keep").unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin("GET", "/bad", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        store
            .finish(
                &record,
                Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Rejected,
                None,
            )
            .unwrap();
        let terminal_directory = record.locator.path();
        symlink(&target, terminal_directory.join("unsafe-link")).unwrap();
        assert!(store.delete_ids(std::slice::from_ref(&record.id)).is_err());
        assert_eq!(fs::read(target).unwrap(), b"keep");
        assert!(terminal_directory.exists());
    }

    #[test]
    fn delete_all_rechecks_the_non_active_count_and_preserves_active_records() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        for _ in 0..2 {
            let (record, _) = store
                .begin("GET", "/bad", None, "HTTP/1.1", Vec::new(), None)
                .unwrap();
            store
                .finish(
                    &record,
                    Instant::now(),
                    &RuntimeMeasurements::default(),
                    Outcome::Rejected,
                    None,
                )
                .unwrap();
        }
        let (active, _) = store
            .begin("GET", "/active", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        assert!(store.delete_all(1).is_err());
        assert_eq!(store.scan().unwrap().len(), 3);
        assert_eq!(store.delete_all(2).unwrap(), 2);
        let remaining = store.scan().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].request.id, active.id);
        assert!(remaining[0].active);
    }

    #[test]
    fn delete_ids_requires_a_unique_valid_non_active_selection_before_removing_anything() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let make_finished = |uri| {
            let (record, _) = store
                .begin("GET", uri, None, "HTTP/1.1", Vec::new(), None)
                .unwrap();
            let id = record.id.clone();
            store
                .finish(
                    &record,
                    Instant::now(),
                    &RuntimeMeasurements::default(),
                    Outcome::Rejected,
                    None,
                )
                .unwrap();
            id
        };
        let first = make_finished("/first");
        let second = make_finished("/second");
        let (active, _) = store
            .begin("GET", "/active", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let missing = Uuid::now_v7().to_string();

        for (ids, expected) in [
            (Vec::new(), "at least one"),
            (vec![first.clone(), first.clone()], "must not be repeated"),
            (
                vec![
                    first.clone(),
                    "550e8400-e29b-41d4-a716-446655440000".to_string(),
                ],
                "not UUID v7",
            ),
            (vec![first.clone(), missing], "not found"),
            (vec![first.clone(), active.id.clone()], "active Traffic"),
        ] {
            let error = store.delete_ids(&ids).unwrap_err().to_string();
            assert!(error.contains(expected), "{ids:?}: {error}");
            assert!(
                store.find(&first).is_ok(),
                "{ids:?} removed the first record"
            );
            assert!(
                store.find(&second).is_ok(),
                "{ids:?} removed the second record"
            );
        }

        assert_eq!(store.delete_ids(&[first, second]).unwrap(), 2);
        let remaining = store.scan().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].request.id, active.id);
        assert!(remaining[0].active);
    }
}
