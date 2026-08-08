use crate::traffic_interpretation::ProtocolSummary;
use anyhow::{bail, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use uuid::Uuid;

pub(super) const FORMAT_VERSION: u32 = 1;
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct SummaryMetadata {
    pub schema_version: u32,
    pub record_id: String,
    pub kind: String,
    pub observed_at: String,
    pub terminal: bool,
    pub timing: TimingMetadata,
    #[serde(default)]
    pub protocol: Option<ProtocolSummary>,
    pub outcome: Option<Outcome>,
    pub errors: Vec<DiagnosticMetadata>,
    pub warnings: Vec<DiagnosticMetadata>,
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
}

pub(super) struct NewRecord {
    pub id: String,
    pub directory: PathBuf,
    pub request_body: fs::File,
    pub response_body: fs::File,
    pub summary: SummaryHandle,
    pub origin: Instant,
}

#[derive(Clone, Debug)]
pub(super) struct StoredRecord {
    pub directory: PathBuf,
    pub request: RequestMetadata,
    pub response: Option<ResponseMetadata>,
    pub summary: SummaryMetadata,
    pub result: Option<ResultMetadata>,
    pub request_body_bytes: u64,
    pub response_body_bytes: u64,
    pub active: bool,
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
        crate::tenant::real_dir_exists(&self.root, "Traffic Record collection")?;
        let id = Uuid::now_v7().to_string();
        let observed_at = utc_now();
        let origin = Instant::now();
        let directory_name = format!(
            "{}-{}-{id}",
            utc_basic_now(),
            sanitize_host(host_hint.unwrap_or("invalid"))
        );
        let directory = self.root.join(directory_name);
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
                terminal: false,
                timing: TimingMetadata::default(),
                protocol: Some(ProtocolSummary::for_url(upstream_url)),
                outcome: None,
                errors: Vec::new(),
                warnings: Vec::new(),
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
        directory: &Path,
        handle: &SummaryHandle,
        update: impl FnOnce(&mut SummaryMetadata) -> bool,
    ) -> Result<bool> {
        validate_record_ancestor(&self.root, directory)?;
        let mut summary = handle
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = update(&mut summary);
        if changed {
            atomic_write_json(directory, SUMMARY_JSON, &*summary)?;
        }
        Ok(changed)
    }

    pub fn write_response(&self, directory: &Path, metadata: &ResponseMetadata) -> Result<()> {
        validate_record_ancestor(&self.root, directory)?;
        let record_id = read_summary_id(directory)?;
        let file = ResponseFile {
            schema_version: FORMAT_VERSION,
            record_id,
            kind: "response".to_string(),
            http_version: metadata.http_version.clone(),
            status: metadata.status,
            headers: metadata.headers.clone(),
        };
        atomic_write_json(directory, RESPONSE_JSON, &file)
    }

    pub fn create_event_index(&self, record: &NewRecord) -> Result<fs::File> {
        validate_record_ancestor(&self.root, &record.directory)?;
        create_private_file(&record.directory.join(RESPONSE_EVENTS_JSONL))
    }

    pub fn finish(
        &self,
        record: &NewRecord,
        started: Instant,
        measurements: &RuntimeMeasurements,
        outcome: Outcome,
        error: Option<ErrorMetadata>,
    ) -> Result<ResultMetadata> {
        let at_ns = offset_ns(record.origin);
        let mut summary = record
            .summary
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        atomic_write_json(&record.directory, SUMMARY_JSON, &*summary)?;
        let snapshot = summary.clone();
        drop(summary);
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
            ended_at: utc_now(),
            request_bytes: measurements.request_bytes,
            response_bytes: measurements.response_bytes,
            request_body_ms: measurements.request_body_duration.map(duration_ms),
            total_ms,
            outcome,
            error,
        })
    }

    pub fn abandon_active(&self, id: &str) {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id);
    }

    pub fn scan(&self) -> Result<Vec<StoredRecord>> {
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
        records.sort_by(|left, right| {
            right
                .summary
                .observed_at
                .cmp(&left.summary.observed_at)
                .then_with(|| right.request.id.cmp(&left.request.id))
        });
        Ok(records)
    }

    pub fn find(&self, id: &str) -> Result<StoredRecord> {
        validate_id(id)?;
        self.scan()?
            .into_iter()
            .find(|record| record.request.id == id)
            .with_context(|| format!("Traffic Record not found: {id}"))
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
        let record = self.find(id)?;
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

    pub fn delete_ids(&self, ids: &[String]) -> Result<usize> {
        if ids.is_empty() {
            bail!("at least one Traffic Record id is required");
        }
        let unique: HashSet<_> = ids.iter().collect();
        if unique.len() != ids.len() {
            bail!("Traffic Record ids must not be repeated");
        }
        let active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if ids.iter().any(|id| active.contains_key(id)) {
            bail!("active Traffic Records cannot be deleted");
        }
        let records = self.scan()?;
        let mut selected = Vec::new();
        for id in ids {
            validate_id(id)?;
            let record = records
                .iter()
                .find(|record| &record.request.id == id)
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
        let records: Vec<_> = self
            .scan()?
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

fn read_record(path: &Path, active: &HashMap<String, Instant>) -> Result<StoredRecord> {
    let request_file: RequestFile =
        read_json(&path.join(REQUEST_JSON), "Traffic request metadata")?;
    validate_schema(request_file.schema_version, &request_file.kind, "request")?;
    validate_id(&request_file.record_id)?;
    validate_record_directory_name(path, &request_file.record_id)?;
    let mut summary: SummaryMetadata =
        read_json(&path.join(SUMMARY_JSON), "Traffic summary metadata")?;
    validate_schema(summary.schema_version, &summary.kind, "summary")?;
    if summary.record_id != request_file.record_id {
        bail!("Traffic metadata record ids do not match");
    }
    validate_summary(&summary)?;
    append_event_index_warnings(path, &mut summary)?;
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
    let request_body_bytes = regular_file_length(&path.join(REQUEST_BODY), "Traffic request body")?;
    let response_body_bytes =
        regular_file_length(&path.join(RESPONSE_BODY), "Traffic response body")?;
    let request = RequestMetadata {
        format_version: FORMAT_VERSION,
        id: request_file.record_id.clone(),
        started_at: summary.observed_at.clone(),
        method: request_file.method,
        incoming_uri: String::new(),
        upstream_url: request_file.upstream_url,
        http_version: String::new(),
        headers: request_file.headers,
    };
    let response = response_file.map(|metadata| ResponseMetadata {
        format_version: FORMAT_VERSION,
        source: ResponseSource::Upstream,
        headers_at: summary.observed_at.clone(),
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
    if summary
        .protocol
        .as_ref()
        .is_some_and(|protocol| protocol.token_usage.is_some() && !protocol.response_terminal)
    {
        bail!("Traffic protocol summary has final Token Usage before a terminal response");
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

fn append_event_index_warnings(path: &Path, summary: &mut SummaryMetadata) -> Result<()> {
    let index_path = path.join(RESPONSE_EVENTS_JSONL);
    if !crate::tenant::real_file_exists(&index_path, "Traffic SSE event index")? {
        return Ok(());
    }
    let file = crate::tenant::open_real_file(&index_path, "Traffic SSE event index")?;
    for (line_number, line) in std::io::BufReader::new(file).split(b'\n').enumerate() {
        let warning = match line {
            Ok(line) if line.is_empty() => continue,
            Ok(line) => match serde_json::from_slice::<EventIndexLine>(&line) {
                Ok(entry)
                    if entry.schema_version == FORMAT_VERSION
                        && entry.record_id == summary.record_id
                        && entry.kind == "sse_event"
                        && entry.body_start <= entry.body_end
                        && entry.first_arrival_at_ns.parse::<u128>().is_ok()
                        && entry.completed_at_ns.parse::<u128>().is_ok() =>
                {
                    let _ = entry.sequence;
                    continue;
                }
                Ok(_) => "SSE event index line has invalid metadata".to_string(),
                Err(error) => format!("cannot parse SSE event index line: {error}"),
            },
            Err(error) => format!("cannot read SSE event index line: {error}"),
        };
        summary.warnings.push(DiagnosticMetadata {
            phase: "recording".to_string(),
            kind: "event_index_failed".to_string(),
            message: format!("line {}: {warning}", line_number + 1),
            at_ns: summary
                .timing
                .finished_at_ns
                .clone()
                .unwrap_or_else(|| "0".to_string()),
        });
    }
    Ok(())
}

fn read_summary_id(path: &Path) -> Result<String> {
    let summary: SummaryMetadata = read_json(&path.join(SUMMARY_JSON), "Traffic summary metadata")?;
    validate_schema(summary.schema_version, &summary.kind, "summary")?;
    Ok(summary.record_id)
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

fn validate_record_directory_name(path: &Path, id: &str) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Traffic Record directory name is not valid UTF-8")?;
    let suffix = format!("-{id}");
    let prefix = name
        .strip_suffix(&suffix)
        .context("Traffic Record directory name does not match its UUID")?;
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
    Ok(())
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
    let format = time::format_description::parse(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:9]Z",
    )
    .expect("static Traffic timestamp format is valid");
    OffsetDateTime::now_utc()
        .format(&format)
        .unwrap_or_else(|_| "1970-01-01T00:00:00.000000000Z".to_string())
}

fn utc_basic_now() -> String {
    let format = time::format_description::parse(
        "[year][month][day]T[hour][minute][second].[subsecond digits:3]Z",
    )
    .expect("static UTC filename format is valid");
    OffsetDateTime::now_utc()
        .format(&format)
        .unwrap_or_else(|_| "19700101T000000.000Z".to_string())
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
        assert!(record
            .directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("example.com"));
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
    }

    #[test]
    fn derived_result_uses_the_finished_monotonic_offset() {
        let summary = SummaryMetadata {
            schema_version: FORMAT_VERSION,
            record_id: "018f4c8e-4b6b-7c13-8a22-2e4d6d6b6e12".to_string(),
            kind: "summary".to_string(),
            observed_at: "2026-08-06T04:00:00Z".to_string(),
            terminal: true,
            timing: TimingMetadata {
                finished_at_ns: Some("1500000000".to_string()),
                ..TimingMetadata::default()
            },
            protocol: None,
            outcome: Some(Outcome::Completed),
            errors: Vec::new(),
            warnings: Vec::new(),
        };

        let result = summary_to_result(&summary);
        assert!(result.ended_at.starts_with("2026-08-06T04:00:01"));
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
                vec![],
                Some("example.com"),
            )
            .unwrap();
        store
            .write_response(
                &record.directory,
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
        assert_eq!(summary["protocol"]["family"], "openai_responses");
        assert_eq!(summary["protocol"]["response_terminal"], false);
        assert!(summary["protocol"]["model"]["requested"].is_null());
        assert!(record.directory.join(RESPONSE_BODY).is_file());
        assert!(!record.directory.join(RESULT_JSON).exists());
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
            .update_summary(&record.directory, &record.summary, |summary| {
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
        assert!(legacy_store
            .find(&record.id)
            .unwrap()
            .summary
            .protocol
            .is_none());
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
        let directory = record.directory.clone();
        let summary = record.summary.clone();
        let first_store = store.clone();
        let first_barrier = barrier.clone();
        let first = std::thread::spawn(move || {
            first_barrier.wait();
            first_store
                .update_summary(&directory, &summary, |value| {
                    value.timing.upstream_request_body_completed_at_ns = Some("10".to_string());
                    true
                })
                .unwrap();
        });
        let directory = record.directory.clone();
        let summary = record.summary.clone();
        let second_store = store.clone();
        let second_barrier = barrier.clone();
        let second = std::thread::spawn(move || {
            second_barrier.wait();
            second_store
                .update_summary(&directory, &summary, |value| {
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
        let renamed = store.root().join(format!("wrong-name-{}", record.id));
        fs::rename(&record.directory, renamed).unwrap();
        assert!(store.scan().unwrap().is_empty());
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
        symlink(&target, record.directory.join("unsafe-link")).unwrap();
        assert!(store.delete_ids(std::slice::from_ref(&record.id)).is_err());
        assert_eq!(fs::read(target).unwrap(), b"keep");
        assert!(record.directory.exists());
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
