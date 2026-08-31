//! Upstream response recording, downstream streaming, and terminal mapping.

use super::attempt::{RequestAttempt, RequestTerminal};
use super::headers::{forwarded_headers, recorded_headers};
use super::target::version_name;
use crate::request::RequestProxyState;
use crate::request::interpretation::{BodyContentCoding, body_content_coding};
use crate::request::model::{
    ErrorKind, ErrorMetadata, Outcome, ProtocolFamily, ProtocolSummary, RecordedHeader,
    ResponseMetadata, ResponseModeValue, ResponseSource, utc_now,
};
use crate::request::response_observation::replay_encoded_sse_prefix;
use crate::request::sse::{PrefixSniff, SseIndexer, SsePrefixSniffer};
use crate::request::store::FORMAT_VERSION;
use axum::body::Body;
use axum::http::{HeaderMap, Response, StatusCode, header};
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use std::io;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

pub(super) fn stream_upstream_response(
    state: &RequestProxyState,
    upstream_response: reqwest::Response,
    mut guard: RequestAttempt,
) -> Response<Body> {
    let status = upstream_response.status();
    let version = upstream_response.version();
    let original_headers = upstream_response.headers().clone();
    let response_stream_mode =
        response_stream_mode(&original_headers, status, &guard.protocol_summary());
    let metadata = ResponseMetadata {
        format_version: FORMAT_VERSION,
        source: ResponseSource::Upstream,
        headers_at: utc_now(),
        status: status.as_u16(),
        http_version: version_name(version).to_string(),
        headers: recorded_headers(&original_headers),
    };
    if let Err(error) = guard.observe_response_headers(
        &metadata.headers,
        response_stream_mode.observed_event_stream(),
    ) {
        return recording_failure(
            &mut guard,
            format!("checkpoint response metadata: {error:#}"),
        );
    }
    if let Err(error) = guard.write_response(&metadata) {
        return recording_failure(
            &mut guard,
            format!("write Upstream Response metadata: {error:#}"),
        );
    }
    let response_file = match guard.clone_response_body() {
        Ok(file) => tokio::fs::File::from_std(file),
        Err(error) => {
            return recording_failure(&mut guard, format!("clone response body file: {error}"));
        }
    };

    let (sender, receiver) = mpsc::channel::<Result<Bytes, io::Error>>(8);
    let state_for_task = state.clone();
    state.spawn_response_task(async move {
        record_response_stream_with_index(
            state_for_task.shutdown.clone(),
            upstream_response.bytes_stream(),
            response_file,
            sender,
            ResponseStreamConfig {
                mode: response_stream_mode,
                status: status.as_u16(),
                headers: metadata.headers.clone(),
            },
            &mut guard,
        )
        .await;
    });

    let mut builder = Response::builder().status(status).version(version);
    *builder.headers_mut().expect("response builder has headers") =
        forwarded_headers(&original_headers);
    builder
        .body(Body::from_stream(ReceiverStream::new(receiver)))
        .unwrap_or_else(|error| bare_error(StatusCode::BAD_GATEWAY, &error.to_string()))
}

#[cfg(test)]
pub(super) fn record_response_stream(
    shutdown: tokio_util::sync::CancellationToken,
    stream: impl futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    file: tokio::fs::File,
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
    guard: &mut RequestAttempt,
) -> impl std::future::Future<Output = ()> + '_ {
    record_response_stream_with_index(
        shutdown,
        stream,
        file,
        sender,
        ResponseStreamConfig {
            mode: ResponseStreamMode::Normal,
            status: 200,
            headers: Vec::new(),
        },
        guard,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ResponseStreamMode {
    Normal,
    EventStream,
    OpaqueEventStream,
    Detect,
}

impl ResponseStreamMode {
    pub(super) fn observed_event_stream(self) -> Option<bool> {
        match self {
            Self::Normal => Some(false),
            Self::EventStream | Self::OpaqueEventStream => Some(true),
            Self::Detect => None,
        }
    }
}

pub(super) struct ResponseStreamConfig {
    pub(super) mode: ResponseStreamMode,
    pub(super) status: u16,
    pub(super) headers: Vec<RecordedHeader>,
}

pub(super) struct ResponseStreamEnd {
    pub(super) terminal: RequestTerminal,
    pub(super) completed_at_ns: Option<String>,
}

impl ResponseStreamEnd {
    pub(super) fn completed(at_ns: String) -> Self {
        Self {
            terminal: RequestTerminal {
                outcome: Outcome::Completed,
                error: None,
            },
            completed_at_ns: Some(at_ns),
        }
    }

    pub(super) fn incomplete(terminal: RequestTerminal) -> Self {
        Self {
            terminal,
            completed_at_ns: None,
        }
    }
}

const RESPONSE_SHUTDOWN_MESSAGE: &str = "Request Proxy stopped while the response was streaming";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DownstreamSend {
    Sent,
    Closed,
    Shutdown,
}

pub(super) async fn send_downstream(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    shutdown: &tokio_util::sync::CancellationToken,
    item: Result<Bytes, io::Error>,
) -> DownstreamSend {
    tokio::select! {
        biased;
        () = shutdown.cancelled() => DownstreamSend::Shutdown,
        result = sender.send(item) => {
            if result.is_ok() {
                DownstreamSend::Sent
            } else {
                DownstreamSend::Closed
            }
        }
    }
}

pub(super) enum ResponseBodyTracker {
    Normal,
    OpaqueEventStream(BodyContentCoding),
    EventStream(Box<SseIndexer>),
    Detect {
        sniffer: SsePrefixSniffer,
        pending: Vec<(Bytes, String)>,
    },
}

impl ResponseBodyTracker {
    pub(super) fn new(
        mode: ResponseStreamMode,
        guard: &RequestAttempt,
        headers: &[RecordedHeader],
    ) -> Self {
        match mode {
            ResponseStreamMode::Normal => Self::Normal,
            ResponseStreamMode::EventStream => Self::EventStream(Box::new(new_sse_indexer(guard))),
            ResponseStreamMode::OpaqueEventStream => Self::OpaqueEventStream(
                body_content_coding(headers).unwrap_or(BodyContentCoding::Identity),
            ),
            ResponseStreamMode::Detect => Self::Detect {
                sniffer: SsePrefixSniffer::default(),
                pending: Vec::new(),
            },
        }
    }

    pub(super) fn observe_chunk(
        &mut self,
        chunk: &Bytes,
        at_ns: String,
        guard: &RequestAttempt,
    ) -> anyhow::Result<()> {
        match self {
            Self::Normal | Self::OpaqueEventStream(_) => Ok(()),
            Self::EventStream(indexer) => feed_sse_chunk(indexer, chunk, &at_ns, guard),
            Self::Detect { sniffer, pending } => {
                pending.push((chunk.clone(), at_ns));
                match sniffer.observe(chunk) {
                    PrefixSniff::Pending => Ok(()),
                    PrefixSniff::Normal => {
                        guard.observe_response_mode(false)?;
                        *self = Self::Normal;
                        Ok(())
                    }
                    PrefixSniff::EventStream => {
                        guard.observe_response_mode(true)?;
                        let buffered = std::mem::take(pending);
                        let mut indexer = new_sse_indexer(guard);
                        for (buffered_chunk, buffered_at_ns) in buffered {
                            feed_sse_chunk(&mut indexer, &buffered_chunk, &buffered_at_ns, guard)?;
                        }
                        *self = Self::EventStream(Box::new(indexer));
                        Ok(())
                    }
                }
            }
        }
    }

    pub(super) fn finish(&mut self, guard: &RequestAttempt) -> anyhow::Result<()> {
        match self {
            Self::Normal => Ok(()),
            Self::OpaqueEventStream(coding) => guard.observe_encoded_sse_response(*coding),
            Self::Detect { .. } => {
                guard.observe_response_mode(false)?;
                *self = Self::Normal;
                Ok(())
            }
            Self::EventStream(indexer) => {
                match indexer.finish() {
                    Ok(true) => guard.add_warning(
                        "event_index_failed",
                        "truncated SSE event was not indexed".to_string(),
                    ),
                    Ok(false) => {}
                    Err(error) => guard.add_warning("event_index_failed", error.to_string()),
                }
                let events = indexer.take_protocol_events();
                guard.observe_sse_events(&events)?;
                if let Some(at_ns) = indexer.take_first_token_at_ns() {
                    guard.observe_first_token(at_ns)?;
                }
                Ok(())
            }
        }
    }

    pub(super) fn is_event_stream(&self) -> bool {
        matches!(self, Self::EventStream(_) | Self::OpaqueEventStream(_))
    }

    fn opaque_coding(&self) -> Option<BodyContentCoding> {
        match self {
            Self::OpaqueEventStream(coding) => Some(*coding),
            _ => None,
        }
    }

    pub(super) fn terminal_at_ns(&self, family: ProtocolFamily) -> Option<&str> {
        match self {
            Self::EventStream(indexer) => indexer.terminal_at_ns(family),
            Self::Normal | Self::OpaqueEventStream(_) | Self::Detect { .. } => None,
        }
    }
}

pub(super) fn new_sse_indexer(guard: &RequestAttempt) -> SseIndexer {
    let event_index = match guard.create_event_index() {
        Ok(file) => Some(file),
        Err(error) => {
            guard.add_warning("event_index_failed", error.to_string());
            None
        }
    };
    SseIndexer::new(event_index, guard.request_id().to_string())
}

pub(super) fn feed_sse_chunk(
    indexer: &mut SseIndexer,
    chunk: &Bytes,
    at_ns: &str,
    guard: &RequestAttempt,
) -> anyhow::Result<()> {
    let body_offset = indexer.body_offset();
    if let Err(error) = indexer.feed(chunk, body_offset, at_ns) {
        guard.add_warning("event_index_failed", error.to_string());
        indexer.disable_indexing();
    }
    guard.observe_sse_events(&indexer.take_protocol_events())?;
    if let Some(at_ns) = indexer.take_first_token_at_ns() {
        guard.observe_first_token(at_ns)?;
    }
    Ok(())
}

pub(super) fn response_error(
    outcome: Outcome,
    kind: ErrorKind,
    message: impl Into<String>,
) -> RequestTerminal {
    RequestTerminal {
        outcome,
        error: Some(ErrorMetadata {
            kind,
            message: message.into(),
        }),
    }
}

/// Best-effort truncation signal on shutdown. Without an injected error the
/// body channel closes cleanly and a chunked/SSE client cannot distinguish a
/// truncated response from a normal end of stream. `try_send` never blocks a
/// shutdown on a client that has stopped reading.
pub(super) fn notify_shutdown_truncation(sender: &mpsc::Sender<Result<Bytes, io::Error>>) {
    let _ = sender.try_send(Err(io::Error::new(
        io::ErrorKind::Interrupted,
        RESPONSE_SHUTDOWN_MESSAGE,
    )));
}

pub(super) async fn notify_response_error(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    shutdown: &tokio_util::sync::CancellationToken,
    error: io::Error,
    outcome: Outcome,
    kind: ErrorKind,
) -> RequestTerminal {
    let message = error.to_string();
    let _ = send_downstream(sender, shutdown, Err(error)).await;
    response_error(outcome, kind, message)
}

pub(super) async fn notify_recording_error(
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    shutdown: &tokio_util::sync::CancellationToken,
    error: io::Error,
) -> RequestTerminal {
    notify_response_error(
        sender,
        shutdown,
        error,
        Outcome::RecordingFailed,
        ErrorKind::ResponseRecordingFailed,
    )
    .await
}

pub(super) async fn write_response_chunk(
    file: &mut tokio::fs::File,
    chunk: &Bytes,
) -> io::Result<()> {
    file.write_all(chunk).await.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("write Upstream Response body: {error}"),
        )
    })?;
    file.flush()
        .await
        .map_err(|error| io::Error::new(error.kind(), format!("flush response body: {error}")))
}

pub(super) async fn record_response_chunk(
    chunk: Bytes,
    file: &mut tokio::fs::File,
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    shutdown: &tokio_util::sync::CancellationToken,
    tracker: &mut ResponseBodyTracker,
    guard: &RequestAttempt,
) -> Option<ResponseStreamEnd> {
    if guard.response_first_byte_unseen()
        && let Err(error) = guard.mark_timing(|timing| {
            timing.upstream_response_body_first_byte_at_ns = Some(guard.at_ns());
        })
    {
        return Some(ResponseStreamEnd::incomplete(
            notify_recording_error(sender, shutdown, io::Error::other(error.to_string())).await,
        ));
    }

    if let Err(error) = write_response_chunk(file, &chunk).await {
        return Some(ResponseStreamEnd::incomplete(
            notify_recording_error(sender, shutdown, error).await,
        ));
    }

    guard.add_response_bytes(chunk.len());
    if let Err(error) = tracker.observe_chunk(&chunk, guard.at_ns(), guard) {
        return Some(ResponseStreamEnd::incomplete(
            notify_recording_error(sender, shutdown, io::Error::other(error.to_string())).await,
        ));
    }

    match send_downstream(sender, shutdown, Ok(chunk)).await {
        DownstreamSend::Sent => None,
        DownstreamSend::Closed => Some(client_closed_terminal(tracker, guard)),
        DownstreamSend::Shutdown => {
            notify_shutdown_truncation(sender);
            Some(ResponseStreamEnd::incomplete(response_error(
                Outcome::ServerShutdown,
                ErrorKind::ServerShutdown,
                RESPONSE_SHUTDOWN_MESSAGE,
            )))
        }
    }
}

pub(super) async fn stream_response_body(
    shutdown: &tokio_util::sync::CancellationToken,
    stream: impl futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    file: &mut tokio::fs::File,
    sender: &mpsc::Sender<Result<Bytes, io::Error>>,
    tracker: &mut ResponseBodyTracker,
    guard: &RequestAttempt,
) -> ResponseStreamEnd {
    let mut stream = Box::pin(stream);
    loop {
        let next = tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                notify_shutdown_truncation(sender);
                return ResponseStreamEnd::incomplete(response_error(
                        Outcome::ServerShutdown,
                        ErrorKind::ServerShutdown,
                        RESPONSE_SHUTDOWN_MESSAGE,
                    ));
            }
            // Prefer an already-ready upstream EOF when the client closes at
            // the same time. This avoids turning a normal response into a
            // disconnect solely because the downstream body was dropped first.
            next = stream.try_next() => next,
            () = sender.closed() => return client_closed_terminal(tracker, guard),
        };
        match next {
            Ok(Some(chunk)) => {
                if let Some(terminal) =
                    record_response_chunk(chunk, file, sender, shutdown, tracker, guard).await
                {
                    return terminal;
                }
            }
            Ok(None) => {
                return match tracker.finish(guard) {
                    Ok(()) => ResponseStreamEnd::completed(guard.at_ns()),
                    Err(error) => ResponseStreamEnd::incomplete(response_error(
                        Outcome::RecordingFailed,
                        ErrorKind::ResponseRecordingFailed,
                        error.to_string(),
                    )),
                };
            }
            Err(error) => {
                // A recorded request-side failure (client disconnect,
                // recording error, shutdown) aborts the in-flight upstream
                // request and resurfaces here; attribute it to its cause
                // rather than to the upstream.
                let recorded_failure = guard.request_stream_failure();
                if let Some(failure) = recorded_failure {
                    let outcome = match failure.kind {
                        ErrorKind::ClientDisconnected | ErrorKind::RequestBodyFailed => {
                            Outcome::ClientDisconnected
                        }
                        ErrorKind::ServerShutdown => Outcome::ServerShutdown,
                        _ => Outcome::RecordingFailed,
                    };
                    let terminal = notify_response_error(
                        sender,
                        shutdown,
                        io::Error::new(io::ErrorKind::UnexpectedEof, failure.message.clone()),
                        outcome,
                        failure.kind,
                    )
                    .await;
                    return ResponseStreamEnd::incomplete(terminal);
                }
                let terminal = notify_response_error(
                    sender,
                    shutdown,
                    io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        format!("upstream response stream failed: {error}"),
                    ),
                    Outcome::UpstreamError,
                    ErrorKind::UpstreamResponseFailed,
                )
                .await;
                return ResponseStreamEnd::incomplete(terminal);
            }
        }
    }
}

pub(super) async fn record_response_stream_with_index(
    shutdown: tokio_util::sync::CancellationToken,
    stream: impl futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    mut file: tokio::fs::File,
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
    config: ResponseStreamConfig,
    guard: &mut RequestAttempt,
) {
    let ResponseStreamConfig {
        mode,
        status: response_status,
        headers: response_headers,
    } = config;
    let mut tracker = ResponseBodyTracker::new(mode, guard, &response_headers);
    let mut stream_end =
        stream_response_body(&shutdown, stream, &mut file, &sender, &mut tracker, guard).await;
    if let Err(error) = file.sync_all().await {
        let message = format!("sync response body: {error}");
        let _ = send_downstream(
            &sender,
            &shutdown,
            Err(io::Error::new(error.kind(), message.clone())),
        )
        .await;
        if guard
            .finish(
                Outcome::RecordingFailed,
                Some(ErrorMetadata {
                    kind: ErrorKind::ResponseRecordingFailed,
                    message,
                }),
            )
            .is_err()
        {
            guard.warn_finalization_failed();
        }
    } else {
        let semantic_result = if stream_end.completed_at_ns.is_some() && !tracker.is_event_stream()
        {
            guard.observe_json_response(response_status, &response_headers)
        } else {
            Ok(())
        };
        if let Err(error) = semantic_result {
            stream_end.terminal = response_error(
                Outcome::RecordingFailed,
                ErrorKind::ResponseRecordingFailed,
                error.to_string(),
            );
        }
        if let Some(completed_at_ns) = stream_end.completed_at_ns
            && let Err(error) = guard.mark_timing(|timing| {
                timing.upstream_response_body_completed_at_ns = Some(completed_at_ns);
            })
        {
            stream_end.terminal = response_error(
                Outcome::RecordingFailed,
                ErrorKind::ResponseRecordingFailed,
                error.to_string(),
            );
        }
        if let Err(error) = guard.finish(stream_end.terminal.outcome, stream_end.terminal.error) {
            let message = format!("finalize Request: {error:#}");
            let _ =
                send_downstream(&sender, &shutdown, Err(io::Error::other(message.clone()))).await;
            guard.warn_finalization_failed();
        }
    }
}

pub(super) fn client_closed_terminal(
    tracker: &ResponseBodyTracker,
    guard: &RequestAttempt,
) -> ResponseStreamEnd {
    let family = guard.protocol_summary().family;
    if let Some(at_ns) = tracker.terminal_at_ns(family) {
        return ResponseStreamEnd::completed(at_ns.to_string());
    }
    if encoded_terminal_seen_on_close(tracker, guard) {
        return ResponseStreamEnd::completed(guard.at_ns());
    }
    ResponseStreamEnd::incomplete(response_error(
        Outcome::ClientDisconnected,
        ErrorKind::ClientDisconnected,
        "client disconnected while the upstream response was streaming",
    ))
}

pub(super) fn declared_content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

/// An Agent's normal close immediately after a complete SSE response must not
/// be recorded as a failed client disconnect. A content-coded stream has no
/// incremental indexer, so decode the recorded body prefix and look for a
/// terminal event there.
pub(super) fn encoded_terminal_seen_on_close(
    tracker: &ResponseBodyTracker,
    guard: &RequestAttempt,
) -> bool {
    let Some(coding) = tracker.opaque_coding().filter(|coding| coding.is_encoded()) else {
        return false;
    };
    let family = guard.protocol_summary().family;
    let observation = guard.with_request_path(|directory| {
        let file = crate::foundation::safe_fs::open_real_file(
            &directory.join("response.body"),
            "Upstream Response body",
        )?;
        replay_encoded_sse_prefix(file, coding, guard.request_id().to_string(), family)
    });
    let Ok(Ok(observation)) = observation else {
        return false;
    };
    if observation.terminal_seen {
        let _ = guard.observe_sse_events(&observation.events);
    }
    observation.terminal_seen
}

pub(super) async fn reject_with_body(
    guard: &mut RequestAttempt,
    body: Body,
    shutdown: tokio_util::sync::CancellationToken,
    status: StatusCode,
    message: &str,
    outcome: Outcome,
    kind: ErrorKind,
) -> Response<Body> {
    let request_file = match guard.clone_request_body() {
        Ok(file) => tokio::fs::File::from_std(file),
        Err(error) => return recording_failure(guard, format!("clone request body file: {error}")),
    };
    let mut stream = body.into_data_stream();
    let mut file = request_file;
    loop {
        let next = tokio::select! {
            () = shutdown.cancelled() => {
                return finish_proxy_response(
                    guard,
                    StatusCode::SERVICE_UNAVAILABLE,
                    "AIBox Request Proxy is shutting down",
                    Outcome::ServerShutdown,
                    ErrorKind::ServerShutdown,
                );
            }
            next = stream.next() => next,
        };
        let Some(next) = next else { break };
        let chunk = match next {
            Ok(chunk) => chunk,
            Err(error) => {
                return finish_proxy_response(
                    guard,
                    StatusCode::BAD_REQUEST,
                    &format!("read client request body: {error}"),
                    Outcome::ClientDisconnected,
                    ErrorKind::RequestBodyFailed,
                );
            }
        };
        if let Err(error) = file.write_all(&chunk).await {
            return recording_failure(guard, format!("write Request body: {error}"));
        }
        guard.add_request_bytes(chunk.len());
    }
    if let Err(error) = file.sync_all().await {
        return recording_failure(guard, format!("sync request body: {error}"));
    }
    guard.mark_request_body_finished();
    finish_proxy_response(guard, status, message, outcome, kind)
}

pub(super) fn finish_proxy_response(
    guard: &mut RequestAttempt,
    status: StatusCode,
    message: &str,
    outcome: Outcome,
    kind: ErrorKind,
) -> Response<Body> {
    let body = format!("{message}\n");
    let mut headers = HeaderMap::from_iter([
        (
            header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("text/plain; charset=utf-8"),
        ),
        (
            header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static("no-store"),
        ),
    ]);
    headers.insert(
        header::CONTENT_LENGTH,
        axum::http::HeaderValue::from_str(&body.len().to_string())
            .expect("proxy error body length is a valid header"),
    );
    let finish = guard.finish(
        outcome,
        Some(ErrorMetadata {
            kind,
            message: message.to_string(),
        }),
    );
    if let Err(error) = finish {
        return bare_error(StatusCode::INSUFFICIENT_STORAGE, &error.to_string());
    }
    response_with_headers(status, headers, Body::from(body))
}

pub(super) fn recording_failure(
    guard: &mut RequestAttempt,
    message: impl Into<String>,
) -> Response<Body> {
    let message = message.into();
    let _ = guard.finish(
        Outcome::RecordingFailed,
        Some(ErrorMetadata {
            kind: ErrorKind::RecordingFailed,
            message: message.clone(),
        }),
    );
    bare_error(StatusCode::INSUFFICIENT_STORAGE, &message)
}

pub(super) fn response_with_headers(
    status: StatusCode,
    headers: HeaderMap,
    body: Body,
) -> Response<Body> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

pub(super) fn is_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::CONTENT_TYPE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .any(|media_type| media_type.trim().eq_ignore_ascii_case("text/event-stream"))
}

pub(super) fn response_stream_mode(
    headers: &HeaderMap,
    status: StatusCode,
    protocol: &ProtocolSummary,
) -> ResponseStreamMode {
    if is_event_stream(headers) {
        let recorded = recorded_headers(headers);
        return if matches!(
            body_content_coding(&recorded),
            Ok(BodyContentCoding::Identity)
        ) {
            ResponseStreamMode::EventStream
        } else {
            ResponseStreamMode::OpaqueEventStream
        };
    }
    if !headers.contains_key(header::CONTENT_TYPE)
        && status.is_success()
        && protocol.family != ProtocolFamily::Unknown
        && protocol.response_mode.requested == Some(ResponseModeValue::Stream)
    {
        return ResponseStreamMode::Detect;
    }
    ResponseStreamMode::Normal
}

pub(crate) fn bare_error(status: StatusCode, message: &str) -> Response<Body> {
    let mut response = Response::new(Body::from(format!("{message}\n")));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}
