use super::attempt::*;
use super::headers::*;
use super::request_stream::*;
use super::response_stream::*;
use super::target::*;
use super::*;
use crate::request::model::{
    ErrorMetadata, ProtocolFamily, ProtocolSummary, RecordedHeader, ResponseModeValue,
    SummaryMetadata, TimingMetadata,
};
use crate::request::sse::{PrefixSniff, SseIndexer, SsePrefixSniffer, is_first_token_data};
use crate::request::store::{RequestStore, SummaryHandle};
use axum::http::{HeaderMap, Method, header};
use base64::Engine as _;
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::BodyExt as _;
use std::convert::Infallible;
use std::io;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

struct FakeUpstreamSender {
    response: Mutex<Option<reqwest::Response>>,
}

impl FakeUpstreamSender {
    fn new(response: reqwest::Response) -> Self {
        Self {
            response: Mutex::new(Some(response)),
        }
    }
}

impl UpstreamSender for FakeUpstreamSender {
    type Connection = ();

    fn connect(
        &self,
        _url: &Url,
        _allow_private_upstream: bool,
    ) -> UpstreamFuture<'_, Result<Self::Connection, UpstreamConnectError>> {
        Box::pin(async { Ok(()) })
    }

    fn send(
        &self,
        _connection: Self::Connection,
        request: UpstreamRequest,
    ) -> UpstreamFuture<'_, Result<reqwest::Response, UpstreamSendError>> {
        let response = self
            .response
            .lock()
            .expect("fake upstream response store poisoned")
            .take()
            .expect("fake upstream response already used");
        Box::pin(async move {
            request
                .body
                .collect()
                .await
                .map_err(|error| UpstreamSendError {
                    message: error.to_string(),
                    timeout: false,
                })?;
            Ok(response)
        })
    }
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
async fn rejected_request_preserves_url_query_headers_and_body_without_a_socket() {
    let temp = tempfile::tempdir().unwrap();
    let state = RequestProxyState::new(temp.path(), CancellationToken::new()).unwrap();
    let target = "http://192.0.2.1/v1/echo?tag=one&tag=&tag=two";
    let mut request = Request::builder()
        .method(Method::POST)
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
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
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
                headers: Vec::new(),
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
fn public_address_filter_rejects_special_ranges() {
    for address in [
        "0.0.0.0",
        "10.0.0.1",
        "100.64.0.1",
        "127.0.0.1",
        "169.254.169.254",
        "172.16.0.1",
        "192.0.2.1",
        "192.168.1.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "240.0.0.1",
        "::",
        "::1",
        "::2",
        "::ffff:127.0.0.1",
        "100::1",
        "2001:db8::1",
        "3fff::1",
        "fc00::1",
        "fe80::1",
        "ff00::1",
    ] {
        assert!(
            !is_public_ip(address.parse().unwrap()),
            "accepted {address}"
        );
    }
    for address in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
        assert!(is_public_ip(address.parse().unwrap()), "rejected {address}");
    }
}

#[test]
fn upstream_address_filter_accepts_fake_ip_range() {
    for address in ["198.18.0.0", "198.18.2.68", "198.19.255.255"] {
        let address = address.parse().unwrap();
        assert!(!is_public_ip(address), "publicly routed {address}");
        assert!(
            is_allowed_upstream_ip(address),
            "rejected Fake-IP address {address}"
        );
    }
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

#[test]
fn one_non_public_dns_candidate_rejects_the_whole_target() {
    let mixed = [
        "1.1.1.1:443".parse().unwrap(),
        "10.0.0.1:443".parse().unwrap(),
    ];
    assert!(matches!(
        require_allowed_addresses("mixed.example", &mixed, false),
        Err(TargetError::Rejected(_))
    ));
    assert!(require_allowed_addresses("test-only", &mixed, true).is_ok());
}
