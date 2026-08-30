//! Strict Request detail reads, tolerant catalogs, event timings, and deletion.

use super::layout::{
    RequestFile, ResponseFile, canonical_sort_key, optional_json, parse_request_directory_name,
    read_json, regular_file_length, remove_controlled_request_dir, validate_id,
    validate_regular_file, validate_request_ancestor,
};
use super::{
    DiagnosticMetadata, ErrorKind, ErrorMetadata, FORMAT_VERSION, Outcome, REQUEST_BODY,
    REQUEST_JSON, RESPONSE_BODY, RESPONSE_EVENTS_JSONL, RESPONSE_JSON, RESULT_JSON,
    RequestDetailReadError, RequestMetadata, RequestStore, ResponseMetadata, ResponseSource,
    ResultMetadata, SUMMARY_JSON, StoredEventTiming, StoredEventTimings, StoredRequest,
    StoredRequestSummary, SummaryMetadata, anchored_at, offset_ns,
};
use crate::foundation::sync::{lock_unpoisoned, read_unpoisoned, write_unpoisoned};
use crate::request::assessment::calculate_assessment;
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Instant;
use time::OffsetDateTime;

impl RequestStore {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn scan(&self) -> Result<Vec<StoredRequest>> {
        let _namespace = read_unpoisoned(&self.namespace);
        self.scan_unlocked()
    }

    pub fn scan_summaries(&self) -> Result<Vec<StoredRequestSummary>> {
        let _namespace = read_unpoisoned(&self.namespace);
        self.scan_summaries_unlocked()
    }

    fn request_directories(&self) -> Result<Vec<PathBuf>> {
        if !crate::foundation::safe_fs::real_dir_exists(&self.root, "Request collection")? {
            return Ok(Vec::new());
        }
        let mut directories = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("read Request collection {}", self.root.display()))?
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    self.warning("request collection entry could not be inspected", None);
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    self.warning("request entry could not be inspected", None);
                    continue;
                }
            };
            if !metadata.file_type().is_dir() {
                self.warning("unexpected request entry ignored", None);
                continue;
            }
            directories.push(path);
        }
        Ok(directories)
    }

    fn scan_summaries_unlocked(&self) -> Result<Vec<StoredRequestSummary>> {
        let directories = self.request_directories()?;
        let active = lock_unpoisoned(&self.active).clone();
        let mut requests = Vec::new();
        for path in directories {
            match read_request_summary(&path, &active) {
                Ok(request) => requests.push(request),
                Err(_) => self.warning("incomplete or invalid request ignored", None),
            }
        }
        requests.sort_by(|left, right| right.sort_key.cmp(&left.sort_key));
        Ok(requests)
    }

    fn scan_unlocked(&self) -> Result<Vec<StoredRequest>> {
        let directories = self.request_directories()?;
        let active = lock_unpoisoned(&self.active).clone();
        let mut requests = Vec::new();
        for path in directories {
            match read_request(&path, &active) {
                Ok(request) => requests.push(request),
                Err(_) => self.warning("incomplete or invalid request ignored", None),
            }
        }
        requests.sort_by(|left, right| right.sort_key.cmp(&left.sort_key));
        Ok(requests)
    }

    // Explicit request operations only inspect directory names carrying the
    // requested UUID. A malformed matching entry is an error, while unrelated
    // collection entries retain the tolerant listing behavior above.
    fn find_unlocked(&self, id: &str) -> Result<StoredRequest> {
        let mut ids = HashSet::new();
        ids.insert(id);
        self.find_many_unlocked(&ids)?.remove(id).ok_or_else(|| {
            crate::application_error::application_error(
                crate::application_error::ApplicationErrorKind::NotFound,
                format!("Request not found: {id}"),
            )
        })
    }

    fn find_many_unlocked(&self, ids: &HashSet<&str>) -> Result<HashMap<String, StoredRequest>> {
        if !crate::foundation::safe_fs::real_dir_exists(&self.root, "Request collection")? {
            return Ok(HashMap::new());
        }
        let active = lock_unpoisoned(&self.active).clone();
        let mut requests = HashMap::with_capacity(ids.len());
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("read Request collection {}", self.root.display()))?
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
                .with_context(|| format!("inspect selected Request {}", path.display()))?;
            if !metadata.file_type().is_dir() {
                bail!(
                    "selected Request is not a real directory: {}",
                    path.display()
                );
            }
            let request = read_request(&path, &active)
                .with_context(|| format!("read selected Request {}", path.display()))?;
            if request.request.id != candidate {
                bail!("selected Request metadata id does not match its directory name");
            }
            if requests.insert(candidate.to_string(), request).is_some() {
                bail!("multiple Request directories match id {candidate}");
            }
        }
        Ok(requests)
    }

    pub fn find(&self, id: &str) -> Result<StoredRequest> {
        let _namespace = read_unpoisoned(&self.namespace);
        validate_id(id)?;
        self.find_unlocked(id)
    }

    pub fn open_body(&self, id: &str, response: bool, offset: u64) -> Result<(fs::File, u64)> {
        let _namespace = read_unpoisoned(&self.namespace);
        validate_id(id)?;
        let request = self.find_unlocked(id)?;
        self.open_request_body_unlocked(&request, response, offset)
    }

    pub fn open_request_body(
        &self,
        request: &StoredRequest,
        response: bool,
        offset: u64,
    ) -> Result<(fs::File, u64)> {
        let _namespace = read_unpoisoned(&self.namespace);
        let current = self.find_unlocked(&request.request.id)?;
        self.open_request_body_unlocked(&current, response, offset)
    }

    fn open_request_body_unlocked(
        &self,
        request: &StoredRequest,
        response: bool,
        offset: u64,
    ) -> Result<(fs::File, u64)> {
        validate_request_ancestor(&self.root, &request.directory)?;
        let path = request.directory.join(if response {
            RESPONSE_BODY
        } else {
            REQUEST_BODY
        });
        validate_regular_file(&path, "Request body")?;
        let mut file = crate::foundation::safe_fs::open_real_file(&path, "Request body")?;
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
        let request = self.find_unlocked(id)?;
        validate_request_ancestor(&self.root, &request.directory)?;
        let path = request.directory.join(RESPONSE_EVENTS_JSONL);
        if !crate::foundation::safe_fs::real_file_exists(&path, "Request SSE event index")? {
            return Ok(StoredEventTimings {
                available: false,
                partial: false,
                events: Vec::new(),
                next_sequence: after_sequence,
                warning: Some("SSE Event timing index is unavailable".to_string()),
            });
        }

        let file = crate::foundation::safe_fs::open_real_file(&path, "Request SSE event index")?;
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
            } else if request.active {
                break;
            }
            if line.is_empty() {
                continue;
            }
            match serde_json::from_slice::<EventIndexLine>(&line) {
                Ok(entry) if event_index_entry_valid(&entry, &request.request.id) => {
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
    ) -> std::result::Result<StoredRequest, RequestDetailReadError> {
        let _namespace = read_unpoisoned(&self.namespace);
        validate_id(id).map_err(RequestDetailReadError::Lookup)?;
        let mut request = self
            .find_unlocked(id)
            .map_err(RequestDetailReadError::Lookup)?;
        append_event_index_warnings(&request.directory, &mut request.summary, request.active)
            .map_err(RequestDetailReadError::EventIndex)?;
        Ok(request)
    }

    pub fn delete_ids(&self, ids: &[String]) -> Result<usize> {
        let _namespace = write_unpoisoned(&self.namespace);
        if ids.is_empty() {
            bail!("at least one Request id is required");
        }
        let unique: HashSet<_> = ids.iter().collect();
        if unique.len() != ids.len() {
            bail!("Request IDs must not be repeated");
        }
        for id in ids {
            validate_id(id)?;
        }
        let active = lock_unpoisoned(&self.active).clone();
        if ids.iter().any(|id| active.contains_key(id)) {
            return Err(crate::application_error::application_error(
                crate::application_error::ApplicationErrorKind::Conflict,
                "active Requests cannot be deleted",
            ));
        }
        let requested: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let requests = self.find_many_unlocked(&requested)?;
        let mut selected = Vec::new();
        for id in ids {
            let request = requests.get(id).ok_or_else(|| {
                crate::application_error::application_error(
                    crate::application_error::ApplicationErrorKind::NotFound,
                    format!("Request not found: {id}"),
                )
            })?;
            if request.active {
                return Err(crate::application_error::application_error(
                    crate::application_error::ApplicationErrorKind::Conflict,
                    "active Requests cannot be deleted",
                ));
            }
            validate_request_ancestor(&self.root, &request.directory)?;
            selected.push(request.directory.clone());
        }
        for path in &selected {
            remove_controlled_request_dir(path)?;
        }
        crate::foundation::safe_fs::sync_dir(&self.root)?;
        Ok(selected.len())
    }
}

pub(super) fn read_request_summary(
    path: &Path,
    active: &HashMap<String, Instant>,
) -> Result<StoredRequestSummary> {
    let summary: SummaryMetadata = read_json(&path.join(SUMMARY_JSON), "Request summary metadata")?;
    validate_schema(summary.schema_version, &summary.kind, "summary")?;
    validate_id(&summary.request_id)?;
    let directory = parse_request_directory_name(path, &summary.request_id)?;
    validate_summary(&summary)?;
    let live_elapsed_ns = active_elapsed_ns(summary.terminal, active, &summary.request_id);
    Ok(StoredRequestSummary {
        sort_key: canonical_sort_key(&summary, &directory.host, &summary.request_id)?,
        summary,
        active: live_elapsed_ns.is_some(),
        live_elapsed_ns,
    })
}

fn read_request(path: &Path, active: &HashMap<String, Instant>) -> Result<StoredRequest> {
    let request_file: RequestFile =
        read_json(&path.join(REQUEST_JSON), "Incoming HTTP Request metadata")?;
    validate_schema(request_file.schema_version, &request_file.kind, "request")?;
    validate_id(&request_file.request_id)?;
    let directory = parse_request_directory_name(path, &request_file.request_id)?;
    let summary: SummaryMetadata = read_json(&path.join(SUMMARY_JSON), "Request summary metadata")?;
    validate_schema(summary.schema_version, &summary.kind, "summary")?;
    if summary.request_id != request_file.request_id {
        bail!("Request metadata ids do not match");
    }
    validate_summary(&summary)?;
    if summary.request.method != request_file.method
        || summary.request.upstream_url != request_file.upstream_url
    {
        bail!("Incoming HTTP Request metadata does not match its Summary projection");
    }
    let _ = crate::foundation::safe_fs::real_file_exists(
        &path.join(RESPONSE_EVENTS_JSONL),
        "Request SSE event index",
    )?;
    if crate::foundation::safe_fs::real_file_exists(
        path.join(RESULT_JSON).as_path(),
        "legacy result metadata",
    )? {
        bail!("legacy result.json is unsupported");
    }
    let response_file: Option<ResponseFile> =
        optional_json(&path.join(RESPONSE_JSON), "Upstream Response metadata")?;
    if let Some(response) = &response_file {
        validate_schema(response.schema_version, &response.kind, "response")?;
        if response.request_id != request_file.request_id {
            bail!("Upstream Response metadata id does not match");
        }
    }
    match (&summary.response, &response_file) {
        (None, None) => {}
        (Some(projected), Some(response))
            if projected.status == response.status
                && projected.http_version == response.http_version => {}
        _ => bail!("Upstream Response metadata does not match its Summary projection"),
    }
    let request_body_bytes =
        regular_file_length(&path.join(REQUEST_BODY), "Incoming HTTP Request body")?;
    let response_body_bytes =
        regular_file_length(&path.join(RESPONSE_BODY), "Upstream Response body")?;
    let request = RequestMetadata {
        format_version: FORMAT_VERSION,
        id: request_file.request_id.clone(),
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
    Ok(StoredRequest {
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

pub(super) fn active_elapsed_ns(
    terminal: bool,
    active: &HashMap<String, Instant>,
    request_id: &str,
) -> Option<String> {
    if terminal {
        None
    } else {
        active.get(request_id).copied().map(offset_ns)
    }
}

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

pub(super) fn terminal_summary_matches(
    path: &Path,
    id: &str,
    outcome: Outcome,
    finished_at_ns: &str,
) -> bool {
    read_json::<SummaryMetadata>(&path.join(SUMMARY_JSON), "Request summary metadata").is_ok_and(
        |summary| {
            summary.request_id == id
                && summary.terminal
                && summary.outcome == Some(outcome)
                && summary.timing.finished_at_ns.as_deref() == Some(finished_at_ns)
        },
    )
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

pub(super) fn parse_error_kind(kind: &str) -> ErrorKind {
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

#[derive(Deserialize)]
struct EventIndexLine {
    schema_version: u32,
    request_id: String,
    kind: String,
    sequence: u64,
    body_start: u64,
    body_end: u64,
    first_arrival_at_ns: String,
    completed_at_ns: String,
}

fn event_index_entry_valid(entry: &EventIndexLine, request_id: &str) -> bool {
    entry.schema_version == FORMAT_VERSION
        && entry.request_id == request_id
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
    if !crate::foundation::safe_fs::real_file_exists(&index_path, "Request SSE event index")? {
        return Ok(());
    }
    let file = crate::foundation::safe_fs::open_real_file(&index_path, "Request SSE event index")?;
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
            Ok(entry) if event_index_entry_valid(&entry, &summary.request_id) => {
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
