use super::*;
use crate::request::{
    ObservedRequest, Outcome, ProtocolDiagnostic, RequestProxyState, RequestStore,
    RuntimeMeasurements,
};
use crate::service::tests::test_state;
use axum::body::Body;
use axum::http::Response;
use axum::response::IntoResponse as _;
use base64::Engine as _;
use http_body_util::BodyExt as _;
use std::io::Write as _;
use uuid::Uuid;

fn inspection(store: &RequestStore) -> RequestInspection {
    RequestInspection::new(store.clone())
}

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
    let list = list_requests_inner(&inspection(&store), None).unwrap();

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
                format_version: crate::request::format_version(),
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
    let responded_summary = list_requests_inner(&inspection(&store), None)
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
    let state = RequestProxyState::for_test(temp.path()).unwrap();
    let mut ids = Vec::new();
    for (status, provider_error) in [(401, false), (200, true)] {
        let (request, _) = state
            .inspection()
            .store()
            .begin(ObservedRequest {
                upstream_url: Some("https://api.example.test/v1/responses"),
                host_hint: Some("api.example.test"),
                ..ObservedRequest::test("POST", "/https://api.example.test/v1/responses")
            })
            .unwrap();
        state
            .inspection()
            .store()
            .write_response(
                &request.locator,
                &request.summary,
                &ResponseMetadata {
                    format_version: crate::request::format_version(),
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
                .inspection()
                .store()
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
            .inspection()
            .store()
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

    let list = list_requests_inner(&state.inspection(), None).unwrap();
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

    let list = list_requests_inner(&inspection(&store), None).unwrap();
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

    let list = list_requests_inner(&inspection(&store), None).unwrap();
    assert_eq!(list.requests.len(), 1);
}

#[tokio::test]
async fn active_durations_do_not_depend_on_the_wall_clock_anchor() {
    let temp = tempfile::tempdir().unwrap();
    let state = RequestProxyState::for_test(temp.path()).unwrap();
    let (request, _) = state
        .inspection()
        .store()
        .begin(ObservedRequest::test("GET", "/active"))
        .unwrap();
    state
        .inspection()
        .store()
        .update_summary(&request.locator, &request.summary, |summary| {
            summary.observed_at = "9999-01-01T00:00:00Z".to_string();
            true
        })
        .unwrap();

    let list = list_requests_inner(&state.inspection(), None).unwrap();
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
    let state = RequestProxyState::for_test(temp.path()).unwrap();
    let (request, _) = state
        .inspection()
        .store()
        .begin(ObservedRequest::test("GET", "/events"))
        .unwrap();
    let mut index = state
        .inspection()
        .store()
        .create_event_index(&request)
        .unwrap();
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
    let state = RequestProxyState::for_test(temp.path()).unwrap();
    let (request, _) = state
        .inspection()
        .store()
        .begin(ObservedRequest::test("GET", "/events"))
        .unwrap();
    let mut index = state
        .inspection()
        .store()
        .create_event_index(&request)
        .unwrap();
    write!(index, "{{\"schema_version\":").unwrap();
    index.flush().unwrap();

    let response = request_detail(State(state.clone()), Path(request.id.clone())).await;
    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert!(json["summary"]["warnings"].as_array().unwrap().is_empty());

    state.inspection().store().abandon_active(&request.id);
    let response = request_detail(State(state), Path(request.id)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let json = response_json(response).await;
    assert_eq!(json["summary"]["warnings"].as_array().unwrap().len(), 1);
}

#[test]
fn detail_response_adds_canonical_reason_without_mutating_raw_metadata() {
    let detail = ResponseDetail::from(ResponseMetadata {
        format_version: crate::request::format_version(),
        source: ResponseSource::Upstream,
        headers_at: "2026-08-06T04:00:00Z".to_string(),
        status: 200,
        http_version: "HTTP/2".to_string(),
        headers: Vec::new(),
    });
    assert_eq!(detail.reason_phrase.as_deref(), Some("OK"));
    let json = serde_json::to_value(detail).unwrap();
    assert_eq!(json["reason_phrase"], "OK");
    assert_eq!(json["format_version"], crate::request::format_version());
}

#[tokio::test]
async fn detail_response_includes_timeline_and_persisted_protocol_summary() {
    let temp = tempfile::tempdir().unwrap();
    let state = RequestProxyState::for_test(temp.path()).unwrap();
    let (mut request, _) = state
        .inspection()
        .store()
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
        .inspection()
        .store()
        .update_summary(&request.locator, &request.summary, |summary| {
            let protocol = summary.protocol.as_mut().unwrap();
            protocol.model.requested = Some("persisted-model".to_string());
            protocol.response_terminal = true;
            true
        })
        .unwrap();
    let id = request.id.clone();
    state
        .inspection()
        .store()
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
        let response = body_response(inspection(&store), &id, response_body, offset).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_LENGTH], length);
        assert_eq!(
            response.headers()["x-aibox-request-next-offset"],
            next_offset
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(body.as_ref(), expected);
    }

    let invalid_range = body_response(inspection(&store), &id, false, 6).await;
    assert_eq!(invalid_range.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    let missing = body_response(inspection(&store), &Uuid::now_v7().to_string(), false, 0).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn decoded_body_api_handles_identity_zstd_and_gzip_without_changing_raw_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let identity_id = finished_request(&store, "/identity", b"plain request", b"");
    let identity = decoded_body_response(inspection(&store), &identity_id, false).await;
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
                format_version: crate::request::format_version(),
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
        let decoded = decoded_body_response(inspection(&store), &id, response_body).await;
        assert_eq!(decoded.status(), StatusCode::OK);
        assert_eq!(
            decoded.into_body().collect().await.unwrap().to_bytes(),
            expected
        );
        let raw = body_response(inspection(&store), &id, response_body, 0).await;
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

    use flate2::Compression;
    use flate2::write::GzEncoder;
    let gzip_source = br#"{"result":"gzip-response"}"#;
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(gzip_source).unwrap();
    let gzip_compressed = encoder.finish().unwrap();
    let (mut gzip_request, _) = store.begin(ObservedRequest::test("POST", "/gzip")).unwrap();
    gzip_request.request_body.write_all(b"{}").unwrap();
    gzip_request
        .response_body
        .write_all(&gzip_compressed)
        .unwrap();
    store
        .write_response(
            &gzip_request.locator,
            &gzip_request.summary,
            &ResponseMetadata {
                format_version: crate::request::format_version(),
                source: ResponseSource::Upstream,
                headers_at: "2026-08-09T00:00:00Z".to_string(),
                status: 200,
                http_version: "HTTP/2".to_string(),
                headers: vec![recorded_header("content-encoding", "gzip")],
            },
        )
        .unwrap();
    let gzip_id = gzip_request.id.clone();
    store
        .finish(
            &gzip_request,
            std::time::Instant::now(),
            &RuntimeMeasurements::default(),
            Outcome::Completed,
            None,
        )
        .unwrap();
    let gzip_decoded = decoded_body_response(inspection(&store), &gzip_id, true).await;
    assert_eq!(gzip_decoded.status(), StatusCode::OK);
    assert_eq!(
        gzip_decoded.into_body().collect().await.unwrap().to_bytes(),
        gzip_source.as_slice()
    );
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
    let waiting = decoded_body_response(inspection(&store), &active.id, false).await;
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
    let response = decoded_body_response(inspection(&store), &unsupported_id, false).await;
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
    let response = decoded_body_response(inspection(&store), &corrupt_id, false).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.into_body().collect().await.is_err());
}

#[tokio::test]
async fn event_timing_api_returns_incremental_valid_entries_and_partial_state() {
    let temp = tempfile::tempdir().unwrap();
    let state = RequestProxyState::for_test(temp.path()).unwrap();
    let (request, _) = state
        .inspection()
        .store()
        .begin(ObservedRequest::test("GET", "/events"))
        .unwrap();
    let mut index = state
        .inspection()
        .store()
        .create_event_index(&request)
        .unwrap();
    for (sequence, completed_at_ns) in [(0, "1000000"), (1, "2500000")] {
        writeln!(
            index,
            "{}",
            json!({
                "schema_version": crate::request::format_version(),
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
        .inspection()
        .store()
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
    let state = RequestProxyState::for_test(temp.path()).unwrap();
    let id = finished_request(&state.inspection().store(), "/without-events", b"", b"");

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

/// Deletion goes through `RequestCoordinator`, so it takes `ServiceState` and
/// the shared management gate rather than `RequestProxyState` directly.
#[tokio::test]
async fn deletion_api_maps_selection_conflicts_and_successes() {
    let temp = tempfile::tempdir().unwrap();
    let state = test_state(temp.path());
    let store = state.request().inspection().store();
    let (active, _) = store
        .begin(ObservedRequest::test("GET", "/active"))
        .unwrap();

    let conflict = delete_requests(
        State(state.clone()),
        Json(DeleteRequest {
            ids: vec![active.id.clone()],
        }),
    )
    .await
    .into_response();
    assert_eq!(conflict.status(), StatusCode::CONFLICT);

    store
        .finish(
            &active,
            std::time::Instant::now(),
            &RuntimeMeasurements::default(),
            Outcome::Rejected,
            None,
        )
        .unwrap();
    let second = finished_request(&store, "/delete-selected", b"", b"");
    let deleted = delete_requests(
        State(state),
        Json(DeleteRequest {
            ids: vec![active.id, second],
        }),
    )
    .await
    .into_response();
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
    let first = list_requests_inner(&inspection(&store), None).unwrap();
    assert_eq!(first.total, 51);
    assert_eq!(first.requests.len(), 50);
    assert!(first.has_next);
    let second = list_requests_inner(&inspection(&store), Some(2)).unwrap();
    assert_eq!(second.requests.len(), 1);
    assert!(!second.has_next);

    finished_request(&store, "/new", b"", b"");
    let recomputed_second = list_requests_inner(&inspection(&store), Some(2)).unwrap();
    assert_eq!(recomputed_second.total, 52);
    assert_eq!(recomputed_second.requests.len(), 2);
    assert_eq!(recomputed_second.requests[1].id, second.requests[0].id);
    assert!(
        list_requests_inner(&inspection(&store), Some(3))
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
        let error = list_requests_inner(&inspection(&store), Some(page))
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

    let requests = list_requests_inner(&inspection(&store), None)
        .unwrap()
        .requests;
    assert_eq!(requests[0].id, first.id);
    assert_eq!(requests[1].id, second.id);
    assert!(requests.iter().all(|request| request.ended_at.is_some()));
}
