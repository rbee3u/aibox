use crate::traffic::AppState;
use crate::traffic_proxy;
use crate::traffic_store::{StoredRecord, TrafficStore};
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
    request: crate::traffic_store::RequestMetadata,
    response: Option<crate::traffic_store::ResponseMetadata>,
    result: Option<crate::traffic_store::ResultMetadata>,
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

    async fn response_json(response: Response<Body>) -> serde_json::Value {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

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
    fn record_summaries_distinguish_active_interrupted_and_completed_state() {
        let temp = tempfile::tempdir().unwrap();
        let first_process = TrafficStore::open(temp.path()).unwrap();
        let (interrupted, _) = first_process
            .begin("GET", "/interrupted", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let interrupted_id = interrupted.id.clone();
        drop(first_process);

        let store = TrafficStore::open(temp.path()).unwrap();
        let completed_id = finished_record(&store, "/completed", b"", b"");
        let (active, _) = store
            .begin("GET", "/active", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let list = list_records_inner(&store, None).unwrap();

        assert_eq!(list.total, 3);
        assert_eq!(list.deletable_count, 2);
        for (id, state, outcome) in [
            (active.id.as_str(), "active", "active"),
            (completed_id.as_str(), "completed", "rejected"),
            (interrupted_id.as_str(), "interrupted", "interrupted"),
        ] {
            let record = list.records.iter().find(|record| record.id == id).unwrap();
            assert_eq!(
                (record.state.as_str(), record.outcome.as_str()),
                (state, outcome)
            );
        }
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
    fn pagination_is_fixed_at_fifty_and_cursor_is_stable_when_new_records_arrive() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        for _ in 0..51 {
            finished_record(&store, "/bad", b"", b"");
        }
        let first = list_records_inner(&store, None).unwrap();
        assert_eq!(first.total, 51);
        assert_eq!(first.records.len(), 50);
        let cursor = first.next_cursor.unwrap();
        let second = list_records_inner(&store, Some(&cursor)).unwrap();
        assert_eq!(second.records.len(), 1);
        assert!(second.next_cursor.is_none());

        finished_record(&store, "/new", b"", b"");
        let stable_second = list_records_inner(&store, Some(&cursor)).unwrap();
        assert_eq!(stable_second.total, 52);
        assert_eq!(stable_second.records.len(), 1);
        assert_eq!(stable_second.records[0].id, second.records[0].id);
    }
}
