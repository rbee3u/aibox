//! The on-disk Request layout and its lifecycle.
//!
//! The facade exposes one concrete Store while writing, inspection/deletion,
//! and safe layout mechanics remain separately owned.

mod layout;
mod reading;
mod writing;

use crate::foundation::sync::lock_unpoisoned;
#[cfg(test)]
use crate::foundation::sync::read_unpoisoned;
#[cfg(test)]
use crate::request::assessment::{diagnostic_findings, refresh_assessment};
pub(crate) use crate::request::model::{
    DiagnosticMetadata, ErrorKind, ErrorMetadata, Outcome, ProtocolSummary, RecordedHeader,
    RequestAssessment, RequestMetadata, ResponseMetadata, ResponseSource, ResultMetadata,
    SummaryMetadata, SummaryRequestMetadata, SummaryResponseMetadata, TerminalRequestEvent,
    TimingMetadata, anchored_at, utc_now,
};
use std::collections::HashMap;
use std::fs;
#[cfg(test)]
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
#[cfg(test)]
use uuid::Uuid;

pub(crate) use layout::offset_ns;
#[cfg(test)]
use layout::safe_display_host;
#[cfg(test)]
use reading::{summary_to_result, validate_schema};

pub(crate) const FORMAT_VERSION: u32 = 4;
const REQUEST_JSON: &str = "request.json";
const REQUEST_BODY: &str = "request.body";
const RESPONSE_JSON: &str = "response.json";
const RESPONSE_BODY: &str = "response.body";
const RESPONSE_EVENTS_JSONL: &str = "response.events.jsonl";
const SUMMARY_JSON: &str = "summary.json";
const RESULT_JSON: &str = "result.json";

#[cfg(test)]
impl SummaryMetadata {
    pub(crate) fn test(request_id: impl Into<String>, protocol: Option<ProtocolSummary>) -> Self {
        Self {
            schema_version: FORMAT_VERSION,
            request_id: request_id.into(),
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
            assessment: RequestAssessment::active(0),
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

    #[cfg(test)]
    pub(crate) fn update<R>(&self, update: impl FnOnce(&mut SummaryMetadata) -> R) -> R {
        let mut summary = lock_unpoisoned(&self.inner);
        update(&mut summary)
    }

    pub(crate) fn read<R>(&self, read: impl FnOnce(&SummaryMetadata) -> R) -> R {
        let summary = lock_unpoisoned(&self.inner);
        read(&summary)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RuntimeMeasurements {
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub request_body_duration: Option<Duration>,
}

/// Process-local handle for the flat Request collection.
///
/// Clones share the active-attempt registry and namespace lock. The registry is
/// deliberately not reconstructed from `active-` directory names: after a
/// process restart, a nonterminal Request is interrupted rather than active.
#[derive(Clone)]
pub(crate) struct RequestStore {
    root: PathBuf,
    active: Arc<Mutex<HashMap<String, Instant>>>,
    namespace: Arc<RwLock<()>>,
    warning_sink: Option<RequestWarningSink>,
}

pub(crate) type RequestWarningSink = Arc<dyn Fn(&str, Option<&str>) + Send + Sync>;

pub(crate) struct FinishedRequest {
    pub(crate) result: ResultMetadata,
    pub(crate) terminal_event: Option<TerminalRequestEvent>,
}

impl std::ops::Deref for FinishedRequest {
    type Target = ResultMetadata;

    fn deref(&self) -> &Self::Target {
        &self.result
    }
}

/// Shared lookup for a Request directory whose name changes at terminalization.
///
/// Long-lived writers must resolve the path through this handle instead of
/// retaining the creation path from [`NewRequest::directory`].
#[derive(Clone, Debug)]
pub(crate) struct RequestLocator {
    inner: Arc<Mutex<PathBuf>>,
    host: Arc<str>,
    display_host: Arc<str>,
}

impl RequestLocator {
    fn new(path: PathBuf, host: String, display_host: String) -> Self {
        Self {
            inner: Arc::new(Mutex::new(path)),
            host: host.into(),
            display_host: display_host.into(),
        }
    }

    pub(crate) fn path(&self) -> PathBuf {
        lock_unpoisoned(&self.inner).clone()
    }

    fn set_path(&self, path: PathBuf) {
        *lock_unpoisoned(&self.inner) = path;
    }
}

/// The application-visible request that opens one Request.
///
/// [`RequestStore::begin`] takes this as a whole because its fields are mostly
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
    /// Supplies the safe Console host and Request directory slug; a missing hint
    /// requests `invalid`.
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

/// Writable state returned after a Request has been durably opened.
pub(crate) struct NewRequest {
    pub id: String,
    /// Initial `active-` path, retained for tests and diagnostics only.
    ///
    /// Runtime path operations must use [`Self::locator`] because
    /// terminalization may rename the directory.
    #[allow(dead_code)]
    pub directory: PathBuf,
    pub locator: RequestLocator,
    pub request_body: fs::File,
    pub response_body: fs::File,
    pub summary: SummaryHandle,
    pub origin: Instant,
}

#[derive(Clone, Debug)]
pub(crate) struct StoredRequest {
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

pub(crate) fn timeline_end_at_ns(request: &StoredRequest, live: Option<String>) -> Option<String> {
    if request.active {
        return live;
    }
    if let Some(finished) = &request.summary.timing.finished_at_ns {
        return Some(finished.clone());
    }
    let protocol_offsets = request
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
        request
            .summary
            .timing
            .upstream_request_started_at_ns
            .as_ref(),
        request
            .summary
            .timing
            .upstream_request_body_first_byte_at_ns
            .as_ref(),
        request
            .summary
            .timing
            .upstream_request_body_completed_at_ns
            .as_ref(),
        request
            .summary
            .timing
            .upstream_response_headers_at_ns
            .as_ref(),
        request
            .summary
            .timing
            .upstream_response_body_first_byte_at_ns
            .as_ref(),
        request
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

#[derive(Clone, Debug)]
pub(crate) struct StoredRequestSummary {
    pub sort_key: String,
    pub summary: SummaryMetadata,
    pub active: bool,
    pub live_elapsed_ns: Option<String>,
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

/// Distinguishes a strict Request lookup failure from optional index degradation.
///
/// The Requests module API reports lookup failures as a missing detail, while an
/// unsafe event-index structure is a server error rather than silently omitting
/// its diagnostics.
pub(crate) enum RequestDetailReadError {
    Lookup(anyhow::Error),
    EventIndex(anyhow::Error),
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
