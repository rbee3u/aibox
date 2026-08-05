use super::proxy;
use super::store::{StoredRecord, TrafficStore};
use super::AppState;
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, Query, Request, State};
use axum::http::{header, HeaderValue, Method, Response, StatusCode};
use axum::middleware::Next;
use axum::Json;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use tokio::io::AsyncReadExt as _;
use tokio_util::io::ReaderStream;

const PAGE_SIZE: usize = 50;
const HTML: &str = include_str!("../../assets/traffic.html");
const CSS: &str = include_str!("../../assets/traffic.css");
const JS: &str = include_str!("../../assets/traffic.js");

pub(super) async fn security_middleware(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response<Body> {
    if let Err(message) = validate_management_request(&state, peer, &request) {
        return secure_response(proxy::bare_error(StatusCode::FORBIDDEN, &message));
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
    let expected = format!("127.0.0.1:{}", state.port);
    let allowed_host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|host| {
            host.eq_ignore_ascii_case(&expected)
                || host.eq_ignore_ascii_case(&format!("localhost:{}", state.port))
                || host.eq_ignore_ascii_case(&format!("[::1]:{}", state.port))
        });
    if !allowed_host {
        return Err("invalid Host for the Traffic management interface".to_string());
    }
    if let Some(site) = request
        .headers()
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
    {
        if !matches!(site, "none" | "same-origin") {
            return Err("cross-site management requests are not accepted".to_string());
        }
    }
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let expected_origin = format!("http://{expected}");
    if origin.is_some_and(|origin| !origin.eq_ignore_ascii_case(&expected_origin)) {
        return Err("invalid Origin for the Traffic management interface".to_string());
    }
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        if origin != Some(expected_origin.as_str()) {
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
    proxy::bare_error(StatusCode::NOT_FOUND, "Traffic management route not found")
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
    cursor: Option<String>,
}

#[derive(Serialize)]
struct RecordSummary {
    id: String,
    started_at: String,
    method: String,
    incoming_uri: String,
    upstream_url: Option<String>,
    status: Option<u16>,
    outcome: String,
    state: String,
    total_ms: Option<u64>,
}

#[derive(Serialize)]
struct RecordList {
    records: Vec<RecordSummary>,
    total: usize,
    deletable_count: usize,
    next_cursor: Option<String>,
}

pub(super) async fn list_records(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Response<Body> {
    match list_records_inner(&state.store, query.cursor.as_deref()) {
        Ok(value) => json_response(StatusCode::OK, &value),
        Err(error) => json_error(StatusCode::BAD_REQUEST, &error.to_string()),
    }
}

fn list_records_inner(store: &TrafficStore, cursor: Option<&str>) -> anyhow::Result<RecordList> {
    let records = store.scan()?;
    let total = records.len();
    let deletable_count = records.iter().filter(|record| !record.active).count();
    let cursor = cursor.map(decode_cursor).transpose()?;
    let mut eligible: Vec<_> = records
        .iter()
        .filter(|record| {
            cursor.as_ref().is_none_or(|(started, id)| {
                (&record.request.started_at, &record.request.id) < (started, id)
            })
        })
        .collect();
    let has_more = eligible.len() > PAGE_SIZE;
    eligible.truncate(PAGE_SIZE);
    let next_cursor = if has_more {
        eligible
            .last()
            .map(|record| encode_cursor(&record.request.started_at, &record.request.id))
    } else {
        None
    };
    let records = eligible.into_iter().map(summary).collect();
    Ok(RecordList {
        records,
        total,
        deletable_count,
        next_cursor,
    })
}

fn summary(record: &StoredRecord) -> RecordSummary {
    let (state, outcome) = if record.active {
        ("active", "active")
    } else if let Some(result) = &record.result {
        ("completed", result.outcome.as_str())
    } else {
        ("interrupted", "interrupted")
    };
    RecordSummary {
        id: record.request.id.clone(),
        started_at: record.request.started_at.clone(),
        method: record.request.method.clone(),
        incoming_uri: record.request.incoming_uri.clone(),
        upstream_url: record.request.upstream_url.clone(),
        status: record.response.as_ref().map(|response| response.status),
        outcome: outcome.to_string(),
        state: state.to_string(),
        total_ms: record.result.as_ref().map(|result| result.total_ms),
    }
}

#[derive(Serialize)]
struct RecordDetail {
    request: super::store::RequestMetadata,
    response: Option<super::store::ResponseMetadata>,
    result: Option<super::store::ResultMetadata>,
    state: String,
    request_body_bytes: u64,
    response_body_bytes: u64,
    live_ttfb_ms: Option<u64>,
    live_total_ms: Option<u64>,
}

pub(super) async fn record_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response<Body> {
    match state.store.find(&id) {
        Ok(record) => {
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
            let live_ttfb_ms = record.response.as_ref().and_then(|response| {
                elapsed_wall_ms(&record.request.started_at, Some(&response.headers_at))
            });
            json_response(
                StatusCode::OK,
                &RecordDetail {
                    request: record.request,
                    response: record.response,
                    result: record.result,
                    state: state_name.to_string(),
                    request_body_bytes: record.request_body_bytes,
                    response_body_bytes: record.response_body_bytes,
                    live_ttfb_ms,
                    live_total_ms,
                },
            )
        }
        Err(error) => json_error(StatusCode::NOT_FOUND, &error.to_string()),
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

async fn body_response(
    store: &TrafficStore,
    id: &str,
    response: bool,
    offset: u64,
) -> Response<Body> {
    let (file, length) = match store.open_body(id, response, offset) {
        Ok(value) => value,
        Err(error) if error.to_string().contains("exceeds current length") => {
            return json_error(StatusCode::RANGE_NOT_SATISFIABLE, &error.to_string());
        }
        Err(error) => return json_error(StatusCode::NOT_FOUND, &error.to_string()),
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

#[derive(Deserialize)]
pub(super) struct DeleteRequest {
    ids: Vec<String>,
}

pub(super) async fn delete_records(
    State(state): State<AppState>,
    Json(request): Json<DeleteRequest>,
) -> Response<Body> {
    match state.store.delete_ids(&request.ids) {
        Ok(deleted) => json_response(StatusCode::OK, &json!({"deleted": deleted})),
        Err(error) => {
            let status = if error.to_string().contains("active Traffic") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            json_error(status, &error.to_string())
        }
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
    match state.store.delete_all(request.expected_deletable_count) {
        Ok(deleted) => json_response(StatusCode::OK, &json!({"deleted": deleted})),
        Err(error) if error.to_string().contains("count changed") => {
            json_error(StatusCode::CONFLICT, &error.to_string())
        }
        Err(error) => json_error(StatusCode::BAD_REQUEST, &error.to_string()),
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

fn encode_cursor(started: &str, id: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&(started, id)).expect("cursor strings serialize"))
}

fn decode_cursor(cursor: &str) -> anyhow::Result<(String, String)> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| anyhow::anyhow!("invalid Traffic Record cursor"))?;
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid Traffic Record cursor"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[test]
    fn cursors_are_opaque_and_round_trip() {
        let cursor = encode_cursor("2026-08-05T12:00:00Z", "id");
        assert!(!cursor.contains("2026"));
        assert_eq!(
            decode_cursor(&cursor).unwrap(),
            ("2026-08-05T12:00:00Z".to_string(), "id".to_string())
        );
        assert!(decode_cursor("not-a-cursor").is_err());
    }

    #[test]
    fn management_security_requires_loopback_host_origin_and_csrf() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path(), 9923).unwrap();
        let peer: SocketAddr = "127.0.0.1:40000".parse().unwrap();
        let request = Request::builder()
            .method("POST")
            .uri("/_aibox/traffic/api/records/delete")
            .header("host", "127.0.0.1:9923")
            .header("origin", "http://127.0.0.1:9923")
            .header("sec-fetch-site", "same-origin")
            .header("x-aibox-traffic-csrf", &state.csrf)
            .body(Body::empty())
            .unwrap();
        assert!(validate_management_request(&state, peer, &request).is_ok());
        let remote = "192.0.2.1:40000".parse().unwrap();
        assert!(validate_management_request(&state, remote, &request).is_err());
        let bad = Request::builder()
            .method("POST")
            .uri("/_aibox/traffic/api/records/delete")
            .header("host", "127.0.0.1:9923")
            .header("origin", "http://evil.example")
            .body(Body::empty())
            .unwrap();
        assert!(validate_management_request(&state, peer, &bad).is_err());
    }

    #[test]
    fn pagination_is_fixed_at_fifty_and_cursor_is_stable_when_new_records_arrive() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        for _ in 0..51 {
            let (record, _) = store
                .begin("GET", "/bad", None, "HTTP/1.1", Vec::new(), None)
                .unwrap();
            store
                .finish(
                    &record,
                    std::time::Instant::now(),
                    &super::super::store::RuntimeMeasurements::default(),
                    super::super::store::Outcome::Rejected,
                    None,
                )
                .unwrap();
        }
        let first = list_records_inner(&store, None).unwrap();
        assert_eq!(first.total, 51);
        assert_eq!(first.records.len(), 50);
        let cursor = first.next_cursor.unwrap();
        let second = list_records_inner(&store, Some(&cursor)).unwrap();
        assert_eq!(second.records.len(), 1);
        assert!(second.next_cursor.is_none());

        let (newest, _) = store
            .begin("GET", "/new", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        store
            .finish(
                &newest,
                std::time::Instant::now(),
                &super::super::store::RuntimeMeasurements::default(),
                super::super::store::Outcome::Rejected,
                None,
            )
            .unwrap();
        let stable_second = list_records_inner(&store, Some(&cursor)).unwrap();
        assert_eq!(stable_second.total, 52);
        assert_eq!(stable_second.records.len(), 1);
        assert_eq!(stable_second.records[0].id, second.records[0].id);
    }
}
