//! The on-disk Traffic Record layout and its lifecycle.
//!
//! [`TrafficStore`] owns the flat `$AIBOX_ROOT/traffic/<record>/` collection.
//! Each Record directory holds raw evidence — `request.json`, `request.body`,
//! `response.json`, `response.body`, and the optional `response.events.jsonl`
//! index — plus `summary.json`, the complete list projection.
//!
//! ## Summary is the lifecycle authority
//!
//! `summary.json` exists from [`TrafficStore::begin`] onward and is atomically
//! checkpointed at observable milestones, so an interrupted attempt still has
//! meaningful state and timing. A directory name is only a materialized ordering
//! hint: a Record starts under an `active-` name and [`TrafficStore::finish`]
//! renames it after the terminal Summary is committed. Readers accept both sides
//! of that non-transactional boundary and never infer state from the name. See
//! `docs/adr/0010-materialize-traffic-record-end-order.md`.
//!
//! ## Reads do not trust the filesystem
//!
//! The collection sits in the owner's aibox root, so every read confirms that an
//! entry is a real file or directory and that a Record is still a direct child of
//! the collection. Listing tolerates per-Record corruption while detail reads
//! stay strict, and a [`FORMAT_VERSION`] mismatch is unsupported rather than
//! migrated.

use crate::sync::{lock_unpoisoned, read_unpoisoned, write_unpoisoned};
use crate::tenant;
#[cfg(test)]
use crate::traffic_assessment::diagnostic_findings;
use crate::traffic_assessment::{calculate_assessment, refresh_assessment};
use crate::traffic_interpretation::{ProtocolSummary, coding_agent_session_id};
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

pub(crate) const FORMAT_VERSION: u32 = 2;
const REQUEST_JSON: &str = "request.json";
const REQUEST_BODY: &str = "request.body";
const RESPONSE_JSON: &str = "response.json";
const RESPONSE_BODY: &str = "response.body";
const RESPONSE_EVENTS_JSONL: &str = "response.events.jsonl";
const SUMMARY_JSON: &str = "summary.json";
const RESULT_JSON: &str = "result.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RecordedHeader {
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
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseSource {
    Upstream,
    Proxy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ResponseMetadata {
    pub format_version: u32,
    pub source: ResponseSource,
    pub headers_at: String,
    pub status: u16,
    pub http_version: String,
    pub headers: Vec<RecordedHeader>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
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
pub(crate) struct ErrorMetadata {
    pub kind: ErrorKind,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
pub(crate) struct DiagnosticMetadata {
    pub phase: String,
    pub kind: String,
    pub message: String,
    pub at_ns: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SummaryRequestMetadata {
    pub method: String,
    pub incoming_uri: String,
    pub upstream_url: Option<String>,
    pub http_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SummaryResponseMetadata {
    pub status: u16,
    pub http_version: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AssessmentLevel {
    Active,
    Ok,
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AssessmentSource {
    Traffic,
    Http,
    Provider,
    Diagnostic,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct AssessmentPrimary {
    pub source: AssessmentSource,
    pub kind: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RecordAssessment {
    pub level: AssessmentLevel,
    pub primary: Option<AssessmentPrimary>,
    pub issue_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct AssessmentFinding {
    pub level: AssessmentLevel,
    pub source: AssessmentSource,
    pub kind: String,
    pub message: String,
    pub phase: Option<String>,
    pub at_ns: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct SummaryMetadata {
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
    pub(crate) fn test(record_id: impl Into<String>, protocol: Option<ProtocolSummary>) -> Self {
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
pub(crate) struct SummaryHandle {
    inner: Arc<Mutex<SummaryMetadata>>,
}

impl SummaryHandle {
    pub(crate) fn new(summary: SummaryMetadata) -> Self {
        Self {
            inner: Arc::new(Mutex::new(summary)),
        }
    }

    pub(crate) fn update<R>(&self, update: impl FnOnce(&mut SummaryMetadata) -> R) -> R {
        let mut summary = lock_unpoisoned(&self.inner);
        update(&mut summary)
    }

    pub(crate) fn read<R>(&self, read: impl FnOnce(&SummaryMetadata) -> R) -> R {
        let summary = lock_unpoisoned(&self.inner);
        read(&summary)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Default)]
pub(crate) struct RuntimeMeasurements {
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub request_body_duration: Option<Duration>,
}

#[derive(Clone)]
pub(crate) struct TrafficStore {
    root: PathBuf,
    active: Arc<Mutex<HashMap<String, Instant>>>,
    namespace: Arc<RwLock<()>>,
}

#[derive(Clone, Debug)]
pub(crate) struct RecordLocator {
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

    pub(crate) fn path(&self) -> PathBuf {
        lock_unpoisoned(&self.inner).clone()
    }

    fn set_path(&self, path: PathBuf) {
        *lock_unpoisoned(&self.inner) = path;
    }
}

/// The application-visible request that opens one Traffic Record.
///
/// [`TrafficStore::begin`] takes this as a whole because its fields are mostly
/// interchangeable strings: `method`, `incoming_uri`, and `http_version` share a
/// type, as do `upstream_url` and `host_hint`, so positional arguments would
/// accept a wrong order.
pub(crate) struct ObservedRequest<'a> {
    pub method: &'a str,
    pub incoming_uri: &'a str,
    /// Absent when the incoming URI could not be resolved to an upstream target.
    pub upstream_url: Option<&'a str>,
    pub http_version: &'a str,
    pub headers: Vec<RecordedHeader>,
    /// Names the Record directory; a missing hint records `invalid`.
    pub host_hint: Option<&'a str>,
}

#[cfg(test)]
impl<'a> ObservedRequest<'a> {
    /// A plain HTTP/1.1 request with no upstream target, headers, or host hint.
    /// Combine with struct-update syntax so a test names only what it varies.
    pub fn test(method: &'a str, incoming_uri: &'a str) -> Self {
        Self {
            method,
            incoming_uri,
            upstream_url: None,
            http_version: "HTTP/1.1",
            headers: Vec::new(),
            host_hint: None,
        }
    }
}

pub(crate) struct NewRecord {
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
pub(crate) struct StoredRecord {
    pub directory: PathBuf,
    pub sort_key: String,
    pub request: RequestMetadata,
    pub response: Option<ResponseMetadata>,
    pub summary: SummaryMetadata,
    pub result: Option<ResultMetadata>,
    pub request_body_bytes: u64,
    pub response_body_bytes: u64,
    pub active: bool,
    pub live_elapsed_ns: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct StoredRecordSummary {
    pub sort_key: String,
    pub summary: SummaryMetadata,
    pub active: bool,
    pub live_elapsed_ns: Option<String>,
}

impl RecordAssessment {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredEventTiming {
    pub sequence: u64,
    pub completed_at_ns: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoredEventTimings {
    pub available: bool,
    pub partial: bool,
    pub events: Vec<StoredEventTiming>,
    pub next_sequence: u64,
    pub warning: Option<String>,
}

pub(crate) enum RecordDetailReadError {
    Lookup(anyhow::Error),
    EventIndex(anyhow::Error),
}

impl TrafficStore {
    pub fn open(aibox_root: &Path) -> Result<Self> {
        tenant::ensure_real_dir(aibox_root, "aibox root")?;
        let root = aibox_root.join("traffic");
        tenant::ensure_real_dir(&root, "Traffic Record collection")?;
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

    pub fn begin(&self, observed: ObservedRequest<'_>) -> Result<(NewRecord, RequestMetadata)> {
        let ObservedRequest {
            method,
            incoming_uri,
            upstream_url,
            http_version,
            headers,
            host_hint,
        } = observed;
        let _namespace = write_unpoisoned(&self.namespace);
        tenant::real_dir_exists(&self.root, "Traffic Record collection")?;
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
        lock_unpoisoned(&self.active).insert(id.clone(), origin);

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
                http_version: http_version.to_string(),
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
            tenant::sync_dir(&directory)?;
            tenant::sync_dir(&self.root)?;
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
                lock_unpoisoned(&self.active).remove(&id);
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
        let _namespace = read_unpoisoned(&self.namespace);
        let directory = locator.path();
        validate_record_ancestor(&self.root, &directory)?;
        let mut summary = lock_unpoisoned(&handle.inner);
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
        let _namespace = write_unpoisoned(&self.namespace);
        let directory = locator.path();
        validate_record_ancestor(&self.root, &directory)?;
        let mut summary = lock_unpoisoned(&handle.inner);
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
        let _namespace = read_unpoisoned(&self.namespace);
        let directory = record.locator.path();
        validate_record_ancestor(&self.root, &directory)?;
        create_private_file(&directory.join(RESPONSE_EVENTS_JSONL))
    }

    pub fn with_record_path<R>(
        &self,
        locator: &RecordLocator,
        operation: impl FnOnce(&Path) -> R,
    ) -> Result<R> {
        let _namespace = read_unpoisoned(&self.namespace);
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
        let _namespace = write_unpoisoned(&self.namespace);
        let directory = record.locator.path();
        validate_record_ancestor(&self.root, &directory)?;
        let at_ns = offset_ns(record.origin);
        let mut summary = lock_unpoisoned(&record.summary.inner);
        if summary.terminal {
            let snapshot = summary.clone();
            drop(summary);
            let ended_at = summary_ended_at(&snapshot);
            self.finalize_directory_unlocked(record, &ended_at);
            lock_unpoisoned(&self.active).remove(&record.id);
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
        lock_unpoisoned(&self.active).remove(&record.id);
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
                if let Err(error) = tenant::sync_dir(&self.root) {
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
        lock_unpoisoned(&self.active).remove(id);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn scan(&self) -> Result<Vec<StoredRecord>> {
        let _namespace = read_unpoisoned(&self.namespace);
        self.scan_unlocked()
    }

    pub fn scan_summaries(&self) -> Result<Vec<StoredRecordSummary>> {
        let _namespace = read_unpoisoned(&self.namespace);
        self.scan_summaries_unlocked()
    }

    fn record_directories(&self) -> Result<Vec<PathBuf>> {
        if !tenant::real_dir_exists(&self.root, "Traffic Record collection")? {
            return Ok(Vec::new());
        }
        let mut directories = Vec::new();
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
            directories.push(path);
        }
        Ok(directories)
    }

    fn scan_summaries_unlocked(&self) -> Result<Vec<StoredRecordSummary>> {
        let directories = self.record_directories()?;
        let active = lock_unpoisoned(&self.active).clone();
        let mut records = Vec::new();
        for path in directories {
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
        let directories = self.record_directories()?;
        let active = lock_unpoisoned(&self.active).clone();
        let mut records = Vec::new();
        for path in directories {
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
        if !tenant::real_dir_exists(&self.root, "Traffic Record collection")? {
            return Ok(HashMap::new());
        }
        let active = lock_unpoisoned(&self.active).clone();
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
        let _namespace = read_unpoisoned(&self.namespace);
        validate_id(id)?;
        self.find_unlocked(id)
    }

    pub fn open_body(&self, id: &str, response: bool, offset: u64) -> Result<(fs::File, u64)> {
        let _namespace = read_unpoisoned(&self.namespace);
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
        let _namespace = read_unpoisoned(&self.namespace);
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
        let mut file = tenant::open_real_file(&path, "Traffic body")?;
        let length = file.metadata()?.len();
        if offset > length {
            bail!("body offset {offset} exceeds current length {length}");
        }
        file.seek(SeekFrom::Start(offset))?;
        Ok((file, length))
    }

    pub fn read_event_timings(&self, id: &str, after_sequence: u64) -> Result<StoredEventTimings> {
        let _namespace = read_unpoisoned(&self.namespace);
        validate_id(id)?;
        let record = self.find_unlocked(id)?;
        validate_record_ancestor(&self.root, &record.directory)?;
        let path = record.directory.join(RESPONSE_EVENTS_JSONL);
        if !tenant::real_file_exists(&path, "Traffic SSE event index")? {
            return Ok(StoredEventTimings {
                available: false,
                partial: false,
                events: Vec::new(),
                next_sequence: after_sequence,
                warning: Some("SSE Event timing index is unavailable".to_string()),
            });
        }

        let file = tenant::open_real_file(&path, "Traffic SSE event index")?;
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
        let _namespace = read_unpoisoned(&self.namespace);
        validate_id(id).map_err(RecordDetailReadError::Lookup)?;
        let mut record = self
            .find_unlocked(id)
            .map_err(RecordDetailReadError::Lookup)?;
        append_event_index_warnings(&record.directory, &mut record.summary, record.active)
            .map_err(RecordDetailReadError::EventIndex)?;
        Ok(record)
    }

    pub fn delete_ids(&self, ids: &[String]) -> Result<usize> {
        let _namespace = write_unpoisoned(&self.namespace);
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
        let active = lock_unpoisoned(&self.active).clone();
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
        tenant::sync_dir(&self.root)?;
        Ok(selected.len())
    }

    pub fn delete_all(&self) -> Result<usize> {
        let _namespace = write_unpoisoned(&self.namespace);
        let summaries = self.scan_summaries_unlocked()?;
        let ids: HashSet<_> = summaries
            .into_iter()
            .filter(|record| !record.active)
            .map(|record| record.summary.record_id)
            .collect();
        let requested: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let records = self.find_many_unlocked(&requested)?;
        let mut selected = Vec::with_capacity(ids.len());
        for id in &ids {
            let record = records
                .get(id)
                .with_context(|| format!("Traffic Record not found: {id}"))?;
            if record.active {
                continue;
            }
            validate_record_ancestor(&self.root, &record.directory)?;
            validate_controlled_record_dir(&record.directory)?;
            selected.push(record.directory.clone());
        }
        for path in &selected {
            remove_controlled_record_dir(path)?;
        }
        tenant::sync_dir(&self.root)?;
        Ok(selected.len())
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
    let live_elapsed_ns = active_elapsed_ns(summary.terminal, active, &summary.record_id);
    Ok(StoredRecordSummary {
        sort_key: canonical_sort_key(&summary, &directory.host, &summary.record_id)?,
        summary,
        active: live_elapsed_ns.is_some(),
        live_elapsed_ns,
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
    let _ = tenant::real_file_exists(&path.join(RESPONSE_EVENTS_JSONL), "Traffic SSE event index")?;
    if tenant::real_file_exists(path.join(RESULT_JSON).as_path(), "legacy result metadata")? {
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
    let live_elapsed_ns = active_elapsed_ns(summary.terminal, active, &request.id);
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
        active: live_elapsed_ns.is_some(),
        live_elapsed_ns,
    })
}

fn active_elapsed_ns(
    terminal: bool,
    active: &HashMap<String, Instant>,
    record_id: &str,
) -> Option<String> {
    if terminal {
        None
    } else {
        active.get(record_id).copied().map(offset_ns)
    }
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
    if !tenant::real_file_exists(&index_path, "Traffic SSE event index")? {
        return Ok(());
    }
    let file = tenant::open_real_file(&index_path, "Traffic SSE event index")?;
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
                    &format!("cannot read SSE event index line: {error}"),
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
        let warning = event_index_warning(summary, line_number, &warning);
        summary.warnings.push(warning);
    }
    Ok(())
}

fn event_index_warning(
    summary: &SummaryMetadata,
    line_number: usize,
    message: &str,
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
    let file = tenant::open_real_file(path, kind)?;
    serde_json::from_reader(file).with_context(|| format!("parse {kind} {}", path.display()))
}

fn optional_json<T: serde::de::DeserializeOwned>(path: &Path, kind: &str) -> Result<Option<T>> {
    if !tenant::real_file_exists(path, kind)? {
        return Ok(None);
    }
    read_json(path, kind).map(Some)
}

fn regular_file_length(path: &Path, kind: &str) -> Result<u64> {
    validate_regular_file(path, kind)?;
    Ok(fs::symlink_metadata(path)?.len())
}

fn validate_regular_file(path: &Path, kind: &str) -> Result<()> {
    if !tenant::real_file_exists(path, kind)? {
        bail!("{kind} does not exist: {}", path.display());
    }
    Ok(())
}

fn validate_record_ancestor(root: &Path, directory: &Path) -> Result<()> {
    if directory.parent() != Some(root) {
        bail!("Traffic Record is not a direct child of the Traffic collection");
    }
    if !tenant::real_dir_exists(root, "Traffic Record collection")?
        || !tenant::real_dir_exists(directory, "Traffic Record")?
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
        tenant::sync_dir(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn remove_controlled_record_dir(path: &Path) -> Result<()> {
    let files = validate_controlled_record_dir(path)?;
    for file in files {
        fs::remove_file(&file)
            .with_context(|| format!("delete Traffic file {}", file.display()))?;
    }
    fs::remove_dir(path).with_context(|| format!("delete Traffic Record {}", path.display()))
}

fn validate_controlled_record_dir(path: &Path) -> Result<Vec<PathBuf>> {
    if !tenant::real_dir_exists(path, "Traffic Record")? {
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
    Ok(files)
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

pub(crate) fn utc_now() -> String {
    let format = time::format_description::parse_borrowed::<1>(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:9]Z",
    )
    .expect("static Traffic timestamp format is valid");
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

pub(crate) fn offset_ns(origin: Instant) -> String {
    origin.elapsed().as_nanos().to_string()
}

#[cfg(test)]
#[path = "traffic_store_tests.rs"]
mod tests;
