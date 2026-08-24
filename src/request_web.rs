//! Embedded Console assets and the Requests module JSON/body API.
//!
//! List handlers read only the materialized Request Summary while detail
//! reads stay strict over raw metadata, following
//! `docs/adr/0009-request-evidence-and-projections.md`. Bodies stream from
//! disk as recorded; the decoded variants only undo a recorded content coding.
//! Nothing here redacts, truncates, or expires a Request.

use crate::request::AppState;
use crate::request_assessment::{diagnostic_findings, effective_assessment};
use crate::request_interpretation::{
    BodyContentCoding, ProtocolSummary, body_content_coding, timeline_end_at_ns,
};
use crate::request_store::{
    AssessmentFinding, AssessmentLevel, AssessmentSource, FORMAT_VERSION, RecordedHeader,
    RequestAssessment, RequestDetailReadError, RequestMetadata, RequestStore, ResponseMetadata,
    ResponseSource, ResultMetadata, StoredRequestSummary, SummaryMetadata, anchored_at,
};
use anyhow::Context as _;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{FromRef, Path, Query, State};
use axum::http::{HeaderValue, Response, StatusCode, header};
use axum::routing::{get, post};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Read as _;
use tokio::io::AsyncReadExt as _;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::io::ReaderStream;

const PAGE_SIZE: usize = 50;
const HTML: &str = include_str!("../assets/console.html");
const CSS: &str = include_str!("../assets/console.css");
const JS: &str = include_str!("../assets/console.js");
const CSP_NONCE_PLACEHOLDER: &str = "__AIBOX_CSP_NONCE__";

pub(crate) fn api_router<S>() -> Router<S>
where
    AppState: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/_aibox/api/requests", get(list_requests))
        .route("/_aibox/api/requests/delete", post(delete_requests))
        .route("/_aibox/api/requests/{id}", get(request_detail))
        .route("/_aibox/api/requests/{id}/request-body", get(request_body))
        .route(
            "/_aibox/api/requests/{id}/response-body",
            get(response_body),
        )
        .route(
            "/_aibox/api/requests/{id}/request-body-decoded",
            get(decoded_request_body),
        )
        .route(
            "/_aibox/api/requests/{id}/response-body-decoded",
            get(decoded_response_body),
        )
        .route(
            "/_aibox/api/requests/{id}/response-event-timings",
            get(response_event_timings),
        )
}

pub(crate) async fn index(csp_nonce: &str) -> Response<Body> {
    content(
        StatusCode::OK,
        "text/html; charset=utf-8",
        HTML.replacen(CSP_NONCE_PLACEHOLDER, csp_nonce, 1),
    )
}

pub(crate) async fn css() -> Response<Body> {
    content(StatusCode::OK, "text/css; charset=utf-8", CSS)
}

pub(crate) async fn js() -> Response<Body> {
    content(StatusCode::OK, "application/javascript; charset=utf-8", JS)
}

fn content(
    status: StatusCode,
    content_type: &'static str,
    value: impl Into<Body>,
) -> Response<Body> {
    let mut response = Response::new(value.into());
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

#[derive(Deserialize)]
pub(crate) struct ListQuery {
    page: Option<u64>,
}

#[derive(Serialize)]
struct RequestSummary {
    id: String,
    started_at: String,
    ended_at: Option<String>,
    method: String,
    incoming_uri: String,
    upstream_url: Option<String>,
    status: Option<u16>,
    http_version: Option<String>,
    outcome: String,
    state: String,
    total_ms: Option<u64>,
    protocol: Option<ProtocolSummary>,
    assessment: RequestAssessment,
}

#[derive(Serialize)]
struct RequestList {
    requests: Vec<RequestSummary>,
    total: usize,
    deletable_count: usize,
    has_next: bool,
}

pub(crate) async fn list_requests(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Response<Body> {
    let store = state.store.clone();
    match tokio::task::spawn_blocking(move || list_requests_inner(&store, query.page)).await {
        Ok(Ok(value)) => json_response(StatusCode::OK, &value),
        Ok(Err(error)) => json_error(StatusCode::BAD_REQUEST, &error.to_string()),
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("scan Requests: {error}"),
        ),
    }
}

fn list_requests_inner(store: &RequestStore, page: Option<u64>) -> anyhow::Result<RequestList> {
    let page = page.unwrap_or(1);
    if page == 0 {
        anyhow::bail!("Request page must be a positive integer");
    }
    let start = usize::try_from(page - 1)
        .ok()
        .and_then(|page| page.checked_mul(PAGE_SIZE))
        .context("Request page is too large")?;
    let requests = store.scan_summaries()?;
    let total = requests.len();
    let deletable_count = requests.iter().filter(|request| !request.active).count();
    let has_next = start
        .checked_add(PAGE_SIZE)
        .is_some_and(|next| next < total);
    let requests = requests
        .iter()
        .skip(start)
        .take(PAGE_SIZE)
        .map(summary)
        .collect();
    Ok(RequestList {
        requests,
        total,
        deletable_count,
        has_next,
    })
}

/// Name the display state of a Request. A terminal Request carries an Outcome,
/// so a non-active Request without one was interrupted before it finished.
fn state_name(active: bool, terminal: bool) -> &'static str {
    if active {
        "active"
    } else if terminal {
        "completed"
    } else {
        "interrupted"
    }
}

fn summary(request: &StoredRequestSummary) -> RequestSummary {
    let value = &request.summary;
    let state = state_name(request.active, value.outcome.is_some());
    let outcome = match value.outcome {
        Some(outcome) if !request.active => outcome.as_str(),
        _ => state,
    };
    let ended_at = value
        .terminal
        .then(|| {
            value
                .timing
                .finished_at_ns
                .as_deref()
                .and_then(|offset| anchored_at(&value.observed_at, offset))
        })
        .flatten();
    RequestSummary {
        id: value.request_id.clone(),
        started_at: value.observed_at.clone(),
        ended_at,
        method: value.request.method.clone(),
        incoming_uri: value.request.incoming_uri.clone(),
        upstream_url: value.request.upstream_url.clone(),
        status: value.response.as_ref().map(|response| response.status),
        http_version: value
            .response
            .as_ref()
            .map(|response| response.http_version.clone()),
        outcome: outcome.to_string(),
        state: state.to_string(),
        total_ms: if request.active {
            request.live_elapsed_ns.as_deref().and_then(elapsed_ns_ms)
        } else {
            value
                .timing
                .finished_at_ns
                .as_deref()
                .and_then(elapsed_ns_ms)
        },
        protocol: value.protocol.clone(),
        assessment: effective_assessment(value, request.active),
    }
}

#[derive(Serialize)]
struct ResponseDetail {
    format_version: u32,
    source: ResponseSource,
    headers_at: String,
    status: u16,
    http_version: String,
    reason_phrase: Option<String>,
    headers: Vec<RecordedHeader>,
}

impl From<ResponseMetadata> for ResponseDetail {
    fn from(metadata: ResponseMetadata) -> Self {
        let reason_phrase = StatusCode::from_u16(metadata.status)
            .ok()
            .and_then(|status| status.canonical_reason())
            .map(str::to_string);
        Self {
            format_version: FORMAT_VERSION,
            source: metadata.source,
            headers_at: metadata.headers_at,
            status: metadata.status,
            http_version: metadata.http_version,
            reason_phrase,
            headers: metadata.headers,
        }
    }
}

#[derive(Serialize)]
struct RequestDetail {
    request: RequestMetadata,
    response: Option<ResponseDetail>,
    result: Option<ResultMetadata>,
    summary: SummaryMetadata,
    assessment: RequestAssessment,
    diagnostics: DiagnosticGroups,
    state: String,
    request_body_bytes: u64,
    response_body_bytes: u64,
    live_total_ms: Option<u64>,
    timeline_end_at_ns: Option<String>,
}

#[derive(Serialize)]
struct DiagnosticGroups {
    request: Vec<AssessmentFinding>,
    http: Vec<AssessmentFinding>,
    provider: Vec<AssessmentFinding>,
    warnings: Vec<AssessmentFinding>,
}

fn diagnostic_groups(summary: &SummaryMetadata, interrupted: bool) -> DiagnosticGroups {
    let mut groups = DiagnosticGroups {
        request: Vec::new(),
        http: Vec::new(),
        provider: Vec::new(),
        warnings: Vec::new(),
    };
    for finding in diagnostic_findings(summary, interrupted) {
        if finding.level == AssessmentLevel::Warning {
            groups.warnings.push(finding);
        } else {
            match finding.source {
                AssessmentSource::Request => groups.request.push(finding),
                AssessmentSource::Http => groups.http.push(finding),
                AssessmentSource::Provider => groups.provider.push(finding),
                AssessmentSource::Diagnostic => groups.warnings.push(finding),
            }
        }
    }
    groups
}

pub(crate) async fn request_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response<Body> {
    let store = state.store.clone();
    let lookup_id = id.clone();
    let lookup =
        tokio::task::spawn_blocking(move || store.find_with_event_index_warnings(&lookup_id)).await;
    match lookup {
        Ok(Ok(request)) => {
            let terminal = request.result.is_some();
            let state = state_name(request.active, terminal);
            let live_total_ms = request.live_elapsed_ns.as_deref().and_then(elapsed_ns_ms);
            let interrupted = !request.active && !terminal;
            let assessment = effective_assessment(&request.summary, request.active);
            let diagnostics = diagnostic_groups(&request.summary, interrupted);
            let timeline_end_at_ns = timeline_end_at_ns(&request, request.live_elapsed_ns.clone());
            let response_headers_at = request
                .summary
                .timing
                .upstream_response_headers_at_ns
                .as_deref()
                .and_then(|offset| anchored_at(&request.summary.observed_at, offset));
            let response = request.response.map(|metadata| {
                let mut detail = ResponseDetail::from(metadata);
                if let Some(headers_at) = &response_headers_at {
                    detail.headers_at = headers_at.clone();
                }
                detail
            });
            json_response(
                StatusCode::OK,
                &RequestDetail {
                    request: request.request,
                    response,
                    result: request.result,
                    summary: request.summary,
                    assessment,
                    diagnostics,
                    state: state.to_string(),
                    request_body_bytes: request.request_body_bytes,
                    response_body_bytes: request.response_body_bytes,
                    live_total_ms,
                    timeline_end_at_ns,
                },
            )
        }
        Ok(Err(RequestDetailReadError::Lookup(error))) => {
            json_error(StatusCode::NOT_FOUND, &error.to_string())
        }
        Ok(Err(RequestDetailReadError::EventIndex(error))) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
        }
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("read Request detail: {error}"),
        ),
    }
}

fn elapsed_ns_ms(elapsed_ns: &str) -> Option<u64> {
    elapsed_ns
        .parse::<u128>()
        .ok()
        .and_then(|value| u64::try_from(value / 1_000_000).ok())
}

#[derive(Deserialize)]
pub(crate) struct BodyQuery {
    #[serde(default)]
    offset: u64,
}

pub(crate) async fn request_body(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<BodyQuery>,
) -> Response<Body> {
    body_response(&state.store, &id, false, query.offset).await
}

pub(crate) async fn response_body(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<BodyQuery>,
) -> Response<Body> {
    body_response(&state.store, &id, true, query.offset).await
}

pub(crate) async fn decoded_request_body(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response<Body> {
    decoded_body_response(&state.store, &id, false).await
}

pub(crate) async fn decoded_response_body(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response<Body> {
    decoded_body_response(&state.store, &id, true).await
}

async fn body_response(
    store: &RequestStore,
    id: &str,
    response: bool,
    offset: u64,
) -> Response<Body> {
    let store = store.clone();
    let id = id.to_string();
    let opened = tokio::task::spawn_blocking(move || store.open_body(&id, response, offset)).await;
    let (file, length) = match opened {
        Ok(Ok(value)) => value,
        Ok(Err(error)) if error.to_string().contains("exceeds current length") => {
            return json_error(StatusCode::RANGE_NOT_SATISFIABLE, &error.to_string());
        }
        Ok(Err(error)) => return json_error(StatusCode::NOT_FOUND, &error.to_string()),
        Err(error) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("open Request body: {error}"),
            );
        }
    };
    let remaining = length - offset;
    let file = tokio::fs::File::from_std(file).take(remaining);
    let stream = ReaderStream::new(file);
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&remaining.to_string()).expect("body length is a valid header"),
    );
    response.headers_mut().insert(
        "x-aibox-request-next-offset",
        HeaderValue::from_str(&length.to_string()).expect("body offset is a valid header"),
    );
    response
}

async fn decoded_body_response(store: &RequestStore, id: &str, response: bool) -> Response<Body> {
    let lookup_store = store.clone();
    let lookup_id = id.to_string();
    let request = match tokio::task::spawn_blocking(move || lookup_store.find(&lookup_id)).await {
        Ok(Ok(request)) => request,
        Ok(Err(error)) => return json_error(StatusCode::NOT_FOUND, &error.to_string()),
        Err(error) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("read Request for body decoding: {error}"),
            );
        }
    };
    let completed = if response {
        request
            .summary
            .timing
            .upstream_response_body_completed_at_ns
            .is_some()
    } else {
        request
            .summary
            .timing
            .upstream_request_body_completed_at_ns
            .is_some()
    };
    if request.active && !completed {
        return json_error(
            StatusCode::CONFLICT,
            if response {
                "the response body is still being recorded"
            } else {
                "the request body is still being recorded"
            },
        );
    }
    let headers = if response {
        request
            .response
            .as_ref()
            .map(|metadata| metadata.headers.as_slice())
            .unwrap_or_default()
    } else {
        &request.request.headers
    };
    let coding = match body_content_coding(headers) {
        Ok(coding) => coding,
        Err(error) => return json_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, &error.to_string()),
    };
    let body_store = store.clone();
    let opened =
        tokio::task::spawn_blocking(move || body_store.open_request_body(&request, response, 0))
            .await;
    let (file, length) = match opened {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => return json_error(StatusCode::NOT_FOUND, &error.to_string()),
        Err(error) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("open Request body for decoding: {error}"),
            );
        }
    };
    let (body, length) = match coding {
        BodyContentCoding::Identity => {
            let file = tokio::fs::File::from_std(file).take(length);
            (Body::from_stream(ReaderStream::new(file)), Some(length))
        }
        BodyContentCoding::Zstd => (zstd_body(file), None),
    };
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    if let Some(length) = length {
        response.headers_mut().insert(
            header::CONTENT_LENGTH,
            HeaderValue::from_str(&length.to_string()).expect("body length is a valid header"),
        );
    }
    response
}

fn zstd_body(file: std::fs::File) -> Body {
    const CHUNK_SIZE: usize = 64 * 1024;
    const CHANNEL_CAPACITY: usize = 4;
    let (sender, receiver) = tokio::sync::mpsc::channel(CHANNEL_CAPACITY);
    tokio::task::spawn_blocking(move || {
        let mut decoder = match zstd::stream::read::Decoder::new(file) {
            Ok(decoder) => decoder,
            Err(error) => {
                let _ = sender.blocking_send(Err(error));
                return;
            }
        };
        let mut buffer = vec![0u8; CHUNK_SIZE];
        loop {
            match decoder.read(&mut buffer) {
                Ok(0) => return,
                Ok(read) => {
                    if sender
                        .blocking_send(Ok(Bytes::copy_from_slice(&buffer[..read])))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) => {
                    let _ = sender.blocking_send(Err(error));
                    return;
                }
            }
        }
    });
    Body::from_stream(ReceiverStream::new(receiver))
}

#[derive(Deserialize)]
pub(crate) struct EventTimingQuery {
    #[serde(default)]
    after_sequence: u64,
}

#[derive(Serialize)]
struct EventTimingEntry {
    sequence: u64,
    completed_at_ns: String,
}

#[derive(Serialize)]
struct EventTimingResponse {
    state: &'static str,
    events: Vec<EventTimingEntry>,
    next_sequence: u64,
    warning: Option<String>,
}

pub(crate) async fn response_event_timings(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<EventTimingQuery>,
) -> Response<Body> {
    let store = state.store.clone();
    let timings =
        tokio::task::spawn_blocking(move || store.read_event_timings(&id, query.after_sequence))
            .await;
    match timings {
        Ok(Ok(timings)) => json_response(
            StatusCode::OK,
            &EventTimingResponse {
                state: if !timings.available {
                    "unavailable"
                } else if timings.partial {
                    "partial"
                } else {
                    "available"
                },
                events: timings
                    .events
                    .into_iter()
                    .map(|entry| EventTimingEntry {
                        sequence: entry.sequence,
                        completed_at_ns: entry.completed_at_ns,
                    })
                    .collect(),
                next_sequence: timings.next_sequence,
                warning: timings.warning,
            },
        ),
        Ok(Err(error)) => json_error(StatusCode::NOT_FOUND, &error.to_string()),
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("read Request SSE event timings: {error}"),
        ),
    }
}

#[derive(Deserialize)]
pub(crate) struct DeleteRequest {
    ids: Vec<String>,
}

pub(crate) async fn delete_requests(
    State(state): State<AppState>,
    Json(request): Json<DeleteRequest>,
) -> Response<Body> {
    let store = state.store.clone();
    let deleted = tokio::task::spawn_blocking(move || store.delete_ids(&request.ids)).await;
    match deleted {
        Ok(Ok(deleted)) => json_response(StatusCode::OK, &json!({"deleted": deleted})),
        Ok(Err(error)) => {
            let status = if error.to_string().contains("active Request") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            json_error(status, &error.to_string())
        }
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("delete Requests: {error}"),
        ),
    }
}

fn json_response<T: Serialize>(status: StatusCode, value: &T) -> Response<Body> {
    match serde_json::to_vec(value) {
        Ok(bytes) => content(status, "application/json; charset=utf-8", bytes),
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("serialize Request API response: {error}"),
        ),
    }
}

fn json_error(status: StatusCode, message: &str) -> Response<Body> {
    let body = serde_json::to_vec(&json!({"error": message}))
        .unwrap_or_else(|_| b"{\"error\":\"Request API error\"}".to_vec());
    content(status, "application/json; charset=utf-8", body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_interpretation::ProtocolDiagnostic;
    use crate::request_store::{ObservedRequest, Outcome, RuntimeMeasurements};
    use base64::Engine as _;
    use http_body_util::BodyExt as _;
    use std::io::Write as _;
    use uuid::Uuid;

    fn finished_request(
        store: &RequestStore,
        incoming_uri: &str,
        request_body: &[u8],
        response_body: &[u8],
    ) -> String {
        let (mut request, _) = store
            .begin(ObservedRequest::test("POST", incoming_uri))
            .unwrap();
        request.request_body.write_all(request_body).unwrap();
        request.request_body.flush().unwrap();
        request.response_body.write_all(response_body).unwrap();
        request.response_body.flush().unwrap();
        let id = request.id.clone();
        store
            .finish(
                &request,
                std::time::Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Rejected,
                None,
            )
            .unwrap();
        id
    }

    fn recorded_header(name: &str, value: &str) -> RecordedHeader {
        RecordedHeader {
            name: name.to_string(),
            value_base64: base64::engine::general_purpose::STANDARD.encode(value),
        }
    }

    async fn response_json(response: Response<Body>) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn request_summaries_distinguish_active_interrupted_and_completed_state() {
        let temp = tempfile::tempdir().unwrap();
        let first_process = RequestStore::open(temp.path()).unwrap();
        let (interrupted, _) = first_process
            .begin(ObservedRequest::test("GET", "/interrupted"))
            .unwrap();
        let interrupted_id = interrupted.id;
        drop(first_process);

        let store = RequestStore::open(temp.path()).unwrap();
        let (completed, _) = store
            .begin(ObservedRequest::test("GET", "/completed"))
            .unwrap();
        let completed_id = completed.id.clone();
        store
            .finish(
                &completed,
                std::time::Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Rejected,
                None,
            )
            .unwrap();
        let (active, _) = store
            .begin(ObservedRequest::test("GET", "/active"))
            .unwrap();
        let list = list_requests_inner(&store, None).unwrap();

        assert_eq!(list.total, 3);
        assert_eq!(list.deletable_count, 2);
        for (id, state, outcome, has_duration, has_end_time) in [
            (active.id.as_str(), "active", "active", true, false),
            (completed_id.as_str(), "completed", "rejected", true, true),
            (
                interrupted_id.as_str(),
                "interrupted",
                "interrupted",
                false,
                false,
            ),
        ] {
            let request = list
                .requests
                .iter()
                .find(|request| request.id == id)
                .unwrap();
            assert_eq!(
                (request.state.as_str(), request.outcome.as_str()),
                (state, outcome)
            );
            assert_eq!(request.total_ms.is_some(), has_duration);
            assert_eq!(request.ended_at.is_some(), has_end_time);
            assert!(request.http_version.is_none());
            assert!(request.protocol.is_some());
            let expected_assessment = match state {
                "active" => "active",
                "interrupted" => "warning",
                _ => "error",
            };
            assert_eq!(
                serde_json::to_value(&request.assessment).unwrap()["level"],
                expected_assessment
            );
        }

        let completed_summary = list
            .requests
            .iter()
            .find(|request| request.id == completed_id)
            .unwrap();
        let completed_detail = store.find(&completed_id).unwrap();
        assert_eq!(
            completed_summary.ended_at.as_ref(),
            completed_detail
                .result
                .as_ref()
                .map(|result| &result.ended_at)
        );

        let (responded, _) = store
            .begin(ObservedRequest::test("GET", "/responded"))
            .unwrap();
        store
            .write_response(
                &responded.locator,
                &responded.summary,
                &ResponseMetadata {
                    format_version: FORMAT_VERSION,
                    source: ResponseSource::Upstream,
                    headers_at: "2026-08-06T04:00:00Z".to_string(),
                    status: 204,
                    http_version: "HTTP/2".to_string(),
                    headers: Vec::new(),
                },
            )
            .unwrap();
        store
            .finish(
                &responded,
                std::time::Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Completed,
                None,
            )
            .unwrap();
        let responded_summary = list_requests_inner(&store, None)
            .unwrap()
            .requests
            .into_iter()
            .find(|request| request.id == responded.id)
            .unwrap();
        assert_eq!(responded_summary.status, Some(204));
        assert_eq!(responded_summary.http_version.as_deref(), Some("HTTP/2"));
        assert_eq!(responded_summary.assessment.level, AssessmentLevel::Ok);
    }

    #[tokio::test]
    async fn http_and_provider_failures_remain_independent_in_list_and_detail() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path()).unwrap();
        let mut ids = Vec::new();
        for (status, provider_error) in [(401, false), (200, true)] {
            let (request, _) = state
                .store
                .begin(ObservedRequest {
                    upstream_url: Some("https://api.example.test/v1/responses"),
                    host_hint: Some("api.example.test"),
                    ..ObservedRequest::test("POST", "/https://api.example.test/v1/responses")
                })
                .unwrap();
            state
                .store
                .write_response(
                    &request.locator,
                    &request.summary,
                    &ResponseMetadata {
                        format_version: FORMAT_VERSION,
                        source: ResponseSource::Upstream,
                        headers_at: "2026-08-06T04:00:00Z".to_string(),
                        status,
                        http_version: "HTTP/2".to_string(),
                        headers: Vec::new(),
                    },
                )
                .unwrap();
            if provider_error {
                state
                    .store
                    .update_summary(&request.locator, &request.summary, |summary| {
                        summary
                            .protocol
                            .as_mut()
                            .unwrap()
                            .errors
                            .push(ProtocolDiagnostic {
                                kind: "service_unavailable_error".to_string(),
                                message:
                                    "Our servers are currently overloaded. Please try again later."
                                        .to_string(),
                                at_ns: Some("20".to_string()),
                            });
                        true
                    })
                    .unwrap();
            }
            state
                .store
                .finish(
                    &request,
                    std::time::Instant::now(),
                    &RuntimeMeasurements::default(),
                    Outcome::Completed,
                    None,
                )
                .unwrap();
            ids.push((request.id, status, provider_error));
        }

        let list = list_requests_inner(&state.store, None).unwrap();
        for (id, status, provider_error) in ids {
            let row = list
                .requests
                .iter()
                .find(|request| request.id == id)
                .unwrap();
            assert_eq!(row.status, Some(status));
            assert_eq!(row.assessment.level, AssessmentLevel::Error);
            assert_eq!(
                row.assessment.primary.as_ref().unwrap().source,
                if provider_error {
                    AssessmentSource::Provider
                } else {
                    AssessmentSource::Http
                }
            );

            let response = request_detail(State(state.clone()), Path(id)).await;
            assert_eq!(response.status(), StatusCode::OK);
            let detail = response_json(response).await;
            assert_eq!(detail["response"]["status"], status);
            assert_eq!(detail["assessment"]["level"], "error");
            if provider_error {
                assert_eq!(
                    detail["diagnostics"]["provider"].as_array().unwrap().len(),
                    1
                );
                assert!(detail["diagnostics"]["http"].as_array().unwrap().is_empty());
            } else {
                assert_eq!(detail["diagnostics"]["http"].as_array().unwrap().len(), 1);
                assert!(
                    detail["diagnostics"]["provider"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );
            }
        }
    }

    #[test]
    fn request_list_returns_persisted_protocol_without_interpreting_bodies() {
        let temp = tempfile::tempdir().unwrap();
        let store = RequestStore::open(temp.path()).unwrap();
        let (mut request, _) = store
            .begin(ObservedRequest {
                upstream_url: Some("https://example.test/v1/responses"),
                host_hint: Some("example.test"),
                ..ObservedRequest::test("POST", "/https://example.test/v1/responses")
            })
            .unwrap();
        request.request_body.write_all(b"not request json").unwrap();
        request
            .response_body
            .write_all(b"not response json")
            .unwrap();
        store
            .update_summary(&request.locator, &request.summary, |summary| {
                summary.protocol.as_mut().unwrap().model.requested =
                    Some("persisted-list-model".to_string());
                true
            })
            .unwrap();

        let list = list_requests_inner(&store, None).unwrap();
        let protocol = list.requests[0].protocol.as_ref().unwrap();
        assert_eq!(
            protocol.model.requested.as_deref(),
            Some("persisted-list-model")
        );
        assert!(protocol.warnings.is_empty());
    }

    #[test]
    fn request_list_does_not_parse_the_optional_event_timing_index() {
        let temp = tempfile::tempdir().unwrap();
        let store = RequestStore::open(temp.path()).unwrap();
        let (request, _) = store
            .begin(ObservedRequest::test("GET", "/events"))
            .unwrap();
        let mut index = store.create_event_index(&request).unwrap();
        writeln!(index, "not json").unwrap();
        index.flush().unwrap();

        let list = list_requests_inner(&store, None).unwrap();
        assert_eq!(list.requests.len(), 1);
    }

    #[tokio::test]
    async fn active_durations_do_not_depend_on_the_wall_clock_anchor() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path()).unwrap();
        let (request, _) = state
            .store
            .begin(ObservedRequest::test("GET", "/active"))
            .unwrap();
        state
            .store
            .update_summary(&request.locator, &request.summary, |summary| {
                summary.observed_at = "9999-01-01T00:00:00Z".to_string();
                true
            })
            .unwrap();

        let list = list_requests_inner(&state.store, None).unwrap();
        let list_total_ms = list.requests[0].total_ms.unwrap();

        let response = request_detail(State(state), Path(request.id)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        let detail_total_ms = json["live_total_ms"].as_u64().unwrap();
        let timeline_end_ns = json["timeline_end_at_ns"]
            .as_str()
            .unwrap()
            .parse::<u128>()
            .unwrap();
        assert!(detail_total_ms >= list_total_ms);
        assert_eq!(u128::from(detail_total_ms), timeline_end_ns / 1_000_000);
    }

    #[tokio::test]
    async fn request_detail_adds_event_timing_index_diagnostics_on_demand() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path()).unwrap();
        let (request, _) = state
            .store
            .begin(ObservedRequest::test("GET", "/events"))
            .unwrap();
        let mut index = state.store.create_event_index(&request).unwrap();
        writeln!(index, "not json").unwrap();
        index.flush().unwrap();

        let response = request_detail(State(state), Path(request.id)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        let warnings = json["summary"]["warnings"].as_array().unwrap();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0]["kind"], "event_index_failed");
        assert!(warnings[0]["message"].as_str().unwrap().contains("line 1"));
        assert_eq!(json["assessment"]["level"], "active");
        assert_eq!(json["assessment"]["issue_count"], 1);
        assert_eq!(json["diagnostics"]["warnings"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn request_detail_ignores_only_an_active_unterminated_event_index_tail() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path()).unwrap();
        let (request, _) = state
            .store
            .begin(ObservedRequest::test("GET", "/events"))
            .unwrap();
        let mut index = state.store.create_event_index(&request).unwrap();
        write!(index, "{{\"schema_version\":").unwrap();
        index.flush().unwrap();

        let response = request_detail(State(state.clone()), Path(request.id.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert!(json["summary"]["warnings"].as_array().unwrap().is_empty());

        state.store.abandon_active(&request.id);
        let response = request_detail(State(state), Path(request.id)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["summary"]["warnings"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn detail_response_adds_canonical_reason_without_mutating_raw_metadata() {
        let detail = ResponseDetail::from(ResponseMetadata {
            format_version: FORMAT_VERSION,
            source: ResponseSource::Upstream,
            headers_at: "2026-08-06T04:00:00Z".to_string(),
            status: 200,
            http_version: "HTTP/2".to_string(),
            headers: Vec::new(),
        });
        assert_eq!(detail.reason_phrase.as_deref(), Some("OK"));
        let json = serde_json::to_value(detail).unwrap();
        assert_eq!(json["reason_phrase"], "OK");
        assert_eq!(json["format_version"], FORMAT_VERSION);
    }

    #[tokio::test]
    async fn detail_response_includes_timeline_and_persisted_protocol_summary() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path()).unwrap();
        let (mut request, _) = state
            .store
            .begin(ObservedRequest {
                upstream_url: Some("https://example.test/v1/responses"),
                host_hint: Some("example.test"),
                ..ObservedRequest::test("POST", "/https://example.test/v1/responses")
            })
            .unwrap();
        request.request_body.write_all(b"not request json").unwrap();
        request
            .response_body
            .write_all(b"not response json")
            .unwrap();
        state
            .store
            .update_summary(&request.locator, &request.summary, |summary| {
                let protocol = summary.protocol.as_mut().unwrap();
                protocol.model.requested = Some("persisted-model".to_string());
                protocol.response_terminal = true;
                true
            })
            .unwrap();
        let id = request.id.clone();
        state
            .store
            .finish(
                &request,
                std::time::Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Completed,
                None,
            )
            .unwrap();

        let response = request_detail(State(state), Path(id)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["state"], "completed");
        assert_eq!(json["request"]["method"], "POST");
        assert_eq!(
            json["request"]["incoming_uri"],
            "/https://example.test/v1/responses"
        );
        assert_eq!(json["request"]["http_version"], "HTTP/1.1");
        assert!(json["timeline_end_at_ns"].as_str().is_some());
        assert!(json.get("interpretation").is_none());
        assert_eq!(json["summary"]["protocol"]["family"], "openai_responses");
        assert_eq!(
            json["summary"]["protocol"]["model"]["requested"],
            "persisted-model"
        );
        assert_eq!(json["summary"]["protocol"]["response_terminal"], true);
        assert!(
            json["summary"]["timing"]["finished_at_ns"]
                .as_str()
                .is_some()
        );
    }

    #[tokio::test]
    async fn body_api_streams_exact_offsets_and_reports_invalid_ranges() {
        let temp = tempfile::tempdir().unwrap();
        let store = RequestStore::open(temp.path()).unwrap();
        let id = finished_request(&store, "/body", b"abc\0\xff", b"response");

        for (response_body, offset, expected, length, next_offset) in [
            (false, 2, &b"c\0\xff"[..], "3", "5"),
            (false, 5, &b""[..], "0", "5"),
            (true, 1, &b"esponse"[..], "7", "8"),
        ] {
            let response = body_response(&store, &id, response_body, offset).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_LENGTH], length);
            assert_eq!(
                response.headers()["x-aibox-request-next-offset"],
                next_offset
            );
            let body = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(body.as_ref(), expected);
        }

        let invalid_range = body_response(&store, &id, false, 6).await;
        assert_eq!(invalid_range.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        let missing = body_response(&store, &Uuid::now_v7().to_string(), false, 0).await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn decoded_body_api_handles_identity_and_zstd_without_changing_raw_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let store = RequestStore::open(temp.path()).unwrap();
        let identity_id = finished_request(&store, "/identity", b"plain request", b"");
        let identity = decoded_body_response(&store, &identity_id, false).await;
        assert_eq!(identity.status(), StatusCode::OK);
        assert_eq!(
            identity.into_body().collect().await.unwrap().to_bytes(),
            "plain request"
        );

        let request_source = br#"{"model":"compressed-request"}"#;
        let response_source = br#"{"result":"compressed-response"}"#;
        let request_compressed = zstd::stream::encode_all(request_source.as_slice(), 0).unwrap();
        let response_compressed = zstd::stream::encode_all(response_source.as_slice(), 0).unwrap();
        let (mut request, _) = store
            .begin(ObservedRequest {
                headers: vec![recorded_header("content-encoding", " ZsTd ")],
                ..ObservedRequest::test("POST", "/zstd")
            })
            .unwrap();
        request.request_body.write_all(&request_compressed).unwrap();
        request
            .response_body
            .write_all(&response_compressed)
            .unwrap();
        store
            .write_response(
                &request.locator,
                &request.summary,
                &ResponseMetadata {
                    format_version: FORMAT_VERSION,
                    source: ResponseSource::Upstream,
                    headers_at: "2026-08-09T00:00:00Z".to_string(),
                    status: 200,
                    http_version: "HTTP/2".to_string(),
                    headers: vec![recorded_header("content-encoding", "zstd")],
                },
            )
            .unwrap();
        let id = request.id.clone();
        store
            .finish(
                &request,
                std::time::Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Completed,
                None,
            )
            .unwrap();

        for (response_body, expected) in [
            (false, request_source.as_slice()),
            (true, response_source.as_slice()),
        ] {
            let decoded = decoded_body_response(&store, &id, response_body).await;
            assert_eq!(decoded.status(), StatusCode::OK);
            assert_eq!(
                decoded.into_body().collect().await.unwrap().to_bytes(),
                expected
            );
            let raw = body_response(&store, &id, response_body, 0).await;
            let expected_raw = if response_body {
                &response_compressed
            } else {
                &request_compressed
            };
            assert_eq!(
                raw.into_body().collect().await.unwrap().to_bytes(),
                expected_raw.as_slice()
            );
        }
    }

    #[tokio::test]
    async fn decoded_body_api_rejects_incomplete_unsupported_and_corrupt_content() {
        let temp = tempfile::tempdir().unwrap();
        let store = RequestStore::open(temp.path()).unwrap();
        let (mut active, _) = store
            .begin(ObservedRequest {
                headers: vec![recorded_header("content-encoding", "zstd")],
                ..ObservedRequest::test("POST", "/active")
            })
            .unwrap();
        active.request_body.write_all(b"partial").unwrap();
        let waiting = decoded_body_response(&store, &active.id, false).await;
        assert_eq!(waiting.status(), StatusCode::CONFLICT);

        let (mut unsupported, _) = store
            .begin(ObservedRequest {
                headers: vec![recorded_header("content-encoding", "gzip, zstd")],
                ..ObservedRequest::test("POST", "/unsupported")
            })
            .unwrap();
        unsupported.request_body.write_all(b"encoded").unwrap();
        let unsupported_id = unsupported.id.clone();
        store
            .finish(
                &unsupported,
                std::time::Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Rejected,
                None,
            )
            .unwrap();
        let response = decoded_body_response(&store, &unsupported_id, false).await;
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let (mut corrupt, _) = store
            .begin(ObservedRequest {
                headers: vec![recorded_header("content-encoding", "zstd")],
                ..ObservedRequest::test("POST", "/corrupt")
            })
            .unwrap();
        corrupt.request_body.write_all(b"not zstd").unwrap();
        let corrupt_id = corrupt.id.clone();
        store
            .finish(
                &corrupt,
                std::time::Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Rejected,
                None,
            )
            .unwrap();
        let response = decoded_body_response(&store, &corrupt_id, false).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.into_body().collect().await.is_err());
    }

    #[tokio::test]
    async fn event_timing_api_returns_incremental_valid_entries_and_partial_state() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path()).unwrap();
        let (request, _) = state
            .store
            .begin(ObservedRequest::test("GET", "/events"))
            .unwrap();
        let mut index = state.store.create_event_index(&request).unwrap();
        for (sequence, completed_at_ns) in [(0, "1000000"), (1, "2500000")] {
            writeln!(
                index,
                "{}",
                json!({
                    "schema_version": FORMAT_VERSION,
                    "request_id": request.id,
                    "kind": "sse_event",
                    "sequence": sequence,
                    "body_start": sequence * 10,
                    "body_end": sequence * 10 + 9,
                    "first_arrival_at_ns": completed_at_ns,
                    "completed_at_ns": completed_at_ns,
                })
            )
            .unwrap();
        }
        writeln!(index, "not json").unwrap();
        index.flush().unwrap();
        let id = request.id.clone();
        state
            .store
            .finish(
                &request,
                std::time::Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Completed,
                None,
            )
            .unwrap();

        let response = response_event_timings(
            State(state),
            Path(id),
            Query(EventTimingQuery { after_sequence: 1 }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["state"], "partial");
        assert_eq!(body["next_sequence"], 2);
        assert_eq!(body["events"].as_array().unwrap().len(), 1);
        assert_eq!(body["events"][0]["sequence"], 1);
        assert!(body["warning"].as_str().unwrap().contains("line 3"));
    }

    #[test]
    fn active_event_timing_reader_ignores_an_unterminated_tail() {
        let temp = tempfile::tempdir().unwrap();
        let store = RequestStore::open(temp.path()).unwrap();
        let (request, _) = store
            .begin(ObservedRequest::test("GET", "/events"))
            .unwrap();
        let mut index = store.create_event_index(&request).unwrap();
        write!(index, "{{\"schema_version\":1").unwrap();
        index.flush().unwrap();

        let timings = store.read_event_timings(&request.id, 0).unwrap();
        assert!(timings.available);
        assert!(!timings.partial);
        assert!(timings.events.is_empty());
    }

    #[tokio::test]
    async fn event_timing_api_reports_a_missing_index_as_unavailable() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path()).unwrap();
        let id = finished_request(&state.store, "/without-events", b"", b"");

        let response = response_event_timings(
            State(state),
            Path(id),
            Query(EventTimingQuery { after_sequence: 7 }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["state"], "unavailable");
        assert_eq!(body["events"], json!([]));
        assert_eq!(body["next_sequence"], 7);
        assert!(body["warning"].as_str().unwrap().contains("unavailable"));
    }

    #[tokio::test]
    async fn deletion_api_maps_selection_conflicts_and_successes() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path()).unwrap();
        let (active, _) = state
            .store
            .begin(ObservedRequest::test("GET", "/active"))
            .unwrap();

        let conflict = delete_requests(
            State(state.clone()),
            Json(DeleteRequest {
                ids: vec![active.id.clone()],
            }),
        )
        .await;
        assert_eq!(conflict.status(), StatusCode::CONFLICT);

        state
            .store
            .finish(
                &active,
                std::time::Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Rejected,
                None,
            )
            .unwrap();
        let second = finished_request(&state.store, "/delete-selected", b"", b"");
        let deleted = delete_requests(
            State(state),
            Json(DeleteRequest {
                ids: vec![active.id, second],
            }),
        )
        .await;
        assert_eq!(deleted.status(), StatusCode::OK);
        assert_eq!(response_json(deleted).await, json!({"deleted": 2}));
    }

    #[test]
    fn pagination_is_fixed_at_fifty_and_recomputes_each_page_from_current_order() {
        let temp = tempfile::tempdir().unwrap();
        let store = RequestStore::open(temp.path()).unwrap();
        for _ in 0..51 {
            finished_request(&store, "/bad", b"", b"");
        }
        let first = list_requests_inner(&store, None).unwrap();
        assert_eq!(first.total, 51);
        assert_eq!(first.requests.len(), 50);
        assert!(first.has_next);
        let second = list_requests_inner(&store, Some(2)).unwrap();
        assert_eq!(second.requests.len(), 1);
        assert!(!second.has_next);

        finished_request(&store, "/new", b"", b"");
        let recomputed_second = list_requests_inner(&store, Some(2)).unwrap();
        assert_eq!(recomputed_second.total, 52);
        assert_eq!(recomputed_second.requests.len(), 2);
        assert_eq!(recomputed_second.requests[1].id, second.requests[0].id);
        assert!(
            list_requests_inner(&store, Some(3))
                .unwrap()
                .requests
                .is_empty()
        );
    }

    #[test]
    fn invalid_page_is_rejected_before_the_store_is_scanned() {
        let temp = tempfile::tempdir().unwrap();
        let store = RequestStore::open(temp.path()).unwrap();
        std::fs::remove_dir(store.root()).unwrap();
        std::fs::write(store.root(), b"not a directory").unwrap();

        for (page, expected) in [
            (0, "Request page must be a positive integer"),
            (u64::MAX, "Request page is too large"),
        ] {
            let error = list_requests_inner(&store, Some(page))
                .err()
                .expect("invalid page must be rejected before scanning");
            assert_eq!(error.to_string(), expected, "page={page}");
        }
    }

    #[test]
    fn request_list_uses_terminal_end_order() {
        let temp = tempfile::tempdir().unwrap();
        let store = RequestStore::open(temp.path()).unwrap();
        let (first, _) = store.begin(ObservedRequest::test("GET", "/first")).unwrap();
        let (second, _) = store
            .begin(ObservedRequest::test("GET", "/second"))
            .unwrap();

        for request in [&second, &first] {
            store
                .finish(
                    request,
                    std::time::Instant::now(),
                    &RuntimeMeasurements::default(),
                    Outcome::Completed,
                    None,
                )
                .unwrap();
        }

        let requests = list_requests_inner(&store, None).unwrap().requests;
        assert_eq!(requests[0].id, first.id);
        assert_eq!(requests[1].id, second.id);
        assert!(requests.iter().all(|request| request.ended_at.is_some()));
    }
}
