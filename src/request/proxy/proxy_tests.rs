use super::capture::*;
use super::request_stream::*;
use super::response_stream::*;
use super::target::*;
use super::*;
use crate::request::model::{
    AssessmentLevel, ErrorMetadata, ProtocolFamily, ProtocolSummary, RecordedHeader,
    ResponseModeValue, SummaryMetadata, TimingMetadata,
};
use crate::request::sse::{PrefixSniff, SseIndexer, SsePrefixSniffer, is_first_token_data};
use crate::request::store::{RequestStore, SummaryHandle};
use axum::http::{HeaderMap, Method, header};
use base64::Engine as _;
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::BodyExt as _;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

struct FakeUpstreamSender {
    result: Mutex<Option<FakeUpstreamResult>>,
}

enum FakeUpstreamResult {
    Response(reqwest::Response),
    ConnectError(UpstreamConnectError),
    SendError(UpstreamSendError),
}

impl FakeUpstreamSender {
    fn new(response: reqwest::Response) -> Self {
        Self {
            result: Mutex::new(Some(FakeUpstreamResult::Response(response))),
        }
    }

    fn connect_error(error: UpstreamConnectError) -> Self {
        Self {
            result: Mutex::new(Some(FakeUpstreamResult::ConnectError(error))),
        }
    }

    fn send_error(error: UpstreamSendError) -> Self {
        Self {
            result: Mutex::new(Some(FakeUpstreamResult::SendError(error))),
        }
    }
}

impl UpstreamSender for FakeUpstreamSender {
    type Connection = ();

    fn connect(
        &self,
        _url: &Url,
    ) -> UpstreamFuture<'_, Result<Self::Connection, UpstreamConnectError>> {
        let mut result = self.result.lock().expect("fake upstream result poisoned");
        if matches!(result.as_ref(), Some(FakeUpstreamResult::ConnectError(_))) {
            let Some(FakeUpstreamResult::ConnectError(error)) = result.take() else {
                unreachable!()
            };
            Box::pin(async move { Err(error) })
        } else {
            Box::pin(async { Ok(()) })
        }
    }

    fn send(
        &self,
        _connection: Self::Connection,
        request: UpstreamRequest,
    ) -> UpstreamFuture<'_, Result<reqwest::Response, UpstreamSendError>> {
        let result = self
            .result
            .lock()
            .expect("fake upstream result poisoned")
            .take()
            .expect("fake upstream result already used");
        Box::pin(async move {
            request
                .body
                .collect()
                .await
                .map_err(|error| UpstreamSendError {
                    message: error.to_string(),
                    timeout: false,
                })?;
            match result {
                FakeUpstreamResult::Response(response) => Ok(response),
                FakeUpstreamResult::SendError(error) => Err(error),
                FakeUpstreamResult::ConnectError(_) => {
                    unreachable!("connect errors cannot reach send")
                }
            }
        })
    }
}

struct RetrySender {
    statuses: Mutex<VecDeque<StatusCode>>,
    calls: Arc<AtomicUsize>,
    bodies: Arc<Mutex<Vec<Vec<u8>>>>,
    return_first_without_reading_body: bool,
}

impl RetrySender {
    fn new(statuses: impl IntoIterator<Item = StatusCode>, early_first: bool) -> Self {
        Self {
            statuses: Mutex::new(statuses.into_iter().collect()),
            calls: Arc::new(AtomicUsize::new(0)),
            bodies: Arc::new(Mutex::new(Vec::new())),
            return_first_without_reading_body: early_first,
        }
    }
}

impl UpstreamSender for RetrySender {
    type Connection = ();

    fn connect(
        &self,
        _url: &Url,
    ) -> UpstreamFuture<'_, Result<Self::Connection, UpstreamConnectError>> {
        Box::pin(async { Ok(()) })
    }

    fn send(
        &self,
        _connection: Self::Connection,
        request: UpstreamRequest,
    ) -> UpstreamFuture<'_, Result<reqwest::Response, UpstreamSendError>> {
        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        let status = self
            .statuses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(StatusCode::TOO_MANY_REQUESTS);
        let bodies = self.bodies.clone();
        let early = self.return_first_without_reading_body && index == 0;
        Box::pin(async move {
            if !early {
                let body = request
                    .body
                    .collect()
                    .await
                    .map_err(|error| UpstreamSendError {
                        message: error.to_string(),
                        timeout: false,
                    })?;
                bodies.lock().unwrap().push(body.to_bytes().to_vec());
            }
            let mut response = upstream_response(
                status,
                Some("text/plain"),
                reqwest::Body::from(format!("attempt {index}")),
            );
            if status == StatusCode::TOO_MANY_REQUESTS {
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, "9999".parse().unwrap());
            }
            Ok(response)
        })
    }
}

fn retry_state(root: &std::path::Path, shutdown: CancellationToken) -> RequestProxyState {
    std::fs::write(root.join("retry_urls.txt"), "https://relay.example/v1\n").unwrap();
    RequestProxyState::new(root, shutdown).unwrap()
}

fn upstream_response(
    status: StatusCode,
    content_type: Option<&'static str>,
    body: reqwest::Body,
) -> reqwest::Response {
    let mut builder = Response::builder().status(status);
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    reqwest::Response::from(builder.body(body).unwrap())
}

fn proxy_request(target: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(format!("/{target}"))
        .header(header::CONTENT_LENGTH, "0")
        .body(Body::empty())
        .unwrap()
}

async fn finish_response_tasks(state: &RequestProxyState) {
    state.response_tasks.close();
    state.response_tasks.wait().await;
}

fn single_outcome(state: &RequestProxyState) -> Outcome {
    state
        .store
        .scan()
        .unwrap()
        .remove(0)
        .result
        .expect("Request should be terminal")
        .outcome
}

#[tokio::test(start_paused = true)]
async fn matching_429_replays_complete_post_body_and_records_recovery() {
    let root = tempfile::tempdir().unwrap();
    let state = retry_state(root.path(), CancellationToken::new());
    let sender = RetrySender::new([StatusCode::TOO_MANY_REQUESTS, StatusCode::OK], true);
    let request = Request::builder()
        .method(Method::POST)
        .uri("/https://relay.example/v1/responses?stream=true")
        .header(header::CONTENT_LENGTH, "6")
        .body(Body::from("prompt"))
        .unwrap();

    let response = handle_with_sender(state.clone(), request, &sender).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        Bytes::from_static(b"attempt 1")
    );
    finish_response_tasks(&state).await;
    assert_eq!(sender.calls.load(Ordering::SeqCst), 2);
    assert_eq!(*sender.bodies.lock().unwrap(), [b"prompt".to_vec()]);
    let stored = state.store.scan().unwrap();
    let stored = crate::testutil::only(&stored);
    assert_eq!(
        std::fs::read(stored.directory.join("request.body")).unwrap(),
        b"prompt"
    );
    assert_eq!(
        std::fs::read(stored.directory.join("response.body")).unwrap(),
        b"attempt 1"
    );
    assert_eq!(stored.response.as_ref().unwrap().status, 200);
    assert_eq!(stored.summary.retry.as_ref().unwrap().retry_count, 1);
    assert_eq!(stored.summary.assessment.level, AssessmentLevel::Warning);
    let timing = &stored.summary.timing;
    assert!(
        timing
            .upstream_request_started_at_ns
            .as_ref()
            .unwrap()
            .parse::<u128>()
            .unwrap()
            <= timing
                .upstream_request_body_completed_at_ns
                .as_ref()
                .unwrap()
                .parse::<u128>()
                .unwrap()
    );
}

#[tokio::test(start_paused = true)]
async fn matching_429_stops_at_retry_deadline_and_forwards_last_response() {
    let root = tempfile::tempdir().unwrap();
    let state = retry_state(root.path(), CancellationToken::new());
    let sender = RetrySender::new([], false);

    let response = handle_with_sender(
        state.clone(),
        proxy_request("https://relay.example/v1/responses"),
        &sender,
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers().get(header::RETRY_AFTER).unwrap(), "9999");
    let body = response.into_body().collect().await.unwrap().to_bytes();
    finish_response_tasks(&state).await;
    let calls = sender.calls.load(Ordering::SeqCst);
    assert!(calls > 1 && calls <= 60, "{calls}");
    assert_eq!(body, Bytes::from(format!("attempt {}", calls - 1)));
    let stored = state.store.scan().unwrap();
    let stored = crate::testutil::only(&stored);
    assert_eq!(
        stored.summary.retry.as_ref().unwrap().retry_count as usize,
        calls - 1
    );
    assert_eq!(stored.response.as_ref().unwrap().status, 429);
    assert_eq!(stored.summary.assessment.level, AssessmentLevel::Error);
}

#[tokio::test]
async fn unmatched_429_is_forwarded_without_retry() {
    let root = tempfile::tempdir().unwrap();
    let state = retry_state(root.path(), CancellationToken::new());
    let sender = RetrySender::new([StatusCode::TOO_MANY_REQUESTS], false);
    let response = handle_with_sender(
        state.clone(),
        proxy_request("https://other.example/v1/responses"),
        &sender,
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    response.into_body().collect().await.unwrap();
    finish_response_tasks(&state).await;
    assert_eq!(sender.calls.load(Ordering::SeqCst), 1);
    assert!(state.store.scan().unwrap()[0].summary.retry.is_none());
}

#[tokio::test(start_paused = true)]
async fn shutdown_during_retry_wait_stops_without_another_send() {
    let root = tempfile::tempdir().unwrap();
    let shutdown = CancellationToken::new();
    let state = retry_state(root.path(), shutdown.clone());
    let sender = Arc::new(RetrySender::new([], true));
    let task_state = state.clone();
    let task_sender = sender.clone();
    let task = tokio::spawn(async move {
        handle_with_sender(
            task_state,
            proxy_request("https://relay.example/v1/responses"),
            task_sender.as_ref(),
        )
        .await
    });
    while state
        .store
        .scan()
        .unwrap()
        .first()
        .and_then(|request| request.summary.retry.as_ref())
        .is_none()
    {
        tokio::task::yield_now().await;
    }
    shutdown.cancel();
    let response = task.await.unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(sender.calls.load(Ordering::SeqCst), 1);
    assert_eq!(single_outcome(&state), Outcome::ServerShutdown);
}

#[tokio::test]
async fn injected_sender_runs_normal_response_through_handle_without_a_socket() {
    let temp = tempfile::tempdir().unwrap();
    let state = RequestProxyState::new(temp.path(), CancellationToken::new()).unwrap();
    let sender = FakeUpstreamSender::new(upstream_response(
        StatusCode::OK,
        Some("application/json"),
        reqwest::Body::from(r#"{"ok":true}"#),
    ));

    let response = handle_with_sender(
        state.clone(),
        proxy_request("https://example.com/v1/health"),
        &sender,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        Bytes::from_static(br#"{"ok":true}"#)
    );
    finish_response_tasks(&state).await;
    assert_eq!(single_outcome(&state), Outcome::Completed);
}

#[derive(Clone, Copy, Debug)]
enum ProxyFailureScenario {
    InvalidTarget,
    Upgrade,
    InvalidResolvedTarget,
    Dns,
    ClientConfiguration,
    ConnectTimeout,
    SendFailure,
}

#[tokio::test]
async fn proxy_failures_preserve_status_outcome_and_error_kind_without_a_socket() {
    for (scenario, status, outcome, kind) in [
        (
            ProxyFailureScenario::InvalidTarget,
            StatusCode::BAD_REQUEST,
            Outcome::Rejected,
            ErrorKind::InvalidTargetUrl,
        ),
        (
            ProxyFailureScenario::Upgrade,
            StatusCode::UPGRADE_REQUIRED,
            Outcome::Rejected,
            ErrorKind::UpgradeNotSupported,
        ),
        (
            ProxyFailureScenario::InvalidResolvedTarget,
            StatusCode::BAD_REQUEST,
            Outcome::Rejected,
            ErrorKind::InvalidTargetUrl,
        ),
        (
            ProxyFailureScenario::Dns,
            StatusCode::BAD_GATEWAY,
            Outcome::UpstreamError,
            ErrorKind::DnsError,
        ),
        (
            ProxyFailureScenario::ClientConfiguration,
            StatusCode::BAD_GATEWAY,
            Outcome::UpstreamError,
            ErrorKind::ClientConfiguration,
        ),
        (
            ProxyFailureScenario::ConnectTimeout,
            StatusCode::GATEWAY_TIMEOUT,
            Outcome::UpstreamError,
            ErrorKind::ConnectTimeout,
        ),
        (
            ProxyFailureScenario::SendFailure,
            StatusCode::BAD_GATEWAY,
            Outcome::UpstreamError,
            ErrorKind::UpstreamRequestFailed,
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let state = RequestProxyState::new(temp.path(), CancellationToken::new()).unwrap();
        let mut request = proxy_request(match scenario {
            ProxyFailureScenario::InvalidTarget => "ftp://example.com/v1",
            _ => "https://example.com/v1",
        });
        if matches!(scenario, ProxyFailureScenario::Upgrade) {
            request
                .headers_mut()
                .insert(header::UPGRADE, "websocket".parse().unwrap());
        }
        let sender = match scenario {
            ProxyFailureScenario::InvalidTarget | ProxyFailureScenario::Upgrade => {
                FakeUpstreamSender::new(upstream_response(
                    StatusCode::OK,
                    None,
                    reqwest::Body::from("unused"),
                ))
            }
            ProxyFailureScenario::InvalidResolvedTarget => FakeUpstreamSender::connect_error(
                UpstreamConnectError::InvalidTarget("invalid resolved target".to_string()),
            ),
            ProxyFailureScenario::Dns => FakeUpstreamSender::connect_error(
                UpstreamConnectError::Dns("resolution failed".to_string()),
            ),
            ProxyFailureScenario::ClientConfiguration => FakeUpstreamSender::connect_error(
                UpstreamConnectError::ClientConfiguration("invalid TLS settings".to_string()),
            ),
            ProxyFailureScenario::ConnectTimeout => {
                FakeUpstreamSender::send_error(UpstreamSendError {
                    message: "connection timed out".to_string(),
                    timeout: true,
                })
            }
            ProxyFailureScenario::SendFailure => {
                FakeUpstreamSender::send_error(UpstreamSendError {
                    message: "connection refused".to_string(),
                    timeout: false,
                })
            }
        };

        let response = handle_with_sender(state.clone(), request, &sender).await;
        assert_eq!(response.status(), status, "{scenario:?}");
        response.into_body().collect().await.unwrap();
        finish_response_tasks(&state).await;
        let stored = state.store.scan().unwrap();
        let result = crate::testutil::only(&stored).result.as_ref().unwrap();
        assert_eq!(result.outcome, outcome, "{scenario:?}");
        assert_eq!(result.error.as_ref().unwrap().kind, kind, "{scenario:?}");
    }
}

#[tokio::test]
async fn upstream_error_response_passes_through_and_is_recorded_without_a_socket() {
    let temp = tempfile::tempdir().unwrap();
    let state = RequestProxyState::new(temp.path(), CancellationToken::new()).unwrap();
    let sender = FakeUpstreamSender::new(upstream_response(
        StatusCode::SERVICE_UNAVAILABLE,
        Some("text/plain"),
        reqwest::Body::from("upstream overloaded"),
    ));

    let response = handle_with_sender(
        state.clone(),
        proxy_request("https://example.com/v1"),
        &sender,
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        Bytes::from_static(b"upstream overloaded")
    );
    finish_response_tasks(&state).await;
    let stored = state.store.scan().unwrap();
    let stored = crate::testutil::only(&stored);
    assert_eq!(stored.response.as_ref().unwrap().status, 503);
    assert_eq!(stored.result.as_ref().unwrap().outcome, Outcome::Completed);
    assert!(stored.result.as_ref().unwrap().error.is_none());
    assert_eq!(
        std::fs::read(stored.directory.join("response.body")).unwrap(),
        b"upstream overloaded"
    );
}

#[tokio::test]
async fn injected_sender_runs_terminal_sse_through_handle_without_a_socket() {
    let temp = tempfile::tempdir().unwrap();
    let state = RequestProxyState::new(temp.path(), CancellationToken::new()).unwrap();
    let sender = FakeUpstreamSender::new(upstream_response(
        StatusCode::OK,
        Some("text/event-stream"),
        reqwest::Body::from("data: [DONE]\n\n"),
    ));

    let response = handle_with_sender(
        state.clone(),
        proxy_request("https://example.com/v1/chat/completions"),
        &sender,
    )
    .await;

    assert_eq!(
        response.into_body().collect().await.unwrap().to_bytes(),
        Bytes::from_static(b"data: [DONE]\n\n")
    );
    finish_response_tasks(&state).await;
    assert_eq!(single_outcome(&state), Outcome::Completed);
}

#[tokio::test]
async fn injected_sender_records_client_disconnect_from_handle_without_a_socket() {
    let temp = tempfile::tempdir().unwrap();
    let state = RequestProxyState::new(temp.path(), CancellationToken::new()).unwrap();
    let (_upstream_sender, upstream_receiver) = mpsc::channel::<Result<Bytes, io::Error>>(1);
    let sender = FakeUpstreamSender::new(upstream_response(
        StatusCode::OK,
        Some("application/octet-stream"),
        reqwest::Body::wrap_stream(ReceiverStream::new(upstream_receiver)),
    ));

    let response = handle_with_sender(
        state.clone(),
        proxy_request("https://example.com/v1/stream"),
        &sender,
    )
    .await;
    drop(response);

    finish_response_tasks(&state).await;
    assert_eq!(single_outcome(&state), Outcome::ClientDisconnected);
}

#[tokio::test]
async fn injected_sender_records_streaming_shutdown_from_handle_without_a_socket() {
    let temp = tempfile::tempdir().unwrap();
    let state = RequestProxyState::new(temp.path(), CancellationToken::new()).unwrap();
    let (_upstream_sender, upstream_receiver) = mpsc::channel::<Result<Bytes, io::Error>>(1);
    let sender = FakeUpstreamSender::new(upstream_response(
        StatusCode::OK,
        Some("application/octet-stream"),
        reqwest::Body::wrap_stream(ReceiverStream::new(upstream_receiver)),
    ));

    let response = handle_with_sender(
        state.clone(),
        proxy_request("https://example.com/v1/stream"),
        &sender,
    )
    .await;
    state.shutdown.cancel();
    let _ = response.into_body().collect().await;

    finish_response_tasks(&state).await;
    assert_eq!(single_outcome(&state), Outcome::ServerShutdown);
}

#[test]
fn console_upstream_host_keeps_explicit_ports_and_ipv6_brackets() {
    assert_eq!(
        upstream_host(&Url::parse("https://example.com:8443/path").unwrap()),
        "example.com:8443"
    );
    assert_eq!(
        upstream_host(&Url::parse("https://[2001:db8::1]:8443/path").unwrap()),
        "[2001:db8::1]:8443"
    );
    assert_eq!(
        upstream_host(&Url::parse("https://example.com:443/path").unwrap()),
        "example.com"
    );
}

#[tokio::test]
async fn unsupported_connect_preserves_url_query_headers_and_body_without_a_socket() {
    let temp = tempfile::tempdir().unwrap();
    let state = RequestProxyState::new(temp.path(), CancellationToken::new()).unwrap();
    let target = "http://127.0.0.1:18787/v1/echo?tag=one&tag=&tag=two";
    let mut request = Request::builder()
        .method(Method::CONNECT)
        .uri(format!("/{target}"))
        .body(Body::from(Bytes::from_static(b"request\0\xffbody")))
        .unwrap();
    request
        .headers_mut()
        .append("x-client-repeat", "one".parse().unwrap());
    request
        .headers_mut()
        .append("x-client-repeat", "two".parse().unwrap());

    let response = handle(state.clone(), request).await;
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    let captured_request = state.store.scan().unwrap().remove(0);
    assert_eq!(
        captured_request.request.upstream_url.as_deref(),
        Some(target)
    );
    assert_eq!(
        captured_request
            .request
            .headers
            .iter()
            .filter(|header| header.name == "x-client-repeat")
            .count(),
        2
    );
    assert_eq!(
        std::fs::read(captured_request.directory.join("request.body")).unwrap(),
        b"request\0\xffbody"
    );
    assert_eq!(captured_request.result.unwrap().outcome, Outcome::Rejected);
}

#[test]
fn terminal_retry_preserves_the_original_outcome() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let (captured_request, _) = store
        .begin(ObservedRequest::test("GET", "/failed"))
        .unwrap();
    let id = captured_request.id.clone();
    let summary_path = captured_request.directory.join("summary.json");
    let saved_summary_path = captured_request.directory.join("summary.saved");
    let mut guard = RequestAttempt::new(
        store.clone(),
        captured_request,
        Arc::new(Mutex::new(RuntimeMeasurements::default())),
        Arc::new(Mutex::new(ProtocolObserver::new(None))),
    );

    std::fs::rename(&summary_path, &saved_summary_path).unwrap();
    std::fs::create_dir(&summary_path).unwrap();
    assert!(
        guard
            .finish(
                Outcome::RecordingFailed,
                Some(ErrorMetadata {
                    kind: ErrorKind::ResponseRecordingFailed,
                    message: "response recording failed".to_string(),
                }),
            )
            .is_err()
    );
    std::fs::remove_dir(&summary_path).unwrap();
    std::fs::rename(&saved_summary_path, &summary_path).unwrap();

    drop(guard);

    let result = store.find(&id).unwrap().result.unwrap();
    assert_eq!(result.outcome, Outcome::RecordingFailed);
    assert!(matches!(
        result.error.unwrap().kind,
        ErrorKind::ResponseRecordingFailed
    ));
}

#[tokio::test]
async fn request_chunks_are_recorded_before_they_are_forwarded() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("request.body");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .unwrap();
    let (sender, receiver) = mpsc::channel::<Result<Bytes, Infallible>>(2);
    let body = Body::from_stream(ReceiverStream::new(receiver));
    let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
    let error = Arc::new(Mutex::new(None));
    let stream = recorded_request_stream(
        body,
        tokio::fs::File::from_std(file),
        measurements.clone(),
        error.clone(),
        Instant::now(),
        CancellationToken::new(),
    );
    futures_util::pin_mut!(stream);

    let first = Bytes::from_static(b"request\0");
    sender.send(Ok(first.clone())).await.unwrap();
    assert_eq!(stream.next().await.unwrap().unwrap(), first);
    assert_eq!(std::fs::read(&path).unwrap(), first);

    let second = Bytes::from_static(b"\xffbody");
    sender.send(Ok(second.clone())).await.unwrap();
    assert_eq!(stream.next().await.unwrap().unwrap(), second);
    drop(sender);
    assert!(stream.next().await.is_none());

    assert_eq!(std::fs::read(&path).unwrap(), b"request\0\xffbody");
    let measurements = lock_unpoisoned(&measurements);
    assert_eq!(measurements.request_bytes, 13);
    assert!(measurements.request_body_duration.is_some());
    assert!(lock_unpoisoned(&error).is_none());
}

#[tokio::test]
async fn declared_request_length_checkpoints_without_an_extra_eof_poll() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("request.body");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .unwrap();
    let (sender, receiver) = mpsc::channel::<Result<Bytes, Infallible>>(2);
    let body = Body::from_stream(ReceiverStream::new(receiver));
    let summary = SummaryHandle::new(SummaryMetadata::test(
        String::new(),
        Some(ProtocolSummary::default()),
    ));
    let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
    let error = Arc::new(Mutex::new(None));
    let chunks = [Bytes::from_static(b"com"), Bytes::from_static(b"plete")];
    let stream = recorded_request_stream_with_summary(
        body,
        tokio::fs::File::from_std(file),
        RequestStreamContext {
            measurements: measurements.clone(),
            error_slot: error.clone(),
            summary: summary.clone(),
            protocol: Arc::new(Mutex::new(ProtocolObserver::new(None))),
            request_headers: Vec::new(),
            expected_body_bytes: Some(8),
            request: RequestTarget::Unstored {
                directory: temp.path().to_path_buf(),
            },
            origin: Instant::now(),
            shutdown: CancellationToken::new(),
        },
    );
    futures_util::pin_mut!(stream);

    sender.send(Ok(chunks[0].clone())).await.unwrap();
    assert_eq!(stream.next().await.unwrap().unwrap(), chunks[0]);
    summary.read(|value| {
        assert!(value.timing.upstream_request_body_completed_at_ns.is_none());
    });

    sender.send(Ok(chunks[1].clone())).await.unwrap();
    assert_eq!(stream.next().await.unwrap().unwrap(), chunks[1]);
    assert!(stream.next().await.is_none());
    assert_eq!(std::fs::read(path).unwrap(), b"complete");
    summary.read(|value| {
        assert!(value.timing.upstream_request_body_completed_at_ns.is_some());
    });
    assert!(
        lock_unpoisoned(&measurements)
            .request_body_duration
            .is_some()
    );
    assert!(lock_unpoisoned(&error).is_none());
}

#[tokio::test]
async fn declared_empty_request_is_checkpointed_before_the_body_is_polled() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let (captured_request, _) = store
        .begin(ObservedRequest::test("POST", "/empty"))
        .unwrap();
    let id = captured_request.id.clone();
    let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
    let protocol = Arc::new(Mutex::new(ProtocolObserver::new(None)));
    let mut guard = RequestAttempt::new(
        store.clone(),
        captured_request,
        measurements.clone(),
        protocol,
    );
    let summary = guard.summary_handle();
    let body = Body::from_stream(futures_util::stream::pending::<Result<Bytes, Infallible>>());
    let context = guard.request_stream_context(Vec::new(), Some(0), CancellationToken::new());

    let stream = prepare_recorded_request_stream(&mut guard, body, context)
        .await
        .unwrap_or_else(|response| {
            panic!(
                "empty request preparation failed with {}",
                response.status()
            )
        });

    summary.read(|value| {
        assert!(value.timing.upstream_request_started_at_ns.is_some());
        assert!(value.timing.upstream_request_body_completed_at_ns.is_some());
    });
    assert!(
        lock_unpoisoned(&measurements)
            .request_body_duration
            .is_some()
    );
    let persisted = store.find(&id).unwrap();
    assert!(
        persisted
            .summary
            .timing
            .upstream_request_body_completed_at_ns
            .is_some()
    );
    assert!(
        std::fs::read(persisted.directory.join("request.body"))
            .unwrap()
            .is_empty()
    );
    drop(stream);
}

#[test]
fn declared_content_length_ignores_unusable_values() {
    let mut headers = HeaderMap::new();
    assert_eq!(declared_content_length(&headers), None);

    for (value, expected) in [
        ("0", Some(0)),
        ("42", Some(42)),
        ("1.0", None),
        ("unknown", None),
        ("18446744073709551616", None),
    ] {
        headers.insert(header::CONTENT_LENGTH, value.parse().unwrap());
        assert_eq!(declared_content_length(&headers), expected, "{value}");
    }

    headers.insert(
        header::CONTENT_LENGTH,
        axum::http::HeaderValue::from_bytes(&[0xff]).unwrap(),
    );
    assert_eq!(declared_content_length(&headers), None);
}

#[tokio::test]
async fn failed_request_body_is_not_marked_complete_or_interpreted() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("request.body");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .unwrap();
    let body = Body::from_stream(futures_util::stream::iter([
        Ok(Bytes::from_static(br#"{"model":"partial""#)),
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "client body failed",
        )),
    ]));
    let summary = SummaryHandle::new(SummaryMetadata::test(
        String::new(),
        Some(ProtocolSummary::for_url(Some(
            "https://example.test/v1/responses",
        ))),
    ));
    let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
    let error = Arc::new(Mutex::new(None));
    let stream = recorded_request_stream_with_summary(
        body,
        tokio::fs::File::from_std(file),
        RequestStreamContext {
            measurements: measurements.clone(),
            error_slot: error.clone(),
            summary: summary.clone(),
            protocol: Arc::new(Mutex::new(ProtocolObserver::new(Some(
                "https://example.test/v1/responses",
            )))),
            request_headers: Vec::new(),
            expected_body_bytes: None,
            request: RequestTarget::Unstored {
                directory: temp.path().to_path_buf(),
            },
            origin: Instant::now(),
            shutdown: CancellationToken::new(),
        },
    );
    futures_util::pin_mut!(stream);

    assert!(stream.next().await.unwrap().is_ok());
    assert!(stream.next().await.unwrap().is_err());
    assert!(stream.next().await.is_none());

    summary.read(|value| {
        assert!(value.timing.upstream_request_body_completed_at_ns.is_none());
        assert!(value.protocol.as_ref().unwrap().model.requested.is_none());
    });
    assert!(
        lock_unpoisoned(&measurements)
            .request_body_duration
            .is_none()
    );
    let failure = lock_unpoisoned(&error).clone().unwrap();
    assert_eq!(failure.kind, ErrorKind::RequestBodyFailed);
    assert_eq!(failure.message, "client body failed");
}

#[tokio::test]
async fn sse_chunks_reach_disk_before_the_client_without_a_socket() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let (captured_request, _) = store
        .begin(ObservedRequest {
            upstream_url: Some("https://example.com/v1/responses"),
            host_hint: Some("example.com"),
            ..ObservedRequest::test("GET", "/https://example.com/v1/responses")
        })
        .unwrap();
    let id = captured_request.id.clone();
    let response_path = captured_request.directory.join("response.body");
    let response_file =
        tokio::fs::File::from_std(captured_request.response_body.try_clone().unwrap());
    let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
    let protocol = Arc::new(Mutex::new(ProtocolObserver::new(Some(
        "https://example.com/v1/responses",
    ))));
    let guard = RequestAttempt::new(store.clone(), captured_request, measurements, protocol);
    let (upstream_sender, upstream_receiver) = mpsc::channel::<Result<Bytes, reqwest::Error>>(2);
    let (client_sender, mut client_receiver) = mpsc::channel(2);
    let task = tokio::spawn(async move {
        let mut guard = guard;
        record_response_stream(
            CancellationToken::new(),
            ReceiverStream::new(upstream_receiver),
            response_file,
            client_sender,
            &mut guard,
        )
        .await;
    });

    let first = Bytes::from_static(b"data: first\n\n");
    upstream_sender.send(Ok(first.clone())).await.unwrap();
    assert_eq!(client_receiver.recv().await.unwrap().unwrap(), first);
    assert_eq!(std::fs::read(&response_path).unwrap(), first);
    assert!(store.find(&id).unwrap().active);

    let second = Bytes::from_static(b"data: second\n\n");
    upstream_sender.send(Ok(second.clone())).await.unwrap();
    assert_eq!(client_receiver.recv().await.unwrap().unwrap(), second);
    drop(upstream_sender);
    task.await.unwrap();
    assert!(client_receiver.recv().await.is_none());

    let captured_request = store.find(&id).unwrap();
    assert_eq!(
        std::fs::read(captured_request.directory.join("response.body")).unwrap(),
        b"data: first\n\ndata: second\n\n"
    );
    let result = captured_request.result.unwrap();
    assert_eq!(result.outcome, Outcome::Completed);
    assert_eq!(result.response_bytes, 27);
}

#[tokio::test]
async fn response_recording_failure_errors_the_downstream_without_forwarding_the_chunk() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let (captured_request, _) = store
        .begin(ObservedRequest::test("GET", "/https://example.com/bytes"))
        .unwrap();
    let id = captured_request.id.clone();
    let response_file = tokio::fs::File::from_std(
        std::fs::File::open(captured_request.directory.join("response.body")).unwrap(),
    );
    let guard = RequestAttempt::new(
        store.clone(),
        captured_request,
        Arc::new(Mutex::new(RuntimeMeasurements::default())),
        Arc::new(Mutex::new(ProtocolObserver::new(None))),
    );
    let upstream = futures_util::stream::iter([Ok::<_, reqwest::Error>(Bytes::from_static(
        b"must not reach the client",
    ))]);
    let (client_sender, mut client_receiver) = mpsc::channel(1);
    let task = tokio::spawn(async move {
        let mut guard = guard;
        record_response_stream(
            CancellationToken::new(),
            upstream,
            response_file,
            client_sender,
            &mut guard,
        )
        .await;
    });

    let error = client_receiver.recv().await.unwrap().unwrap_err();
    assert!(error.to_string().contains("response body"), "{error}");
    task.await.unwrap();
    assert!(client_receiver.recv().await.is_none());
    let stored = store.find(&id).unwrap();
    assert_eq!(
        stored.result.as_ref().unwrap().outcome,
        Outcome::RecordingFailed
    );
    assert_eq!(
        stored.result.unwrap().error.unwrap().kind,
        ErrorKind::ResponseRecordingFailed
    );
    assert!(
        std::fs::read(stored.directory.join("response.body"))
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn downstream_send_does_not_block_shutdown_when_the_client_channel_is_full() {
    let shutdown = CancellationToken::new();
    let (sender, _receiver) = mpsc::channel(1);
    sender
        .send(Ok(Bytes::from_static(b"buffered")))
        .await
        .unwrap();
    let send = send_downstream(&sender, &shutdown, Ok(Bytes::from_static(b"blocked")));
    tokio::pin!(send);

    assert!(futures_util::poll!(&mut send).is_pending());
    shutdown.cancel();

    assert_eq!(send.await, DownstreamSend::Shutdown);
}

async fn run_client_close_after_response(
    upstream_url: &'static str,
    chunks: &[&'static [u8]],
    mode: ResponseStreamMode,
) -> (Outcome, ProtocolSummary, TimingMetadata) {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let (captured_request, _) = store
        .begin(ObservedRequest {
            upstream_url: Some(upstream_url),
            host_hint: Some("example.com"),
            ..ObservedRequest::test("POST", upstream_url)
        })
        .unwrap();
    let id = captured_request.id.clone();
    let response_file =
        tokio::fs::File::from_std(captured_request.response_body.try_clone().unwrap());
    let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
    let protocol = Arc::new(Mutex::new(ProtocolObserver::new(Some(upstream_url))));
    let guard = RequestAttempt::new(store.clone(), captured_request, measurements, protocol);
    let (upstream_sender, upstream_receiver) = mpsc::channel::<Result<Bytes, reqwest::Error>>(2);
    let (client_sender, mut client_receiver) = mpsc::channel(2);
    let task = tokio::spawn(async move {
        let mut guard = guard;
        record_response_stream_with_index(
            CancellationToken::new(),
            ReceiverStream::new(upstream_receiver),
            response_file,
            client_sender,
            ResponseStreamConfig {
                mode,
                status: 200,
                headers: Vec::new(),
            },
            &mut guard,
        )
        .await;
    });

    for chunk in chunks {
        upstream_sender
            .send(Ok(Bytes::from_static(chunk)))
            .await
            .unwrap();
        assert_eq!(client_receiver.recv().await.unwrap().unwrap(), *chunk);
    }
    drop(client_receiver);
    task.await.unwrap();

    let captured_request = store.find(&id).unwrap();
    (
        captured_request.result.unwrap().outcome,
        captured_request.summary.protocol.unwrap(),
        captured_request.summary.timing,
    )
}

#[tokio::test]
async fn client_close_after_claude_terminal_event_is_completed() {
    let (outcome, protocol, timing) = run_client_close_after_response(
            "https://example.com/v1/messages",
            &[b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n"],
            ResponseStreamMode::EventStream,
        )
        .await;
    assert_eq!(outcome, Outcome::Completed);
    assert!(!protocol.response_terminal);
    assert!(timing.upstream_response_body_completed_at_ns.is_some());

    let (outcome, protocol, timing) = run_client_close_after_response(
        "https://example.com/v1/messages",
        &[b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"],
        ResponseStreamMode::EventStream,
    )
    .await;
    assert_eq!(outcome, Outcome::Completed);
    assert!(protocol.response_terminal);
    assert!(timing.upstream_response_body_completed_at_ns.is_some());
}

#[tokio::test]
async fn client_close_after_codex_terminal_event_is_completed() {
    let (outcome, protocol, timing) = run_client_close_after_response(
            "https://example.com/v1/responses",
            &[b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":7,\"output_tokens\":3}}}\n\n"],
            ResponseStreamMode::EventStream,
        )
        .await;
    assert_eq!(outcome, Outcome::Completed);
    assert!(protocol.response_terminal);
    assert_eq!(protocol.token_usage.unwrap().output_tokens, Some(3));
    assert!(timing.upstream_response_body_completed_at_ns.is_some());
}

#[tokio::test]
async fn client_close_after_chat_done_is_completed_with_final_usage() {
    let (outcome, protocol, timing) = run_client_close_after_response(
            "https://example.com/v1/chat/completions",
            &[
                b"data: {\"object\":\"chat.completion.chunk\",\"model\":\"gpt-chat\",\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3,\"total_tokens\":10}}\n\n",
                b"data: [DONE]\n\n",
            ],
            ResponseStreamMode::EventStream,
        )
        .await;

    assert_eq!(outcome, Outcome::Completed);
    assert_eq!(protocol.family, ProtocolFamily::OpenaiChatCompletions);
    assert!(protocol.response_terminal);
    assert_eq!(protocol.model.effective.as_deref(), Some("gpt-chat"));
    assert_eq!(protocol.token_usage.unwrap().output_tokens, Some(3));
    assert!(timing.upstream_response_body_completed_at_ns.is_some());
}

#[tokio::test]
async fn unknown_done_stream_still_records_a_client_disconnect() {
    let (outcome, protocol, timing) = run_client_close_after_response(
        "https://example.com/events",
        &[b"data: [DONE]\n\n"],
        ResponseStreamMode::EventStream,
    )
    .await;

    assert_eq!(outcome, Outcome::ClientDisconnected);
    assert_eq!(protocol.family, ProtocolFamily::Unknown);
    assert!(!protocol.response_terminal);
    assert!(timing.upstream_response_body_completed_at_ns.is_none());
}

#[tokio::test]
async fn initial_protocol_events_publish_first_token_and_still_parse_metadata() {
    for (url, event, model) in [
            (
                "https://example.com/v1/responses",
                b"data: {\"type\":\"response.created\",\"response\":{\"model\":\"gpt-test\"}}\n\n"
                    .as_slice(),
                "gpt-test",
            ),
            (
                "https://example.com/v1/messages",
                b"data: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-test\"}}\n\n"
                    .as_slice(),
                "claude-test",
            ),
            (
                "https://example.com/gateway",
                b"data: {\"object\":\"chat.completion.chunk\",\"model\":\"gpt-chat\",\"choices\":[]}\n\n"
                    .as_slice(),
                "gpt-chat",
            ),
        ] {
            let (outcome, protocol, _) =
                run_client_close_after_response(url, &[event], ResponseStreamMode::EventStream)
                    .await;

            assert_eq!(outcome, Outcome::ClientDisconnected);
            assert!(protocol.first_token_at_ns.is_some());
            assert_eq!(protocol.model.effective.as_deref(), Some(model));
        }
}

#[tokio::test]
async fn malformed_sse_data_still_publishes_first_token_and_diagnostics() {
    let (_, protocol, _) = run_client_close_after_response(
        "https://example.com/v1/messages",
        &[b"data: {malformed json\n\n"],
        ResponseStreamMode::EventStream,
    )
    .await;

    assert!(protocol.first_token_at_ns.is_some());
    assert_eq!(protocol.warnings[0].kind, "sse_event_invalid");
}

#[tokio::test]
async fn client_close_after_zstd_terminal_event_is_completed() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let upstream_url = "https://example.com/v1/responses";
    let (captured_request, _) = store
        .begin(ObservedRequest {
            upstream_url: Some(upstream_url),
            host_hint: Some("example.com"),
            ..ObservedRequest::test("POST", upstream_url)
        })
        .unwrap();
    let id = captured_request.id.clone();
    let response_file =
        tokio::fs::File::from_std(captured_request.response_body.try_clone().unwrap());
    let guard = RequestAttempt::new(
        store.clone(),
        captured_request,
        Arc::new(Mutex::new(RuntimeMeasurements::default())),
        Arc::new(Mutex::new(ProtocolObserver::new(Some(upstream_url)))),
    );
    let encoded = zstd::stream::encode_all(
        b"event: response.completed\ndata: {\"type\":\"response.completed\"}\n\n".as_slice(),
        0,
    )
    .unwrap();
    let (upstream_sender, upstream_receiver) = mpsc::channel::<Result<Bytes, reqwest::Error>>(2);
    let (client_sender, mut client_receiver) = mpsc::channel(2);
    let task = tokio::spawn(async move {
        let mut guard = guard;
        record_response_stream_with_index(
            CancellationToken::new(),
            ReceiverStream::new(upstream_receiver),
            response_file,
            client_sender,
            ResponseStreamConfig {
                mode: ResponseStreamMode::OpaqueEventStream,
                status: 200,
                headers: vec![RecordedHeader {
                    name: "content-encoding".to_string(),
                    value_base64: base64::engine::general_purpose::STANDARD.encode("zstd"),
                }],
            },
            &mut guard,
        )
        .await;
    });

    upstream_sender
        .send(Ok(Bytes::from(encoded)))
        .await
        .unwrap();
    assert!(client_receiver.recv().await.unwrap().is_ok());
    // Close the client while the upstream stream stays open: the
    // delivered terminal event must keep this a completed exchange.
    drop(client_receiver);
    task.await.unwrap();
    drop(upstream_sender);

    let captured_request = store.find(&id).unwrap();
    assert_eq!(captured_request.result.unwrap().outcome, Outcome::Completed);
    assert!(captured_request.summary.protocol.unwrap().response_terminal);
    assert!(
        captured_request
            .summary
            .timing
            .upstream_response_body_completed_at_ns
            .is_some()
    );
}

#[tokio::test]
async fn zstd_sse_is_interpreted_only_after_eof_without_event_timing() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let upstream_url = "https://example.com/v1/responses";
    let (captured_request, _) = store
        .begin(ObservedRequest {
            upstream_url: Some(upstream_url),
            host_hint: Some("example.com"),
            ..ObservedRequest::test("POST", upstream_url)
        })
        .unwrap();
    let id = captured_request.id.clone();
    let headers = vec![
        RecordedHeader {
            name: "content-type".to_string(),
            value_base64: base64::engine::general_purpose::STANDARD.encode("text/event-stream"),
        },
        RecordedHeader {
            name: "content-encoding".to_string(),
            value_base64: base64::engine::general_purpose::STANDARD.encode("zstd"),
        },
    ];
    let guard = RequestAttempt::new(
        store.clone(),
        captured_request,
        Arc::new(Mutex::new(RuntimeMeasurements::default())),
        Arc::new(Mutex::new(ProtocolObserver::new(Some(upstream_url)))),
    );
    guard
        .observe_response_headers(&headers, Some(true))
        .unwrap();
    let body = zstd::stream::encode_all(
        br#"event: response.failed
data: {"type":"error","error":{"type":"service_unavailable_error","message":"overloaded"}}

"#
        .as_slice(),
        0,
    )
    .unwrap();
    let response_file = tokio::fs::File::from_std(guard.clone_response_body().unwrap());
    let (sender, mut receiver) = mpsc::channel(2);
    let task = tokio::spawn(async move {
        let mut guard = guard;
        record_response_stream_with_index(
            CancellationToken::new(),
            futures_util::stream::iter([Ok(Bytes::from(body))]),
            response_file,
            sender,
            ResponseStreamConfig {
                mode: ResponseStreamMode::OpaqueEventStream,
                status: 200,
                headers,
            },
            &mut guard,
        )
        .await;
    });
    while receiver.recv().await.is_some() {}
    task.await.unwrap();

    let captured_request = store.find(&id).unwrap();
    let protocol = captured_request.summary.protocol.unwrap();
    assert!(protocol.response_terminal);
    assert_eq!(protocol.errors[0].kind, "service_unavailable_error");
    assert!(protocol.first_token_at_ns.is_none());
    assert!(
        !captured_request
            .directory
            .join("response.events.jsonl")
            .exists()
    );
}

fn brotli_encode(bytes: &[u8]) -> Vec<u8> {
    let mut input = bytes;
    let mut output = Vec::new();
    brotli::BrotliCompress(
        &mut input,
        &mut output,
        &brotli::enc::BrotliEncoderParams::default(),
    )
    .unwrap();
    output
}

#[tokio::test]
async fn brotli_sse_is_interpreted_only_after_eof_without_event_timing() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let upstream_url = "https://example.com/v1/messages";
    let (captured_request, _) = store
        .begin(ObservedRequest {
            upstream_url: Some(upstream_url),
            host_hint: Some("example.com"),
            ..ObservedRequest::test("POST", upstream_url)
        })
        .unwrap();
    let id = captured_request.id.clone();
    let headers = vec![
        RecordedHeader {
            name: "content-type".to_string(),
            value_base64: base64::engine::general_purpose::STANDARD.encode("text/event-stream"),
        },
        RecordedHeader {
            name: "content-encoding".to_string(),
            value_base64: base64::engine::general_purpose::STANDARD.encode("br"),
        },
    ];
    let guard = RequestAttempt::new(
        store.clone(),
        captured_request,
        Arc::new(Mutex::new(RuntimeMeasurements::default())),
        Arc::new(Mutex::new(ProtocolObserver::new(Some(upstream_url)))),
    );
    guard
        .observe_response_headers(&headers, Some(true))
        .unwrap();
    let body = brotli_encode(
        br#"event: message_stop
data: {"type":"message_stop"}

"#,
    );
    let response_file = tokio::fs::File::from_std(guard.clone_response_body().unwrap());
    let (sender, mut receiver) = mpsc::channel(2);
    let task = tokio::spawn(async move {
        let mut guard = guard;
        record_response_stream_with_index(
            CancellationToken::new(),
            futures_util::stream::iter([Ok(Bytes::from(body))]),
            response_file,
            sender,
            ResponseStreamConfig {
                mode: ResponseStreamMode::OpaqueEventStream,
                status: 200,
                headers,
            },
            &mut guard,
        )
        .await;
    });
    while receiver.recv().await.is_some() {}
    task.await.unwrap();

    let captured_request = store.find(&id).unwrap();
    let protocol = captured_request.summary.protocol.unwrap();
    assert!(protocol.response_terminal);
    assert!(captured_request.summary.warnings.is_empty());
    assert_eq!(
        captured_request.summary.assessment.level,
        AssessmentLevel::Ok
    );
    assert!(
        !captured_request
            .directory
            .join("response.events.jsonl")
            .exists()
    );
}

#[tokio::test]
async fn unsupported_sse_content_encoding_warns_without_claiming_a_missing_terminal() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let upstream_url = "https://example.com/v1/messages";
    let (captured_request, _) = store
        .begin(ObservedRequest {
            upstream_url: Some(upstream_url),
            host_hint: Some("example.com"),
            ..ObservedRequest::test("POST", upstream_url)
        })
        .unwrap();
    let id = captured_request.id.clone();
    let headers = vec![
        RecordedHeader {
            name: "content-type".to_string(),
            value_base64: base64::engine::general_purpose::STANDARD.encode("text/event-stream"),
        },
        RecordedHeader {
            name: "content-encoding".to_string(),
            value_base64: base64::engine::general_purpose::STANDARD.encode("compress"),
        },
    ];
    let guard = RequestAttempt::new(
        store.clone(),
        captured_request,
        Arc::new(Mutex::new(RuntimeMeasurements::default())),
        Arc::new(Mutex::new(ProtocolObserver::new(Some(upstream_url)))),
    );
    guard
        .observe_response_headers(&headers, Some(true))
        .unwrap();
    let response_file = tokio::fs::File::from_std(guard.clone_response_body().unwrap());
    let (sender, mut receiver) = mpsc::channel(2);
    let task = tokio::spawn(async move {
        let mut guard = guard;
        record_response_stream_with_index(
            CancellationToken::new(),
            futures_util::stream::iter([Ok(Bytes::from_static(b"not-sse"))]),
            response_file,
            sender,
            ResponseStreamConfig {
                mode: ResponseStreamMode::OpaqueEventStream,
                status: 200,
                headers,
            },
            &mut guard,
        )
        .await;
    });
    while receiver.recv().await.is_some() {}
    task.await.unwrap();

    let captured_request = store.find(&id).unwrap();
    assert_eq!(
        captured_request.summary.warnings[0].kind,
        "response_interpretation_failed"
    );
    assert_eq!(
        captured_request.summary.warnings[0].message,
        "unsupported Content-Encoding \"compress\""
    );
    assert!(
        !captured_request
            .summary
            .protocol
            .as_ref()
            .unwrap()
            .response_terminal
    );
    assert_eq!(
        captured_request
            .summary
            .assessment
            .primary
            .as_ref()
            .unwrap()
            .kind,
        "response_interpretation_failed"
    );
    assert_eq!(captured_request.summary.assessment.issue_count, 1);
}

#[tokio::test]
async fn client_close_before_sse_terminal_event_is_disconnected() {
    let (outcome, protocol, timing) = run_client_close_after_response(
        "https://example.com/v1/messages",
        &[b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\"}\n\n"],
        ResponseStreamMode::EventStream,
    )
    .await;
    assert_eq!(outcome, Outcome::ClientDisconnected);
    assert!(!protocol.response_terminal);
    assert!(protocol.first_token_at_ns.is_some());
    assert!(protocol.token_usage.is_none());
    assert!(timing.upstream_response_body_completed_at_ns.is_none());
}

#[tokio::test]
async fn headerless_split_sse_is_completed_when_client_closes_after_terminal_event() {
    let (outcome, protocol, timing) = run_client_close_after_response(
            "https://example.com/v1/responses",
            &[
                b"eve",
                b"nt: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":7,\"output_tokens\":3}}}\n\n",
            ],
            ResponseStreamMode::Detect,
        )
        .await;

    assert_eq!(outcome, Outcome::Completed);
    assert_eq!(
        protocol.response_mode.observed,
        Some(ResponseModeValue::Stream)
    );
    assert!(timing.upstream_response_body_completed_at_ns.is_some());
    assert!(protocol.response_terminal);
    assert_eq!(protocol.token_usage.unwrap().output_tokens, Some(3));
}

#[tokio::test]
async fn headerless_json_response_remains_normal_and_keeps_usage() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let upstream_url = "https://example.com/v1/responses";
    let (captured_request, _) = store
        .begin(ObservedRequest {
            upstream_url: Some(upstream_url),
            host_hint: Some("example.com"),
            ..ObservedRequest::test("POST", upstream_url)
        })
        .unwrap();
    let id = captured_request.id.clone();
    let event_index_path = captured_request.directory.join("response.events.jsonl");
    let response_file =
        tokio::fs::File::from_std(captured_request.response_body.try_clone().unwrap());
    let guard = RequestAttempt::new(
        store.clone(),
        captured_request,
        Arc::new(Mutex::new(RuntimeMeasurements::default())),
        Arc::new(Mutex::new(ProtocolObserver::new(Some(upstream_url)))),
    );
    let body = Bytes::from_static(
            br#"{"object":"response","model":"gpt-test","usage":{"input_tokens":12,"output_tokens":4}}"#,
        );
    let expected_body = body.clone();
    let (client_sender, mut client_receiver) = mpsc::channel(2);
    let task = tokio::spawn(async move {
        let mut guard = guard;
        record_response_stream_with_index(
            CancellationToken::new(),
            futures_util::stream::iter([Ok(body.clone())]),
            response_file,
            client_sender,
            ResponseStreamConfig {
                mode: ResponseStreamMode::Detect,
                status: 200,
                headers: Vec::new(),
            },
            &mut guard,
        )
        .await;
    });

    assert_eq!(
        client_receiver.recv().await.unwrap().unwrap(),
        expected_body
    );
    task.await.unwrap();
    assert!(client_receiver.recv().await.is_none());

    let captured_request = store.find(&id).unwrap();
    assert_eq!(captured_request.result.unwrap().outcome, Outcome::Completed);
    assert_eq!(
        captured_request
            .summary
            .protocol
            .as_ref()
            .unwrap()
            .response_mode
            .observed,
        Some(ResponseModeValue::Normal)
    );
    assert!(
        captured_request
            .summary
            .protocol
            .as_ref()
            .unwrap()
            .response_terminal
    );
    assert_eq!(
        captured_request
            .summary
            .protocol
            .unwrap()
            .token_usage
            .unwrap()
            .output_tokens,
        Some(4)
    );
    assert!(!event_index_path.exists());
}

#[test]
fn response_stream_mode_only_sniffs_successful_requested_streams_without_content_type() {
    let mut protocol = ProtocolSummary::for_url(Some("https://example.com/v1/responses"));
    protocol.response_mode.requested = Some(ResponseModeValue::Stream);
    let mut headers = HeaderMap::new();

    assert_eq!(
        response_stream_mode(&headers, StatusCode::OK, &protocol),
        ResponseStreamMode::Detect
    );
    assert_eq!(
        response_stream_mode(&headers, StatusCode::UNAUTHORIZED, &protocol),
        ResponseStreamMode::Normal
    );

    headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    assert_eq!(
        response_stream_mode(&headers, StatusCode::OK, &protocol),
        ResponseStreamMode::Normal
    );
    headers.insert(
        header::CONTENT_TYPE,
        "text/event-stream; charset=utf-8".parse().unwrap(),
    );
    assert_eq!(
        response_stream_mode(&headers, StatusCode::OK, &protocol),
        ResponseStreamMode::EventStream
    );
    headers.insert(header::CONTENT_ENCODING, "zstd".parse().unwrap());
    assert_eq!(
        response_stream_mode(&headers, StatusCode::OK, &protocol),
        ResponseStreamMode::OpaqueEventStream
    );
    headers.insert(header::CONTENT_ENCODING, "br".parse().unwrap());
    assert_eq!(
        response_stream_mode(&headers, StatusCode::OK, &protocol),
        ResponseStreamMode::OpaqueEventStream
    );
    headers.insert(header::CONTENT_ENCODING, "deflate".parse().unwrap());
    assert_eq!(
        response_stream_mode(&headers, StatusCode::OK, &protocol),
        ResponseStreamMode::OpaqueEventStream
    );
    headers.insert(header::CONTENT_ENCODING, "compress".parse().unwrap());
    assert_eq!(
        response_stream_mode(&headers, StatusCode::OK, &protocol),
        ResponseStreamMode::OpaqueEventStream
    );
}

#[test]
fn sse_prefix_sniffer_handles_split_bom_and_rejects_json() {
    let mut sniffer = SsePrefixSniffer::default();
    assert_eq!(sniffer.observe(b"\xef"), PrefixSniff::Pending);
    assert_eq!(sniffer.observe(b"\xbb\xbfeve"), PrefixSniff::Pending);
    assert_eq!(
        sniffer.observe(b"nt: response.created\n"),
        PrefixSniff::EventStream
    );

    let mut json = SsePrefixSniffer::default();
    assert_eq!(
        json.observe(br#"{"object":"response"}"#),
        PrefixSniff::Normal
    );
}

#[test]
fn first_token_data_matches_relay_line_filtering() {
    assert!(!is_first_token_data(b""));
    assert!(!is_first_token_data(b" \t\r\n"));
    assert!(!is_first_token_data("\u{00a0}".as_bytes()));
    assert!(!is_first_token_data(b" [DONE]"));
    assert!(!is_first_token_data(b"[DONE] trailing relay text"));
    assert!(is_first_token_data(b"ping"));
    assert!(is_first_token_data(b"{"));
    assert!(is_first_token_data(b"\xff"));
}

#[test]
fn sse_first_token_counts_any_eligible_data_line_and_never_overwrites_it() {
    let ignored = b"\xef\xbb\xbf: comment\nevent: response.created\r\ndata:\rdata: \t \ndata: [DONE] trailing\r\n";
    let mut indexer = SseIndexer::new(None, "captured_request-1".to_string());
    indexer.feed(ignored, 0, "1").unwrap();
    assert!(indexer.take_first_token_at_ns().is_none());

    let message_start = b"data:\ndata: {\"type\":\"message_start\"}\n\n";
    indexer
        .feed(message_start, ignored.len() as u64, "2")
        .unwrap();
    assert_eq!(indexer.take_first_token_at_ns().as_deref(), Some("2"));

    indexer
        .feed(
            b"data: ping\n\n",
            (ignored.len() + message_start.len()) as u64,
            "3",
        )
        .unwrap();
    assert!(indexer.take_first_token_at_ns().is_none());
}

#[test]
fn sse_first_token_accepts_relay_compatible_non_output_data() {
    for line in [
        b"data: ping\n".as_slice(),
        b"data: {\"type\":\"error\",\"error\":{}}\n".as_slice(),
        b"data: {malformed json\n".as_slice(),
        b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"\"}\n".as_slice(),
        b"data: {\"type\":\"response.created\"}\n".as_slice(),
    ] {
        let mut indexer = SseIndexer::new(None, "captured_request-1".to_string());
        indexer.feed(line, 0, "7").unwrap();
        assert_eq!(indexer.take_first_token_at_ns().as_deref(), Some("7"));
    }
}

#[test]
fn sse_first_token_supports_lf_cr_and_crlf_lines() {
    for body in [
        b"data: ping\ndata: later\n".as_slice(),
        b"data: ping\rdata: later\r".as_slice(),
        b"data: ping\r\ndata: later\r\n".as_slice(),
    ] {
        let mut indexer = SseIndexer::new(None, "captured_request-1".to_string());
        indexer.feed(body, 0, "11").unwrap();
        indexer.finish().unwrap();
        assert_eq!(indexer.take_first_token_at_ns().as_deref(), Some("11"));
    }
}

#[test]
fn sse_first_token_uses_line_completion_and_eof_arrival_times() {
    let mut indexer = SseIndexer::new(None, "captured_request-1".to_string());
    let first = b"\xef\xbb\xbfdata: {\"type\":\"response.created\"}";
    indexer.feed(first, 0, "1").unwrap();
    assert!(indexer.take_first_token_at_ns().is_none());
    indexer.feed(b"\r", first.len() as u64, "2").unwrap();
    assert!(indexer.take_first_token_at_ns().is_none());
    indexer.feed(b"\n", (first.len() + 1) as u64, "3").unwrap();
    assert_eq!(indexer.take_first_token_at_ns().as_deref(), Some("3"));

    let mut eof = SseIndexer::new(None, "captured_request-2".to_string());
    eof.feed(b"data: ping", 0, "8").unwrap();
    assert!(eof.take_first_token_at_ns().is_none());
    assert!(eof.finish().unwrap());
    assert_eq!(eof.take_first_token_at_ns().as_deref(), Some("8"));
}

#[test]
fn terminal_sse_detection_does_not_require_an_event_index() {
    let mut indexer = SseIndexer::new(None, "captured_request-1".to_string());
    let first = b"data: {\"type\":\"response.com";
    indexer.feed(first, 0, "1").unwrap();
    indexer
        .feed(b"pleted\"}\n\n", first.len() as u64, "2")
        .unwrap();

    assert!(indexer.terminal_seen(ProtocolFamily::OpenaiResponses));
    assert_eq!(
        indexer.terminal_at_ns(ProtocolFamily::OpenaiResponses),
        Some("2")
    );
    let tracker = ResponseBodyTracker::EventStream(Box::new(indexer));
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let (captured_request, _) = store
        .begin(ObservedRequest::test("GET", "/terminal"))
        .unwrap();
    let guard = RequestAttempt::new(
        store,
        captured_request,
        Arc::new(Mutex::new(RuntimeMeasurements::default())),
        Arc::new(Mutex::new(ProtocolObserver::new(None))),
    );
    let terminal = client_closed_terminal(&tracker, &guard);
    assert_eq!(terminal.terminal.outcome, Outcome::Completed);
    assert_eq!(terminal.completed_at_ns.as_deref(), Some("2"));
}

#[tokio::test]
async fn upstream_eof_wins_when_client_closes_at_the_same_time() {
    let temp = tempfile::tempdir().unwrap();
    let store = RequestStore::open(temp.path()).unwrap();
    let (captured_request, _) = store
        .begin(ObservedRequest {
            upstream_url: Some("https://example.com/v1/health"),
            host_hint: Some("example.com"),
            ..ObservedRequest::test("GET", "/https://example.com/v1/health")
        })
        .unwrap();
    let id = captured_request.id.clone();
    let response_file =
        tokio::fs::File::from_std(captured_request.response_body.try_clone().unwrap());
    let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
    let protocol = Arc::new(Mutex::new(ProtocolObserver::new(Some(
        "https://example.com/v1/responses",
    ))));
    let guard = RequestAttempt::new(store.clone(), captured_request, measurements, protocol);
    let (upstream_sender, upstream_receiver) = mpsc::channel::<Result<Bytes, reqwest::Error>>(1);
    let (client_sender, client_receiver) = mpsc::channel(1);
    drop(upstream_sender);
    drop(client_receiver);
    let task = tokio::spawn(async move {
        let mut guard = guard;
        record_response_stream(
            CancellationToken::new(),
            ReceiverStream::new(upstream_receiver),
            response_file,
            client_sender,
            &mut guard,
        )
        .await;
    });

    task.await.unwrap();
    assert_eq!(
        store.find(&id).unwrap().result.unwrap().outcome,
        Outcome::Completed
    );
}

#[test]
fn sse_index_handles_bom_split_chunks_and_crlf_ranges() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("response.events.jsonl");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(true)
        .open(&path)
        .unwrap();
    let mut indexer = SseIndexer::new(Some(file), "captured_request-1".to_string());
    indexer.feed(b"\xef", 0, "1").unwrap();
    indexer.feed(b"\xbb\xbfdata: first\r", 1, "2").unwrap();
    indexer.feed(b"\n\r\ndata: second\n\n", 15, "3").unwrap();
    assert!(!indexer.finish().unwrap());
    let lines = std::fs::read_to_string(path).unwrap();
    let entries: Vec<serde_json::Value> = lines
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["sequence"], 0);
    assert_eq!(entries[0]["body_start"], 3);
    assert_eq!(entries[0]["body_end"], 18);
    assert_eq!(entries[1]["sequence"], 1);
    assert_eq!(entries[1]["body_start"], 18);
}

#[test]
fn hop_by_hop_and_connection_named_headers_are_removed() {
    let mut headers = HeaderMap::new();
    headers.append(
        header::CONNECTION,
        axum::http::HeaderValue::from_bytes(b"x-internal, keep-alive, \xff").unwrap(),
    );
    headers.append(
        axum::http::HeaderName::from_static("x-internal"),
        "secret".parse().unwrap(),
    );
    headers.append(
        axum::http::HeaderName::from_static("x-repeat"),
        "one".parse().unwrap(),
    );
    headers.append(
        axum::http::HeaderName::from_static("x-repeat"),
        "two".parse().unwrap(),
    );
    let forwarded = forwarded_headers(&headers);
    assert!(!forwarded.contains_key(header::CONNECTION));
    assert!(!forwarded.contains_key("x-internal"));
    assert_eq!(forwarded.get_all("x-repeat").iter().count(), 2);
}

#[test]
fn recorded_headers_drop_connection_named_fields() {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONNECTION,
        axum::http::HeaderValue::from_bytes(b"x-hop, keep-alive, \xff").unwrap(),
    );
    headers.insert("x-hop", "secret".parse().unwrap());
    headers.insert("x-app", "kept".parse().unwrap());

    let recorded = recorded_headers(&headers);

    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].name, "x-app");
}

#[tokio::test]
async fn literal_loopback_and_private_targets_resolve_without_a_socket() {
    for (target, address) in [
        ("http://127.0.0.1:18787/v1/responses", "127.0.0.1:18787"),
        ("http://10.0.0.1:18787/v1/responses", "10.0.0.1:18787"),
        ("http://[::1]:18787/v1/responses", "[::1]:18787"),
    ] {
        let resolved = validate_and_resolve(&Url::parse(target).unwrap()).await;
        assert_eq!(resolved.unwrap(), vec![address.parse().unwrap()]);
    }
}
