use crate::traffic::AppState;
use crate::traffic_interpretation::{
    BodyContentCoding, ProtocolSummary, body_content_coding, timeline_end_at_ns,
};
use crate::traffic_proxy;
use crate::traffic_store::{
    AssessmentFinding, AssessmentLevel, AssessmentSource, RecordAssessment, RecordDetailReadError,
    RecordedHeader, ResponseMetadata, ResponseSource, StoredRecordSummary, SummaryMetadata,
    TrafficStore, anchored_at, diagnostic_findings, effective_assessment,
};
use anyhow::Context as _;
use axum::Json;
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Query, Request, State};
use axum::http::{HeaderValue, Method, Response, StatusCode, header};
use axum::middleware::Next;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::Read as _;
use std::net::SocketAddr;
use tokio::io::AsyncReadExt as _;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::io::ReaderStream;

const PAGE_SIZE: usize = 50;
const HTML: &str = include_str!("../assets/traffic.html");
const CSS: &str = include_str!("../assets/traffic.css");
const JS: &str = include_str!("../assets/traffic.js");

pub(super) async fn security_middleware(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response<Body> {
    if let Err(message) = validate_management_request(&state, peer, &request) {
        return secure_response(traffic_proxy::bare_error(StatusCode::FORBIDDEN, &message));
    }
    secure_response(next.run(request).await)
}

fn validate_management_request(
    state: &AppState,
    peer: SocketAddr,
    request: &Request,
) -> Result<(), String> {
    if !peer.ip().is_loopback() {
        return Err("the Traffic management interface is loopback-only".to_string());
    }
    let allowed_host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| allowed_loopback_host(host, state.port));
    if !allowed_host {
        return Err("invalid Host for the Traffic management interface".to_string());
    }
    if let Some(site) = request
        .headers()
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        && !matches!(site, "none" | "same-origin")
    {
        return Err("cross-site management requests are not accepted".to_string());
    }
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let origin_allowed = origin.is_some_and(|origin| allowed_loopback_origin(origin, state.port));
    if origin.is_some() && !origin_allowed {
        return Err("invalid Origin for the Traffic management interface".to_string());
    }
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        if !origin_allowed {
            return Err("mutating management requests require the loopback Origin".to_string());
        }
        let token = request
            .headers()
            .get("x-aibox-traffic-csrf")
            .and_then(|value| value.to_str().ok());
        if token != Some(state.csrf.as_str()) {
            return Err("invalid Traffic management CSRF token".to_string());
        }
    }
    Ok(())
}

fn allowed_loopback_origin(origin: &str, port: u16) -> bool {
    let mut allowed = vec![
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
        format!("http://[::1]:{port}"),
    ];
    if port == 80 {
        allowed.extend([
            "http://127.0.0.1".to_string(),
            "http://localhost".to_string(),
            "http://[::1]".to_string(),
        ]);
    }
    allowed
        .iter()
        .any(|allowed| origin.eq_ignore_ascii_case(allowed))
}

fn allowed_loopback_host(host: &str, port: u16) -> bool {
    let mut allowed = vec![
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    if port == 80 {
        allowed.extend([
            "127.0.0.1".to_string(),
            "localhost".to_string(),
            "[::1]".to_string(),
        ]);
    }
    allowed
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
}

fn secure_response(mut response: Response<Body>) -> Response<Body> {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert("pragma", HeaderValue::from_static("no-cache"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "content-security-policy",
        HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        ),
    );
    response
}

pub(super) async fn index(State(state): State<AppState>) -> Response<Body> {
    let html = HTML.replace("__AIBOX_CSRF__", &state.csrf);
    content(StatusCode::OK, "text/html; charset=utf-8", html)
}

pub(super) async fn css() -> Response<Body> {
    content(StatusCode::OK, "text/css; charset=utf-8", CSS)
}

pub(super) async fn js() -> Response<Body> {
    content(StatusCode::OK, "application/javascript; charset=utf-8", JS)
}

pub(super) async fn not_found() -> Response<Body> {
    traffic_proxy::bare_error(StatusCode::NOT_FOUND, "Traffic management route not found")
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
pub(super) struct ListQuery {
    page: Option<u64>,
}

#[derive(Serialize)]
struct RecordSummary {
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
    assessment: RecordAssessment,
}

#[derive(Serialize)]
struct RecordList {
    records: Vec<RecordSummary>,
    total: usize,
    deletable_count: usize,
    has_next: bool,
}

pub(super) async fn list_records(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Response<Body> {
    let store = state.store.clone();
    match tokio::task::spawn_blocking(move || list_records_inner(&store, query.page)).await {
        Ok(Ok(value)) => json_response(StatusCode::OK, &value),
        Ok(Err(error)) => json_error(StatusCode::BAD_REQUEST, &error.to_string()),
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("scan Traffic Records: {error}"),
        ),
    }
}

fn list_records_inner(store: &TrafficStore, page: Option<u64>) -> anyhow::Result<RecordList> {
    let page = page.unwrap_or(1);
    if page == 0 {
        anyhow::bail!("Traffic Record page must be a positive integer");
    }
    let start = usize::try_from(page - 1)
        .ok()
        .and_then(|page| page.checked_mul(PAGE_SIZE))
        .context("Traffic Record page is too large")?;
    let records = store.scan_summaries()?;
    let total = records.len();
    let deletable_count = records.iter().filter(|record| !record.active).count();
    let has_next = start
        .checked_add(PAGE_SIZE)
        .is_some_and(|next| next < total);
    let records = records
        .iter()
        .skip(start)
        .take(PAGE_SIZE)
        .map(summary)
        .collect();
    Ok(RecordList {
        records,
        total,
        deletable_count,
        has_next,
    })
}

fn summary(record: &StoredRecordSummary) -> RecordSummary {
    let value = &record.summary;
    let (state, outcome) = if record.active {
        ("active", "active")
    } else if let Some(outcome) = value.outcome {
        ("completed", outcome.as_str())
    } else {
        ("interrupted", "interrupted")
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
    RecordSummary {
        id: value.record_id.clone(),
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
        total_ms: if record.active {
            elapsed_wall_ms(&value.observed_at, None)
        } else {
            value
                .timing
                .finished_at_ns
                .as_deref()
                .and_then(|value| value.parse::<u128>().ok())
                .and_then(|value| u64::try_from(value / 1_000_000).ok())
        },
        protocol: value.protocol.clone(),
        assessment: effective_assessment(value, record.active),
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
            format_version: crate::traffic_store::FORMAT_VERSION,
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
struct RecordDetail {
    request: crate::traffic_store::RequestMetadata,
    response: Option<ResponseDetail>,
    result: Option<crate::traffic_store::ResultMetadata>,
    summary: SummaryMetadata,
    assessment: RecordAssessment,
    diagnostics: DiagnosticGroups,
    state: String,
    request_body_bytes: u64,
    response_body_bytes: u64,
    live_total_ms: Option<u64>,
    timeline_end_at_ns: Option<String>,
}

#[derive(Serialize)]
struct DiagnosticGroups {
    traffic: Vec<AssessmentFinding>,
    http: Vec<AssessmentFinding>,
    provider: Vec<AssessmentFinding>,
    warnings: Vec<AssessmentFinding>,
}

fn diagnostic_groups(summary: &SummaryMetadata, interrupted: bool) -> DiagnosticGroups {
    let mut groups = DiagnosticGroups {
        traffic: Vec::new(),
        http: Vec::new(),
        provider: Vec::new(),
        warnings: Vec::new(),
    };
    for finding in diagnostic_findings(summary, interrupted) {
        if finding.level == AssessmentLevel::Warning {
            groups.warnings.push(finding);
        } else {
            match finding.source {
                AssessmentSource::Traffic => groups.traffic.push(finding),
                AssessmentSource::Http => groups.http.push(finding),
                AssessmentSource::Provider => groups.provider.push(finding),
                AssessmentSource::Diagnostic => groups.warnings.push(finding),
            }
        }
    }
    groups
}

pub(super) async fn record_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response<Body> {
    let store = state.store.clone();
    let lookup_id = id.clone();
    let lookup =
        tokio::task::spawn_blocking(move || store.find_with_event_index_warnings(&lookup_id)).await;
    match lookup {
        Ok(Ok(record)) => {
            let state_name = if record.active {
                "active"
            } else if record.result.is_none() {
                "interrupted"
            } else {
                "completed"
            };
            let live_total_ms = record
                .active
                .then(|| elapsed_wall_ms(&record.request.started_at, None))
                .flatten();
            let interrupted = !record.active && record.result.is_none();
            let assessment = effective_assessment(&record.summary, record.active);
            let diagnostics = diagnostic_groups(&record.summary, interrupted);
            let live_elapsed_ns = state.store.live_elapsed_ns(&id);
            let timeline_end_at_ns = timeline_end_at_ns(&record, live_elapsed_ns);
            let response_headers_at = record
                .summary
                .timing
                .upstream_response_headers_at_ns
                .as_deref()
                .and_then(|offset| anchored_at(&record.summary.observed_at, offset));
            let response = record.response.map(|metadata| {
                let mut detail = ResponseDetail::from(metadata);
                if let Some(headers_at) = &response_headers_at {
                    detail.headers_at = headers_at.clone();
                }
                detail
            });
            json_response(
                StatusCode::OK,
                &RecordDetail {
                    request: record.request,
                    response,
                    result: record.result,
                    summary: record.summary,
                    assessment,
                    diagnostics,
                    state: state_name.to_string(),
                    request_body_bytes: record.request_body_bytes,
                    response_body_bytes: record.response_body_bytes,
                    live_total_ms,
                    timeline_end_at_ns,
                },
            )
        }
        Ok(Err(RecordDetailReadError::Lookup(error))) => {
            json_error(StatusCode::NOT_FOUND, &error.to_string())
        }
        Ok(Err(RecordDetailReadError::EventIndex(error))) => {
            json_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
        }
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("read Traffic Record detail: {error}"),
        ),
    }
}

fn elapsed_wall_ms(started: &str, ended: Option<&str>) -> Option<u64> {
    use time::format_description::well_known::Rfc3339;
    let started = time::OffsetDateTime::parse(started, &Rfc3339).ok()?;
    let ended = ended
        .and_then(|value| time::OffsetDateTime::parse(value, &Rfc3339).ok())
        .unwrap_or_else(time::OffsetDateTime::now_utc);
    u64::try_from((ended - started).whole_milliseconds()).ok()
}

#[derive(Deserialize)]
pub(super) struct BodyQuery {
    #[serde(default)]
    offset: u64,
}

pub(super) async fn request_body(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<BodyQuery>,
) -> Response<Body> {
    body_response(&state.store, &id, false, query.offset).await
}

pub(super) async fn response_body(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<BodyQuery>,
) -> Response<Body> {
    body_response(&state.store, &id, true, query.offset).await
}

pub(super) async fn decoded_request_body(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response<Body> {
    decoded_body_response(&state.store, &id, false).await
}

pub(super) async fn decoded_response_body(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response<Body> {
    decoded_body_response(&state.store, &id, true).await
}

async fn body_response(
    store: &TrafficStore,
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
                &format!("open Traffic body: {error}"),
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
        "x-aibox-traffic-next-offset",
        HeaderValue::from_str(&length.to_string()).expect("body offset is a valid header"),
    );
    response
}

async fn decoded_body_response(store: &TrafficStore, id: &str, response: bool) -> Response<Body> {
    let lookup_store = store.clone();
    let lookup_id = id.to_string();
    let record = match tokio::task::spawn_blocking(move || lookup_store.find(&lookup_id)).await {
        Ok(Ok(record)) => record,
        Ok(Err(error)) => return json_error(StatusCode::NOT_FOUND, &error.to_string()),
        Err(error) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("read Traffic Record for body decoding: {error}"),
            );
        }
    };
    let completed = if response {
        record
            .summary
            .timing
            .upstream_response_body_completed_at_ns
            .is_some()
    } else {
        record
            .summary
            .timing
            .upstream_request_body_completed_at_ns
            .is_some()
    };
    if record.active && !completed {
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
        record
            .response
            .as_ref()
            .map(|metadata| metadata.headers.as_slice())
            .unwrap_or_default()
    } else {
        &record.request.headers
    };
    let coding = match body_content_coding(headers) {
        Ok(coding) => coding,
        Err(error) => return json_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, &error.to_string()),
    };
    let body_store = store.clone();
    let opened =
        tokio::task::spawn_blocking(move || body_store.open_record_body(&record, response, 0))
            .await;
    let (file, length) = match opened {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => return json_error(StatusCode::NOT_FOUND, &error.to_string()),
        Err(error) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("open Traffic body for decoding: {error}"),
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
pub(super) struct EventTimingQuery {
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

pub(super) async fn response_event_timings(
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
            &format!("read Traffic SSE Event timings: {error}"),
        ),
    }
}

#[derive(Deserialize)]
pub(super) struct DeleteRequest {
    ids: Vec<String>,
}

pub(super) async fn delete_records(
    State(state): State<AppState>,
    Json(request): Json<DeleteRequest>,
) -> Response<Body> {
    let store = state.store.clone();
    let deleted = tokio::task::spawn_blocking(move || store.delete_ids(&request.ids)).await;
    match deleted {
        Ok(Ok(deleted)) => json_response(StatusCode::OK, &json!({"deleted": deleted})),
        Ok(Err(error)) => {
            let status = if error.to_string().contains("active Traffic") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            json_error(status, &error.to_string())
        }
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("delete Traffic Records: {error}"),
        ),
    }
}

#[derive(Deserialize)]
pub(super) struct DeleteAllRequest {
    expected_deletable_count: usize,
}

pub(super) async fn delete_all(
    State(state): State<AppState>,
    Json(request): Json<DeleteAllRequest>,
) -> Response<Body> {
    let store = state.store.clone();
    let deleted =
        tokio::task::spawn_blocking(move || store.delete_all(request.expected_deletable_count))
            .await;
    match deleted {
        Ok(Ok(deleted)) => json_response(StatusCode::OK, &json!({"deleted": deleted})),
        Ok(Err(error)) if error.to_string().contains("count changed") => {
            json_error(StatusCode::CONFLICT, &error.to_string())
        }
        Ok(Err(error)) => json_error(StatusCode::BAD_REQUEST, &error.to_string()),
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("delete all Traffic Records: {error}"),
        ),
    }
}

fn json_response<T: Serialize>(status: StatusCode, value: &T) -> Response<Body> {
    match serde_json::to_vec(value) {
        Ok(bytes) => content(status, "application/json; charset=utf-8", bytes),
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("serialize management response: {error}"),
        ),
    }
}

fn json_error(status: StatusCode, message: &str) -> Response<Body> {
    let body = serde_json::to_vec(&json!({"error": message}))
        .unwrap_or_else(|_| b"{\"error\":\"management error\"}".to_vec());
    content(status, "application/json; charset=utf-8", body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use base64::Engine as _;
    use http_body_util::BodyExt as _;
    use std::io::Write as _;
    use uuid::Uuid;

    fn management_request(
        method: Method,
        host: Option<&str>,
        origin: Option<&str>,
        fetch_site: Option<&str>,
        csrf: Option<&str>,
    ) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri("/_aibox/traffic/api/records");
        for (name, value) in [
            (header::HOST.as_str(), host),
            (header::ORIGIN.as_str(), origin),
            ("sec-fetch-site", fetch_site),
            ("x-aibox-traffic-csrf", csrf),
        ] {
            if let Some(value) = value {
                builder = builder.header(name, value);
            }
        }
        builder.body(Body::empty()).unwrap()
    }

    fn finished_record(
        store: &TrafficStore,
        incoming_uri: &str,
        request_body: &[u8],
        response_body: &[u8],
    ) -> String {
        let (mut record, _) = store
            .begin("POST", incoming_uri, None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        record.request_body.write_all(request_body).unwrap();
        record.request_body.flush().unwrap();
        record.response_body.write_all(response_body).unwrap();
        record.response_body.flush().unwrap();
        let id = record.id.clone();
        store
            .finish(
                &record,
                std::time::Instant::now(),
                &crate::traffic_store::RuntimeMeasurements::default(),
                crate::traffic_store::Outcome::Rejected,
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
    fn management_security_enforces_each_loopback_origin_and_csrf_boundary() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path(), 9923).unwrap();
        for (label, peer, method, host, origin, site, token, expected) in [
            (
                "canonical GET",
                "127.0.0.1:40000",
                Method::GET,
                Some("127.0.0.1:9923"),
                None,
                None,
                None,
                None,
            ),
            (
                "localhost HEAD",
                "127.0.0.1:40000",
                Method::HEAD,
                Some("localhost:9923"),
                None,
                Some("none"),
                None,
                None,
            ),
            (
                "IPv6 loopback GET",
                "[::1]:40000",
                Method::GET,
                Some("[::1]:9923"),
                Some("http://127.0.0.1:9923"),
                Some("same-origin"),
                None,
                None,
            ),
            (
                "authenticated POST",
                "127.0.0.1:40000",
                Method::POST,
                Some("127.0.0.1:9923"),
                Some("http://127.0.0.1:9923"),
                Some("same-origin"),
                Some(true),
                None,
            ),
            (
                "localhost authenticated POST",
                "127.0.0.1:40000",
                Method::POST,
                Some("localhost:9923"),
                Some("http://localhost:9923"),
                Some("same-origin"),
                Some(true),
                None,
            ),
            (
                "IPv6 authenticated POST",
                "[::1]:40000",
                Method::POST,
                Some("[::1]:9923"),
                Some("http://[::1]:9923"),
                Some("same-origin"),
                Some(true),
                None,
            ),
            (
                "remote peer",
                "192.0.2.1:40000",
                Method::GET,
                Some("127.0.0.1:9923"),
                None,
                None,
                None,
                Some("the Traffic management interface is loopback-only"),
            ),
            (
                "untrusted Host",
                "127.0.0.1:40000",
                Method::GET,
                Some("evil.example"),
                None,
                None,
                None,
                Some("invalid Host for the Traffic management interface"),
            ),
            (
                "cross-site fetch",
                "127.0.0.1:40000",
                Method::GET,
                Some("127.0.0.1:9923"),
                None,
                Some("cross-site"),
                None,
                Some("cross-site management requests are not accepted"),
            ),
            (
                "foreign Origin",
                "127.0.0.1:40000",
                Method::GET,
                Some("127.0.0.1:9923"),
                Some("http://evil.example"),
                None,
                None,
                Some("invalid Origin for the Traffic management interface"),
            ),
            (
                "POST without Origin",
                "127.0.0.1:40000",
                Method::POST,
                Some("127.0.0.1:9923"),
                None,
                None,
                Some(true),
                Some("mutating management requests require the loopback Origin"),
            ),
            (
                "POST with bad CSRF",
                "127.0.0.1:40000",
                Method::POST,
                Some("127.0.0.1:9923"),
                Some("http://127.0.0.1:9923"),
                None,
                Some(false),
                Some("invalid Traffic management CSRF token"),
            ),
        ] {
            let csrf = token.map(|valid| {
                if valid {
                    state.csrf.as_str()
                } else {
                    "wrong-token"
                }
            });
            let request = management_request(method, host, origin, site, csrf);
            let actual = validate_management_request(&state, peer.parse().unwrap(), &request).err();
            assert_eq!(actual.as_deref(), expected, "{label}");
        }
    }

    #[test]
    fn management_security_accepts_the_omitted_default_http_port_only_for_port_80() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path(), 80).unwrap();
        let request = management_request(
            Method::POST,
            Some("localhost"),
            Some("http://localhost"),
            Some("same-origin"),
            Some(&state.csrf),
        );
        validate_management_request(&state, "127.0.0.1:40000".parse().unwrap(), &request).unwrap();

        let state = AppState::for_test(temp.path(), 9923).unwrap();
        let request = management_request(Method::GET, Some("localhost"), None, None, None);
        assert!(
            validate_management_request(&state, "127.0.0.1:40000".parse().unwrap(), &request)
                .is_err()
        );
    }

    #[test]
    fn record_summaries_distinguish_active_interrupted_and_completed_state() {
        let temp = tempfile::tempdir().unwrap();
        let first_process = TrafficStore::open(temp.path()).unwrap();
        let (interrupted, _) = first_process
            .begin("GET", "/interrupted", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let interrupted_id = interrupted.id.clone();
        drop(first_process);

        let store = TrafficStore::open(temp.path()).unwrap();
        let (completed, _) = store
            .begin("GET", "/completed", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let completed_id = completed.id.clone();
        store
            .finish(
                &completed,
                std::time::Instant::now(),
                &crate::traffic_store::RuntimeMeasurements::default(),
                crate::traffic_store::Outcome::Rejected,
                None,
            )
            .unwrap();
        let (active, _) = store
            .begin("GET", "/active", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let list = list_records_inner(&store, None).unwrap();

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
            let record = list.records.iter().find(|record| record.id == id).unwrap();
            assert_eq!(
                (record.state.as_str(), record.outcome.as_str()),
                (state, outcome)
            );
            assert_eq!(record.total_ms.is_some(), has_duration);
            assert_eq!(record.ended_at.is_some(), has_end_time);
            assert!(record.http_version.is_none());
            assert!(record.protocol.is_some());
            let expected_assessment = match state {
                "active" => "active",
                "interrupted" => "warning",
                _ => "error",
            };
            assert_eq!(
                serde_json::to_value(&record.assessment).unwrap()["level"],
                expected_assessment
            );
        }

        let completed_summary = list
            .records
            .iter()
            .find(|record| record.id == completed_id)
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
            .begin("GET", "/responded", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        store
            .write_response(
                &responded.locator,
                &responded.summary,
                &ResponseMetadata {
                    format_version: crate::traffic_store::FORMAT_VERSION,
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
                &crate::traffic_store::RuntimeMeasurements::default(),
                crate::traffic_store::Outcome::Completed,
                None,
            )
            .unwrap();
        let responded_summary = list_records_inner(&store, None)
            .unwrap()
            .records
            .into_iter()
            .find(|record| record.id == responded.id)
            .unwrap();
        assert_eq!(responded_summary.status, Some(204));
        assert_eq!(responded_summary.http_version.as_deref(), Some("HTTP/2"));
        assert_eq!(responded_summary.assessment.level, AssessmentLevel::Ok);
    }

    #[tokio::test]
    async fn http_and_provider_failures_remain_independent_in_list_and_detail() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path(), 9923).unwrap();
        let mut ids = Vec::new();
        for (status, provider_error) in [(401, false), (200, true)] {
            let (record, _) = state
                .store
                .begin(
                    "POST",
                    "/https://api.example.test/v1/responses",
                    Some("https://api.example.test/v1/responses"),
                    "HTTP/1.1",
                    Vec::new(),
                    Some("api.example.test"),
                )
                .unwrap();
            state
                .store
                .write_response(
                    &record.locator,
                    &record.summary,
                    &ResponseMetadata {
                        format_version: crate::traffic_store::FORMAT_VERSION,
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
                    .update_summary(&record.locator, &record.summary, |summary| {
                        summary.protocol.as_mut().unwrap().errors.push(
                            crate::traffic_interpretation::ProtocolDiagnostic {
                                kind: "service_unavailable_error".to_string(),
                                message:
                                    "Our servers are currently overloaded. Please try again later."
                                        .to_string(),
                                at_ns: Some("20".to_string()),
                            },
                        );
                        true
                    })
                    .unwrap();
            }
            state
                .store
                .finish(
                    &record,
                    std::time::Instant::now(),
                    &crate::traffic_store::RuntimeMeasurements::default(),
                    crate::traffic_store::Outcome::Completed,
                    None,
                )
                .unwrap();
            ids.push((record.id, status, provider_error));
        }

        let list = list_records_inner(&state.store, None).unwrap();
        for (id, status, provider_error) in ids {
            let row = list.records.iter().find(|record| record.id == id).unwrap();
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

            let response = record_detail(State(state.clone()), Path(id)).await;
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
    fn record_list_returns_persisted_protocol_without_interpreting_bodies() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (mut record, _) = store
            .begin(
                "POST",
                "/https://example.test/v1/responses",
                Some("https://example.test/v1/responses"),
                "HTTP/1.1",
                Vec::new(),
                Some("example.test"),
            )
            .unwrap();
        record.request_body.write_all(b"not request json").unwrap();
        record
            .response_body
            .write_all(b"not response json")
            .unwrap();
        store
            .update_summary(&record.locator, &record.summary, |summary| {
                summary.protocol.as_mut().unwrap().model.requested =
                    Some("persisted-list-model".to_string());
                true
            })
            .unwrap();

        let list = list_records_inner(&store, None).unwrap();
        let protocol = list.records[0].protocol.as_ref().unwrap();
        assert_eq!(
            protocol.model.requested.as_deref(),
            Some("persisted-list-model")
        );
        assert!(protocol.warnings.is_empty());
    }

    #[test]
    fn record_list_does_not_parse_the_optional_event_timing_index() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin("GET", "/events", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let mut index = store.create_event_index(&record).unwrap();
        writeln!(index, "not json").unwrap();
        index.flush().unwrap();

        let list = list_records_inner(&store, None).unwrap();
        assert_eq!(list.records.len(), 1);
    }

    #[tokio::test]
    async fn record_detail_adds_event_timing_index_diagnostics_on_demand() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path(), 9923).unwrap();
        let (record, _) = state
            .store
            .begin("GET", "/events", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let mut index = state.store.create_event_index(&record).unwrap();
        writeln!(index, "not json").unwrap();
        index.flush().unwrap();

        let response = record_detail(State(state), Path(record.id)).await;
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
    async fn record_detail_ignores_only_an_active_unterminated_event_index_tail() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path(), 9923).unwrap();
        let (record, _) = state
            .store
            .begin("GET", "/events", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let mut index = state.store.create_event_index(&record).unwrap();
        write!(index, "{{\"schema_version\":").unwrap();
        index.flush().unwrap();

        let response = record_detail(State(state.clone()), Path(record.id.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert!(json["summary"]["warnings"].as_array().unwrap().is_empty());

        state.store.abandon_active(&record.id);
        let response = record_detail(State(state), Path(record.id)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let json = response_json(response).await;
        assert_eq!(json["summary"]["warnings"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn detail_response_adds_canonical_reason_without_mutating_raw_metadata() {
        let detail = ResponseDetail::from(ResponseMetadata {
            format_version: crate::traffic_store::FORMAT_VERSION,
            source: ResponseSource::Upstream,
            headers_at: "2026-08-06T04:00:00Z".to_string(),
            status: 200,
            http_version: "HTTP/2".to_string(),
            headers: Vec::new(),
        });
        assert_eq!(detail.reason_phrase.as_deref(), Some("OK"));
        let json = serde_json::to_value(detail).unwrap();
        assert_eq!(json["reason_phrase"], "OK");
        assert_eq!(json["format_version"], 2);
    }

    #[tokio::test]
    async fn detail_response_includes_timeline_and_persisted_protocol_summary() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path(), 9923).unwrap();
        let (mut record, _) = state
            .store
            .begin(
                "POST",
                "/https://example.test/v1/responses",
                Some("https://example.test/v1/responses"),
                "HTTP/1.1",
                Vec::new(),
                Some("example.test"),
            )
            .unwrap();
        record.request_body.write_all(b"not request json").unwrap();
        record
            .response_body
            .write_all(b"not response json")
            .unwrap();
        state
            .store
            .update_summary(&record.locator, &record.summary, |summary| {
                let protocol = summary.protocol.as_mut().unwrap();
                protocol.model.requested = Some("persisted-model".to_string());
                protocol.response_terminal = true;
                true
            })
            .unwrap();
        let id = record.id.clone();
        state
            .store
            .finish(
                &record,
                std::time::Instant::now(),
                &crate::traffic_store::RuntimeMeasurements::default(),
                crate::traffic_store::Outcome::Completed,
                None,
            )
            .unwrap();

        let response = record_detail(State(state), Path(id)).await;
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
        let store = TrafficStore::open(temp.path()).unwrap();
        let id = finished_record(&store, "/body", b"abc\0\xff", b"response");

        for (response_body, offset, expected, length, next_offset) in [
            (false, 2, &b"c\0\xff"[..], "3", "5"),
            (false, 5, &b""[..], "0", "5"),
            (true, 1, &b"esponse"[..], "7", "8"),
        ] {
            let response = body_response(&store, &id, response_body, offset).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_LENGTH], length);
            assert_eq!(
                response.headers()["x-aibox-traffic-next-offset"],
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
        let store = TrafficStore::open(temp.path()).unwrap();
        let identity_id = finished_record(&store, "/identity", b"plain request", b"");
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
        let (mut record, _) = store
            .begin(
                "POST",
                "/zstd",
                None,
                "HTTP/1.1",
                vec![recorded_header("content-encoding", " ZsTd ")],
                None,
            )
            .unwrap();
        record.request_body.write_all(&request_compressed).unwrap();
        record
            .response_body
            .write_all(&response_compressed)
            .unwrap();
        store
            .write_response(
                &record.locator,
                &record.summary,
                &ResponseMetadata {
                    format_version: crate::traffic_store::FORMAT_VERSION,
                    source: ResponseSource::Upstream,
                    headers_at: "2026-08-09T00:00:00Z".to_string(),
                    status: 200,
                    http_version: "HTTP/2".to_string(),
                    headers: vec![recorded_header("content-encoding", "zstd")],
                },
            )
            .unwrap();
        let id = record.id.clone();
        store
            .finish(
                &record,
                std::time::Instant::now(),
                &crate::traffic_store::RuntimeMeasurements::default(),
                crate::traffic_store::Outcome::Completed,
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
        let store = TrafficStore::open(temp.path()).unwrap();
        let (mut active, _) = store
            .begin(
                "POST",
                "/active",
                None,
                "HTTP/1.1",
                vec![recorded_header("content-encoding", "zstd")],
                None,
            )
            .unwrap();
        active.request_body.write_all(b"partial").unwrap();
        let waiting = decoded_body_response(&store, &active.id, false).await;
        assert_eq!(waiting.status(), StatusCode::CONFLICT);

        let (mut unsupported, _) = store
            .begin(
                "POST",
                "/unsupported",
                None,
                "HTTP/1.1",
                vec![recorded_header("content-encoding", "gzip, zstd")],
                None,
            )
            .unwrap();
        unsupported.request_body.write_all(b"encoded").unwrap();
        let unsupported_id = unsupported.id.clone();
        store
            .finish(
                &unsupported,
                std::time::Instant::now(),
                &crate::traffic_store::RuntimeMeasurements::default(),
                crate::traffic_store::Outcome::Rejected,
                None,
            )
            .unwrap();
        let response = decoded_body_response(&store, &unsupported_id, false).await;
        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let (mut corrupt, _) = store
            .begin(
                "POST",
                "/corrupt",
                None,
                "HTTP/1.1",
                vec![recorded_header("content-encoding", "zstd")],
                None,
            )
            .unwrap();
        corrupt.request_body.write_all(b"not zstd").unwrap();
        let corrupt_id = corrupt.id.clone();
        store
            .finish(
                &corrupt,
                std::time::Instant::now(),
                &crate::traffic_store::RuntimeMeasurements::default(),
                crate::traffic_store::Outcome::Rejected,
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
        let state = AppState::for_test(temp.path(), 9923).unwrap();
        let (record, _) = state
            .store
            .begin("GET", "/events", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let mut index = state.store.create_event_index(&record).unwrap();
        for (sequence, completed_at_ns) in [(0, "1000000"), (1, "2500000")] {
            writeln!(
                index,
                "{}",
                json!({
                    "schema_version": crate::traffic_store::FORMAT_VERSION,
                    "record_id": record.id,
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
        let id = record.id.clone();
        state
            .store
            .finish(
                &record,
                std::time::Instant::now(),
                &crate::traffic_store::RuntimeMeasurements::default(),
                crate::traffic_store::Outcome::Completed,
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
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin("GET", "/events", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let mut index = store.create_event_index(&record).unwrap();
        write!(index, "{{\"schema_version\":1").unwrap();
        index.flush().unwrap();

        let timings = store.read_event_timings(&record.id, 0).unwrap();
        assert!(timings.available);
        assert!(!timings.partial);
        assert!(timings.events.is_empty());
    }

    #[tokio::test]
    async fn event_timing_api_reports_a_missing_index_as_unavailable() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path(), 9923).unwrap();
        let id = finished_record(&state.store, "/without-events", b"", b"");

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
        let state = AppState::for_test(temp.path(), 9923).unwrap();
        let (active, _) = state
            .store
            .begin("GET", "/active", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();

        let conflict = delete_records(
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
                &crate::traffic_store::RuntimeMeasurements::default(),
                crate::traffic_store::Outcome::Rejected,
                None,
            )
            .unwrap();
        let stale_count = delete_all(
            State(state.clone()),
            Json(DeleteAllRequest {
                expected_deletable_count: 0,
            }),
        )
        .await;
        assert_eq!(stale_count.status(), StatusCode::CONFLICT);

        let deleted = delete_records(
            State(state.clone()),
            Json(DeleteRequest {
                ids: vec![active.id],
            }),
        )
        .await;
        assert_eq!(deleted.status(), StatusCode::OK);
        assert_eq!(response_json(deleted).await, json!({"deleted": 1}));

        finished_record(&state.store, "/delete-all", b"", b"");
        let deleted = delete_all(
            State(state),
            Json(DeleteAllRequest {
                expected_deletable_count: 1,
            }),
        )
        .await;
        assert_eq!(deleted.status(), StatusCode::OK);
        assert_eq!(response_json(deleted).await, json!({"deleted": 1}));
    }

    #[test]
    fn pagination_is_fixed_at_fifty_and_recomputes_each_page_from_current_order() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        for _ in 0..51 {
            finished_record(&store, "/bad", b"", b"");
        }
        let first = list_records_inner(&store, None).unwrap();
        assert_eq!(first.total, 51);
        assert_eq!(first.records.len(), 50);
        assert!(first.has_next);
        let second = list_records_inner(&store, Some(2)).unwrap();
        assert_eq!(second.records.len(), 1);
        assert!(!second.has_next);

        finished_record(&store, "/new", b"", b"");
        let recomputed_second = list_records_inner(&store, Some(2)).unwrap();
        assert_eq!(recomputed_second.total, 52);
        assert_eq!(recomputed_second.records.len(), 2);
        assert_eq!(recomputed_second.records[1].id, second.records[0].id);
        assert!(list_records_inner(&store, Some(0)).is_err());
        assert!(list_records_inner(&store, Some(u64::MAX)).is_err());
        assert!(
            list_records_inner(&store, Some(3))
                .unwrap()
                .records
                .is_empty()
        );
    }

    #[test]
    fn invalid_page_is_rejected_before_the_store_is_scanned() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        std::fs::remove_dir(store.root()).unwrap();
        std::fs::write(store.root(), b"not a directory").unwrap();

        let error = list_records_inner(&store, Some(0)).err().unwrap();
        assert_eq!(
            error.to_string(),
            "Traffic Record page must be a positive integer"
        );
    }

    #[test]
    fn record_list_uses_terminal_end_order() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (first, _) = store
            .begin("GET", "/first", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let (second, _) = store
            .begin("GET", "/second", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();

        for record in [&second, &first] {
            store
                .finish(
                    record,
                    std::time::Instant::now(),
                    &crate::traffic_store::RuntimeMeasurements::default(),
                    crate::traffic_store::Outcome::Completed,
                    None,
                )
                .unwrap();
        }

        let records = list_records_inner(&store, None).unwrap().records;
        assert_eq!(records[0].id, first.id);
        assert_eq!(records[1].id, second.id);
        assert!(records.iter().all(|record| record.ended_at.is_some()));
    }
}
