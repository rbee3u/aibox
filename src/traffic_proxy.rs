use crate::traffic::AppState;
use crate::traffic_interpretation::{ProtocolObserver, ProtocolSummary};
use crate::traffic_store::{
    offset_ns, utc_now, ErrorKind, ErrorMetadata, NewRecord, Outcome, RecordedHeader,
    ResponseMetadata, ResponseSource, RuntimeMeasurements, SummaryHandle, TrafficStore,
    FORMAT_VERSION,
};
use anyhow::Context as _;
use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, Response, StatusCode, Version};
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use serde::Serialize;
use std::collections::HashSet;
use std::io::{self, Write as _};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use url::{Host, Url};

pub(super) async fn handle(state: AppState, request: Request<Body>) -> Response<Body> {
    let (parts, body) = request.into_parts();
    let incoming_uri = parts.uri.to_string();
    let candidate = incoming_uri.strip_prefix('/').unwrap_or_default();
    let parsed = Url::parse(candidate).ok();
    let upstream = parsed
        .as_ref()
        .filter(|url| matches!(url.scheme(), "http" | "https"));
    let host_hint = upstream.and_then(Url::host_str);
    let begin = state.store.begin(
        parts.method.as_str(),
        &incoming_uri,
        upstream.map(Url::as_str),
        version_name(parts.version),
        RecordedHeader::from_headers(&parts.headers),
        host_hint,
    );
    let (record, request_metadata) = match begin {
        Ok(value) => value,
        Err(error) => return bare_error(StatusCode::INSUFFICIENT_STORAGE, &error.to_string()),
    };
    let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
    let protocol = Arc::new(Mutex::new(ProtocolObserver::new(
        request_metadata.upstream_url.as_deref(),
    )));
    let mut guard = RecordGuard::new(
        state.store.clone(),
        record,
        measurements.clone(),
        protocol.clone(),
        Instant::now(),
    );

    if parts.method == Method::CONNECT {
        return reject_with_body(
            &mut guard,
            body,
            state.shutdown.clone(),
            StatusCode::METHOD_NOT_ALLOWED,
            "CONNECT is not supported by aibox Traffic",
            Outcome::Rejected,
            ErrorKind::ConnectNotSupported,
        )
        .await;
    }
    if is_upgrade(&parts.headers) {
        return reject_with_body(
            &mut guard,
            body,
            state.shutdown.clone(),
            StatusCode::UPGRADE_REQUIRED,
            "Upgrade and WebSocket traffic are not supported by aibox Traffic",
            Outcome::Rejected,
            ErrorKind::UpgradeNotSupported,
        )
        .await;
    }
    let Some(url) = upstream.cloned() else {
        return reject_with_body(
            &mut guard,
            body,
            state.shutdown.clone(),
            StatusCode::BAD_REQUEST,
            "proxy path must contain an absolute http:// or https:// target URL",
            Outcome::Rejected,
            ErrorKind::InvalidTargetUrl,
        )
        .await;
    };

    let resolved = tokio::select! {
        _ = state.shutdown.cancelled() => {
            return finish_proxy_response(
                &mut guard,
                StatusCode::SERVICE_UNAVAILABLE,
                "aibox Traffic is shutting down",
                Outcome::ServerShutdown,
                ErrorKind::ServerShutdown,
            );
        }
        result = validate_and_resolve(&url, state.allow_private_upstream) => result,
    };
    let resolved = match resolved {
        Ok(addresses) => addresses,
        Err(TargetError::Rejected(message)) => {
            return reject_with_body(
                &mut guard,
                body,
                state.shutdown.clone(),
                StatusCode::FORBIDDEN,
                &message,
                Outcome::Rejected,
                ErrorKind::NonPublicTarget,
            )
            .await;
        }
        Err(TargetError::Upstream(message)) => {
            return reject_with_body(
                &mut guard,
                body,
                state.shutdown.clone(),
                StatusCode::BAD_GATEWAY,
                &message,
                Outcome::UpstreamError,
                ErrorKind::DnsError,
            )
            .await;
        }
    };
    let client = match build_client(&url, &resolved) {
        Ok(client) => client,
        Err(error) => {
            return reject_with_body(
                &mut guard,
                body,
                state.shutdown.clone(),
                StatusCode::BAD_GATEWAY,
                &error.to_string(),
                Outcome::UpstreamError,
                ErrorKind::ClientConfiguration,
            )
            .await;
        }
    };

    let request_file = match guard.record.request_body.try_clone() {
        Ok(file) => tokio::fs::File::from_std(file),
        Err(error) => {
            return recording_failure(&mut guard, format!("clone request body file: {error}"));
        }
    };
    let request_error = Arc::new(Mutex::new(None::<String>));
    let request_stream = recorded_request_stream_with_summary(
        body,
        request_file,
        RequestStreamContext {
            measurements: measurements.clone(),
            error_slot: request_error.clone(),
            summary: guard.record.summary.clone(),
            protocol,
            request_headers: request_metadata.headers,
            store: Some(guard.store.clone()),
            directory: guard.record.directory.clone(),
            origin: guard.record.origin,
            shutdown: state.shutdown.clone(),
        },
    );
    if let Err(error) = guard.mark_timing(|timing| {
        timing.upstream_request_started_at_ns = Some(offset_ns(guard.record.origin));
    }) {
        return recording_failure(&mut guard, format!("checkpoint request timing: {error:#}"));
    }
    let headers = forwarded_headers(&parts.headers);
    let upstream_request = client
        .request(parts.method.clone(), url)
        .headers(headers)
        .body(reqwest::Body::wrap_stream(request_stream));

    let upstream_response = tokio::select! {
        _ = state.shutdown.cancelled() => {
            return finish_proxy_response(
                &mut guard,
                StatusCode::SERVICE_UNAVAILABLE,
                "aibox Traffic is shutting down",
                Outcome::ServerShutdown,
                ErrorKind::ServerShutdown,
            );
        }
        result = upstream_request.send() => result,
    };
    let upstream_response = match upstream_response {
        Ok(response) => response,
        Err(error) => {
            let recording = request_error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if let Some(message) = recording {
                return finish_proxy_response(
                    &mut guard,
                    StatusCode::INSUFFICIENT_STORAGE,
                    &message,
                    Outcome::RecordingFailed,
                    ErrorKind::RequestRecordingFailed,
                );
            }
            let status = if error.is_timeout() {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::BAD_GATEWAY
            };
            let kind = if error.is_timeout() {
                ErrorKind::ConnectTimeout
            } else {
                ErrorKind::UpstreamRequestFailed
            };
            return finish_proxy_response(
                &mut guard,
                status,
                &format!("upstream request failed: {error}"),
                Outcome::UpstreamError,
                kind,
            );
        }
    };

    let status = upstream_response.status();
    let version = upstream_response.version();
    let original_headers = upstream_response.headers().clone();
    let is_sse = is_event_stream(&original_headers);
    let metadata = ResponseMetadata {
        format_version: FORMAT_VERSION,
        source: ResponseSource::Upstream,
        headers_at: utc_now(),
        status: status.as_u16(),
        http_version: version_name(version).to_string(),
        headers: RecordedHeader::from_headers(&original_headers),
    };
    if let Err(error) = guard.observe_response_headers(&metadata.headers, is_sse) {
        return recording_failure(
            &mut guard,
            format!("checkpoint response metadata: {error:#}"),
        );
    }
    if let Err(error) = state
        .store
        .write_response(&guard.record.directory, &metadata)
    {
        return recording_failure(&mut guard, format!("record response metadata: {error:#}"));
    }
    let response_file = match guard.record.response_body.try_clone() {
        Ok(file) => tokio::fs::File::from_std(file),
        Err(error) => {
            return recording_failure(&mut guard, format!("clone response body file: {error}"));
        }
    };

    let (sender, receiver) = mpsc::channel::<Result<Bytes, io::Error>>(8);
    let state_for_task = state.clone();
    let event_index = if is_sse {
        match state.store.create_event_index(&guard.record) {
            Ok(file) => Some(file),
            Err(error) => {
                guard.add_warning("event_index_failed", error.to_string());
                None
            }
        }
    } else {
        None
    };
    state.response_tasks.spawn(async move {
        record_response_stream_with_index(
            state_for_task.shutdown.clone(),
            upstream_response.bytes_stream(),
            response_file,
            sender,
            ResponseStreamConfig {
                is_sse,
                status: status.as_u16(),
                event_index,
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

struct RecordGuard {
    store: TrafficStore,
    record: NewRecord,
    measurements: Arc<Mutex<RuntimeMeasurements>>,
    protocol: Arc<Mutex<ProtocolObserver>>,
    finished: bool,
}

impl RecordGuard {
    fn new(
        store: TrafficStore,
        record: NewRecord,
        measurements: Arc<Mutex<RuntimeMeasurements>>,
        protocol: Arc<Mutex<ProtocolObserver>>,
        _legacy_started: Instant,
    ) -> Self {
        Self {
            store,
            record,
            measurements,
            protocol,
            finished: false,
        }
    }

    fn mark_timing(
        &self,
        update: impl FnOnce(&mut crate::traffic_store::TimingMetadata),
    ) -> anyhow::Result<()> {
        self.store
            .update_summary(&self.record.directory, &self.record.summary, |summary| {
                update(&mut summary.timing);
                true
            })?;
        Ok(())
    }

    fn observe_response_headers(
        &self,
        headers: &[RecordedHeader],
        event_stream: bool,
    ) -> anyhow::Result<()> {
        let at_ns = offset_ns(self.record.origin);
        let mut observer = self
            .protocol
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        observer.observe_response_headers(headers, event_stream, at_ns.clone());
        let protocol = observer.snapshot();
        self.store
            .update_summary(&self.record.directory, &self.record.summary, |summary| {
                summary.timing.upstream_response_headers_at_ns = Some(at_ns);
                summary.protocol = Some(protocol);
                true
            })?;
        Ok(())
    }

    fn observe_sse_events(&self, events: &[(Vec<u8>, String)]) -> anyhow::Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let mut observer = self
            .protocol
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = events.iter().fold(false, |changed, (data, at_ns)| {
            observer.observe_sse_data(data, at_ns.clone()) | changed
        });
        if changed {
            self.publish_protocol(observer.snapshot())?;
        }
        Ok(())
    }

    fn observe_json_response(&self, status: u16) -> anyhow::Result<()> {
        let at_ns = offset_ns(self.record.origin);
        let mut observer = self
            .protocol
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        observer.observe_json_response(&self.record.directory.join("response.body"), status, at_ns);
        self.publish_protocol(observer.snapshot())
    }

    fn observe_http_status(&self, status: u16) -> anyhow::Result<()> {
        let mut observer = self
            .protocol
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if observer.observe_http_status(status, offset_ns(self.record.origin)) {
            self.publish_protocol(observer.snapshot())?;
        }
        Ok(())
    }

    fn publish_protocol(&self, protocol: ProtocolSummary) -> anyhow::Result<()> {
        self.store
            .update_summary(&self.record.directory, &self.record.summary, |summary| {
                if summary.protocol.as_ref() == Some(&protocol) {
                    return false;
                }
                summary.protocol = Some(protocol);
                true
            })?;
        Ok(())
    }

    fn add_warning(&self, kind: &str, message: String) {
        let result =
            self.store
                .update_summary(&self.record.directory, &self.record.summary, |summary| {
                    summary
                        .warnings
                        .push(crate::traffic_store::DiagnosticMetadata {
                            phase: "recording".to_string(),
                            kind: kind.to_string(),
                            message,
                            at_ns: offset_ns(self.record.origin),
                        });
                    true
                });
        if let Err(error) = result {
            eprintln!("warning: cannot checkpoint Traffic summary: {error:#}");
        }
    }

    fn finish(&mut self, outcome: Outcome, error: Option<ErrorMetadata>) -> anyhow::Result<()> {
        let values = self
            .measurements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        self.record.request_body.sync_all().ok();
        self.record.response_body.sync_all().ok();
        let result = self
            .store
            .finish(&self.record, self.record.origin, &values, outcome, error);
        if result.is_ok() {
            self.finished = true;
        }
        result.map(|_| ())
    }
}

impl Drop for RecordGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let _ = self.finish(
            Outcome::ClientDisconnected,
            Some(ErrorMetadata {
                kind: ErrorKind::ClientDisconnected,
                message: "client disconnected before the proxy attempt completed".to_string(),
            }),
        );
        self.store.abandon_active(&self.record.id);
    }
}

struct RequestStreamContext {
    measurements: Arc<Mutex<RuntimeMeasurements>>,
    error_slot: Arc<Mutex<Option<String>>>,
    summary: SummaryHandle,
    protocol: Arc<Mutex<ProtocolObserver>>,
    request_headers: Vec<RecordedHeader>,
    store: Option<TrafficStore>,
    directory: std::path::PathBuf,
    origin: Instant,
    shutdown: tokio_util::sync::CancellationToken,
}

fn recorded_request_stream_with_summary(
    body: Body,
    mut file: tokio::fs::File,
    context: RequestStreamContext,
) -> impl futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let RequestStreamContext {
        measurements,
        error_slot,
        summary,
        protocol,
        request_headers,
        store,
        directory,
        origin,
        shutdown,
    } = context;
    let mut stream = body.into_data_stream();
    async_stream::stream! {
        let mut body_complete = false;
        loop {
            let next = tokio::select! {
                _ = shutdown.cancelled() => {
                    let error = io::Error::new(io::ErrorKind::Interrupted, "Traffic Proxy is shutting down");
                    *error_slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
                    yield Err(error);
                    break;
                }
                next = stream.next() => next,
            };
            let Some(chunk) = next else {
                body_complete = true;
                break;
            };
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    let error = io::Error::new(io::ErrorKind::UnexpectedEof, error.to_string());
                    *error_slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
                    yield Err(error);
                    break;
                }
            };
            if let Err(error) = file.write_all(&chunk).await {
                *error_slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
                yield Err(error);
                break;
            }
            if let Err(error) = file.flush().await {
                *error_slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
                yield Err(error);
                break;
            }
            {
                let mut values = measurements.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                values.request_bytes = values.request_bytes.saturating_add(chunk.len() as u64);
            }
            let at_ns = offset_ns(origin);
            let first_request_byte = checkpoint_summary_update(
                store.as_ref(),
                &directory,
                &summary,
                |value| {
                    if value.timing.upstream_request_body_first_byte_at_ns.is_some() {
                        return false;
                    }
                    value.timing.upstream_request_body_first_byte_at_ns = Some(at_ns);
                    true
                },
            );
            if let Err(error) = first_request_byte {
                *error_slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
                yield Err(io::Error::other(error.to_string()));
                break;
            }
            yield Ok(chunk);
        }
        match file.sync_all().await {
            Ok(()) if body_complete => {
                measurements.lock().unwrap_or_else(std::sync::PoisonError::into_inner).request_body_duration = Some(origin.elapsed());
                let at_ns = offset_ns(origin);
                let checkpoint = checkpoint_request_complete(
                    store.as_ref(),
                    &directory,
                    &summary,
                    &protocol,
                    &request_headers,
                    at_ns,
                );
                if let Err(error) = checkpoint {
                    *error_slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
                    yield Err(io::Error::other(error.to_string()));
                }
            }
            Ok(()) => {}
            Err(error) => {
                *error_slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
                if body_complete {
                    yield Err(error);
                }
            }
        }
    }
}

fn checkpoint_request_complete(
    store: Option<&TrafficStore>,
    directory: &std::path::Path,
    summary: &SummaryHandle,
    protocol: &Mutex<ProtocolObserver>,
    request_headers: &[RecordedHeader],
    at_ns: String,
) -> anyhow::Result<bool> {
    let mut observer = protocol
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    observer.observe_request(
        &directory.join("request.body"),
        request_headers,
        at_ns.clone(),
    );
    let protocol_snapshot = observer.snapshot();
    checkpoint_summary_update(store, directory, summary, |value| {
        value.timing.upstream_request_body_completed_at_ns = Some(at_ns);
        value.protocol = Some(protocol_snapshot);
        true
    })
}

// Kept as a small test helper for deterministic body-stream tests that do not
// have a Traffic Record directory to checkpoint.
#[cfg(test)]
fn recorded_request_stream(
    body: Body,
    file: tokio::fs::File,
    measurements: Arc<Mutex<RuntimeMeasurements>>,
    error_slot: Arc<Mutex<Option<String>>>,
    started: Instant,
    shutdown: tokio_util::sync::CancellationToken,
) -> impl futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let summary = SummaryHandle::new(crate::traffic_store::SummaryMetadata {
        schema_version: FORMAT_VERSION,
        record_id: String::new(),
        kind: "summary".to_string(),
        observed_at: utc_now(),
        terminal: false,
        timing: crate::traffic_store::TimingMetadata::default(),
        protocol: Some(ProtocolSummary::default()),
        outcome: None,
        errors: Vec::new(),
        warnings: Vec::new(),
    });
    recorded_request_stream_with_summary(
        body,
        file,
        RequestStreamContext {
            measurements,
            error_slot,
            summary,
            protocol: Arc::new(Mutex::new(ProtocolObserver::new(None))),
            request_headers: Vec::new(),
            store: None,
            directory: std::path::PathBuf::new(),
            origin: started,
            shutdown,
        },
    )
}

fn checkpoint_summary_update(
    store: Option<&TrafficStore>,
    directory: &std::path::Path,
    summary: &SummaryHandle,
    update: impl FnOnce(&mut crate::traffic_store::SummaryMetadata) -> bool,
) -> anyhow::Result<bool> {
    match store {
        Some(store) => store.update_summary(directory, summary, update),
        None => Ok(summary.update(update)),
    }
}

#[cfg(test)]
fn record_response_stream(
    shutdown: tokio_util::sync::CancellationToken,
    stream: impl futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    file: tokio::fs::File,
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
    guard: &mut RecordGuard,
) -> impl std::future::Future<Output = ()> + '_ {
    record_response_stream_with_index(
        shutdown,
        stream,
        file,
        sender,
        ResponseStreamConfig {
            is_sse: false,
            status: 200,
            event_index: None,
        },
        guard,
    )
}

struct ResponseStreamConfig {
    is_sse: bool,
    status: u16,
    event_index: Option<std::fs::File>,
}

async fn record_response_stream_with_index(
    shutdown: tokio_util::sync::CancellationToken,
    stream: impl futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    mut file: tokio::fs::File,
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
    config: ResponseStreamConfig,
    guard: &mut RecordGuard,
) {
    let ResponseStreamConfig {
        is_sse,
        status: response_status,
        event_index,
    } = config;
    let mut stream = Box::pin(stream);
    let mut indexer = is_sse.then(|| SseIndexer::new(event_index, guard.record.id.clone()));
    let mut response_completed = false;
    let mut terminal = loop {
        let next = tokio::select! {
            biased;
            _ = shutdown.cancelled() => {
                break (
                    Outcome::ServerShutdown,
                    Some(ErrorMetadata { kind: ErrorKind::ServerShutdown, message: "Traffic Proxy stopped while the response was streaming".to_string() })
                );
            }
            // Prefer an already-ready upstream EOF when the client closes at
            // the same time. This avoids turning a normal response into a
            // disconnect solely because the downstream body was dropped first.
            next = stream.try_next() => next,
            _ = sender.closed() => {
                break client_closed_terminal(indexer.as_ref());
            }
        };
        match next {
            Ok(Some(chunk)) => {
                if guard.record.summary.read(|summary| {
                    summary
                        .timing
                        .upstream_response_body_first_byte_at_ns
                        .is_none()
                }) {
                    if let Err(error) = guard.mark_timing(|timing| {
                        timing.upstream_response_body_first_byte_at_ns =
                            Some(offset_ns(guard.record.origin))
                    }) {
                        let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
                        break (
                            Outcome::RecordingFailed,
                            Some(ErrorMetadata {
                                kind: ErrorKind::ResponseRecordingFailed,
                                message: error.to_string(),
                            }),
                        );
                    }
                }
                if let Err(error) = file.write_all(&chunk).await {
                    let message = format!("record response body: {error}");
                    let _ = sender
                        .send(Err(io::Error::new(error.kind(), message.clone())))
                        .await;
                    break (
                        Outcome::RecordingFailed,
                        Some(ErrorMetadata {
                            kind: ErrorKind::ResponseRecordingFailed,
                            message,
                        }),
                    );
                }
                if let Err(error) = file.flush().await {
                    let message = format!("flush response body: {error}");
                    let _ = sender
                        .send(Err(io::Error::new(error.kind(), message.clone())))
                        .await;
                    break (
                        Outcome::RecordingFailed,
                        Some(ErrorMetadata {
                            kind: ErrorKind::ResponseRecordingFailed,
                            message,
                        }),
                    );
                }
                {
                    let mut values = guard
                        .measurements
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    values.response_bytes =
                        values.response_bytes.saturating_add(chunk.len() as u64);
                }
                if let Some(indexer) = indexer.as_mut() {
                    let body_offset = indexer.body_offset;
                    let feed = indexer.feed(&chunk, body_offset, offset_ns(guard.record.origin));
                    if let Err(error) = feed {
                        guard.add_warning("event_index_failed", error.to_string());
                        indexer.disable_indexing();
                    }
                    let events = indexer.take_protocol_events();
                    if let Err(error) = guard.observe_sse_events(&events) {
                        let _ = sender.send(Err(io::Error::other(error.to_string()))).await;
                        break (
                            Outcome::RecordingFailed,
                            Some(ErrorMetadata {
                                kind: ErrorKind::ResponseRecordingFailed,
                                message: error.to_string(),
                            }),
                        );
                    }
                }
                if sender.send(Ok(chunk)).await.is_err() {
                    break client_closed_terminal(indexer.as_ref());
                }
            }
            Ok(None) => {
                if let Some(indexer) = indexer.as_mut() {
                    match indexer.finish() {
                        Ok(true) => guard.add_warning(
                            "event_index_failed",
                            "truncated SSE event was not indexed".to_string(),
                        ),
                        Ok(false) => {}
                        Err(error) => guard.add_warning("event_index_failed", error.to_string()),
                    }
                    let events = indexer.take_protocol_events();
                    if let Err(error) = guard.observe_sse_events(&events) {
                        break (
                            Outcome::RecordingFailed,
                            Some(ErrorMetadata {
                                kind: ErrorKind::ResponseRecordingFailed,
                                message: error.to_string(),
                            }),
                        );
                    }
                }
                response_completed = true;
                break (Outcome::Completed, None);
            }
            Err(error) => {
                let message = format!("upstream response stream failed: {error}");
                let _ = sender
                    .send(Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        message.clone(),
                    )))
                    .await;
                break (
                    Outcome::UpstreamError,
                    Some(ErrorMetadata {
                        kind: ErrorKind::UpstreamResponseFailed,
                        message,
                    }),
                );
            }
        }
    };
    if let Err(error) = file.sync_all().await {
        let message = format!("sync response body: {error}");
        let _ = sender
            .send(Err(io::Error::new(error.kind(), message.clone())))
            .await;
        if let Err(finish_error) = guard.finish(
            Outcome::RecordingFailed,
            Some(ErrorMetadata {
                kind: ErrorKind::ResponseRecordingFailed,
                message,
            }),
        ) {
            eprintln!("warning: cannot finalize failed Traffic Record: {finish_error:#}");
        }
    } else {
        let semantic_result = if response_completed && !is_sse {
            guard.observe_json_response(response_status)
        } else {
            guard.observe_http_status(response_status)
        };
        if let Err(error) = semantic_result {
            terminal = (
                Outcome::RecordingFailed,
                Some(ErrorMetadata {
                    kind: ErrorKind::ResponseRecordingFailed,
                    message: error.to_string(),
                }),
            );
        }
        if response_completed {
            if let Err(error) = guard.mark_timing(|timing| {
                timing.upstream_response_body_completed_at_ns = Some(offset_ns(guard.record.origin))
            }) {
                terminal = (
                    Outcome::RecordingFailed,
                    Some(ErrorMetadata {
                        kind: ErrorKind::ResponseRecordingFailed,
                        message: error.to_string(),
                    }),
                );
            }
        }
        if let Err(error) = guard.finish(terminal.0, terminal.1) {
            let message = format!("finalize Traffic Record: {error:#}");
            let _ = sender.send(Err(io::Error::other(message.clone()))).await;
            eprintln!("warning: {message}");
        }
    }
}

fn client_closed_terminal(indexer: Option<&SseIndexer>) -> (Outcome, Option<ErrorMetadata>) {
    if indexer.is_some_and(SseIndexer::terminal_seen) {
        return (Outcome::Completed, None);
    }
    (
        Outcome::ClientDisconnected,
        Some(ErrorMetadata {
            kind: ErrorKind::ClientDisconnected,
            message: "client disconnected while the upstream response was streaming".to_string(),
        }),
    )
}

async fn reject_with_body(
    guard: &mut RecordGuard,
    body: Body,
    shutdown: tokio_util::sync::CancellationToken,
    status: StatusCode,
    message: &str,
    outcome: Outcome,
    kind: ErrorKind,
) -> Response<Body> {
    let request_file = match guard.record.request_body.try_clone() {
        Ok(file) => tokio::fs::File::from_std(file),
        Err(error) => return recording_failure(guard, format!("clone request body file: {error}")),
    };
    let mut stream = body.into_data_stream();
    let mut file = request_file;
    loop {
        let next = tokio::select! {
            _ = shutdown.cancelled() => {
                return finish_proxy_response(
                    guard,
                    StatusCode::SERVICE_UNAVAILABLE,
                    "aibox Traffic is shutting down",
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
            return recording_failure(guard, format!("record request body: {error}"));
        }
        let mut values = guard
            .measurements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        values.request_bytes = values.request_bytes.saturating_add(chunk.len() as u64);
    }
    if let Err(error) = file.sync_all().await {
        return recording_failure(guard, format!("sync request body: {error}"));
    }
    guard
        .measurements
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .request_body_duration = Some(guard.record.origin.elapsed());
    finish_proxy_response(guard, status, message, outcome, kind)
}

fn finish_proxy_response(
    guard: &mut RecordGuard,
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

fn recording_failure(guard: &mut RecordGuard, message: String) -> Response<Body> {
    let _ = guard.finish(
        Outcome::RecordingFailed,
        Some(ErrorMetadata {
            kind: ErrorKind::RecordingFailed,
            message: message.clone(),
        }),
    );
    bare_error(StatusCode::INSUFFICIENT_STORAGE, &message)
}

fn response_with_headers(status: StatusCode, headers: HeaderMap, body: Body) -> Response<Body> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

fn is_event_stream(headers: &HeaderMap) -> bool {
    headers
        .get_all(header::CONTENT_TYPE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter_map(|value| value.split(';').next())
        .any(|media_type| media_type.trim().eq_ignore_ascii_case("text/event-stream"))
}

#[derive(Serialize)]
struct SseEventIndexEntry {
    schema_version: u32,
    record_id: String,
    kind: String,
    sequence: u64,
    body_start: u64,
    body_end: u64,
    first_arrival_at_ns: String,
    completed_at_ns: String,
}

struct SseIndexer {
    file: Option<std::fs::File>,
    record_id: String,
    buffer: Vec<u8>,
    buffer_start: u64,
    body_offset: u64,
    event_start: Option<u64>,
    first_arrival_at_ns: Option<String>,
    data_seen: bool,
    event_name: Option<Vec<u8>>,
    data: Vec<u8>,
    protocol_events: Vec<(Vec<u8>, String)>,
    terminal_seen: bool,
    sequence: u64,
    indexing_disabled: bool,
    last_arrival_at_ns: String,
}

impl SseIndexer {
    fn new(file: Option<std::fs::File>, record_id: String) -> Self {
        Self {
            file,
            record_id,
            buffer: Vec::new(),
            buffer_start: 0,
            body_offset: 0,
            event_start: None,
            first_arrival_at_ns: None,
            data_seen: false,
            event_name: None,
            data: Vec::new(),
            protocol_events: Vec::new(),
            terminal_seen: false,
            sequence: 0,
            indexing_disabled: false,
            last_arrival_at_ns: "0".to_string(),
        }
    }

    fn disable_indexing(&mut self) {
        self.indexing_disabled = true;
    }

    fn terminal_seen(&self) -> bool {
        self.terminal_seen
    }

    fn take_protocol_events(&mut self) -> Vec<(Vec<u8>, String)> {
        std::mem::take(&mut self.protocol_events)
    }

    fn feed(&mut self, chunk: &[u8], body_start: u64, at_ns: String) -> anyhow::Result<()> {
        let contiguous = body_start == self.body_offset;
        if !contiguous {
            self.indexing_disabled = true;
        }
        self.body_offset = self.body_offset.saturating_add(chunk.len() as u64);
        self.last_arrival_at_ns = at_ns.clone();
        if self.event_start.is_none() && !chunk.is_empty() {
            self.event_start = Some(body_start);
            self.first_arrival_at_ns = Some(at_ns.clone());
        }
        self.buffer.extend_from_slice(chunk);
        if self.buffer_start == 0 && self.buffer.starts_with(&[0xef, 0xbb, 0xbf]) {
            self.buffer.drain(..3);
            self.buffer_start = 3;
            if self.buffer.is_empty() {
                self.event_start = None;
                self.first_arrival_at_ns = None;
            } else {
                self.event_start = Some(3);
                self.first_arrival_at_ns = Some(at_ns.clone());
            }
        }
        self.process(at_ns, false)?;
        if !contiguous {
            return Err(anyhow::anyhow!("SSE body offsets are not contiguous"));
        }
        Ok(())
    }

    fn process(&mut self, at_ns: String, final_input: bool) -> anyhow::Result<()> {
        let mut consumed = 0usize;
        while let Some((line_end, separator_len)) =
            find_sse_line_end(&self.buffer[consumed..], final_input)
        {
            let line_end = consumed + line_end;
            let line = &self.buffer[consumed..line_end];
            let absolute_end = self.buffer_start + line_end as u64 + separator_len as u64;
            if self.event_start.is_none() && !line.is_empty() {
                self.event_start = Some(self.buffer_start + consumed as u64);
                self.first_arrival_at_ns = Some(at_ns.clone());
            }
            if line.is_empty() {
                if is_terminal_sse_event(self.event_name.as_deref(), &self.data) {
                    self.terminal_seen = true;
                }
                if self.data_seen {
                    self.protocol_events
                        .push((self.data.clone(), at_ns.clone()));
                }
                if self.data_seen && !self.indexing_disabled {
                    if let Some(file) = self.file.as_mut() {
                        let entry = SseEventIndexEntry {
                            schema_version: FORMAT_VERSION,
                            record_id: self.record_id.clone(),
                            kind: "sse_event".to_string(),
                            sequence: self.sequence,
                            body_start: self.event_start.unwrap_or(self.buffer_start),
                            body_end: absolute_end,
                            first_arrival_at_ns: self
                                .first_arrival_at_ns
                                .clone()
                                .unwrap_or_else(|| at_ns.clone()),
                            completed_at_ns: at_ns.clone(),
                        };
                        serde_json::to_writer(&mut *file, &entry)?;
                        file.write_all(b"\n")?;
                        file.flush()?;
                        self.sequence = self.sequence.saturating_add(1);
                    }
                }
                self.event_start = None;
                self.first_arrival_at_ns = None;
                self.data_seen = false;
                self.event_name = None;
                self.data.clear();
            } else if let Some(value) = sse_field_value(line, b"event") {
                self.event_name = Some(value.to_vec());
            } else if let Some(value) = sse_field_value(line, b"data") {
                if self.data_seen {
                    self.data.push(b'\n');
                }
                self.data.extend_from_slice(value);
                self.data_seen = true;
            }
            consumed = line_end + separator_len;
        }
        if consumed > 0 {
            self.buffer.drain(..consumed);
            self.buffer_start += consumed as u64;
        }
        Ok(())
    }

    fn finish(&mut self) -> anyhow::Result<bool> {
        self.process(self.last_arrival_at_ns.clone(), true)?;
        if self.indexing_disabled {
            return Ok(false);
        }
        if let Some(file) = self.file.as_mut() {
            file.sync_all()?;
        }
        Ok(self.event_start.is_some() || !self.buffer.is_empty())
    }
}

fn sse_field_value<'a>(line: &'a [u8], field: &[u8]) -> Option<&'a [u8]> {
    if line == field {
        return Some(&[]);
    }
    let value = line.strip_prefix(field)?.strip_prefix(b":")?;
    Some(value.strip_prefix(b" ").unwrap_or(value))
}

fn is_terminal_sse_event(event_name: Option<&[u8]>, data: &[u8]) -> bool {
    if matches!(
        event_name,
        Some(
            b"message_stop"
                | b"response.completed"
                | b"response.failed"
                | b"response.incomplete"
                | b"response.cancelled"
        )
    ) {
        return true;
    }

    let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) else {
        return false;
    };
    let Some(kind) = value.get("type").and_then(serde_json::Value::as_str) else {
        return false;
    };
    if matches!(
        kind,
        "message_stop"
            | "response.completed"
            | "response.failed"
            | "response.incomplete"
            | "response.cancelled"
    ) {
        return true;
    }
    kind == "message_delta"
        && value
            .get("delta")
            .and_then(|delta| delta.get("stop_reason"))
            .is_some_and(|stop_reason| !stop_reason.is_null())
}

fn find_sse_line_end(bytes: &[u8], final_input: bool) -> Option<(usize, usize)> {
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'\n' => return Some((index, 1)),
            b'\r' => {
                if index + 1 == bytes.len() {
                    return final_input.then_some((index, 1));
                }
                return Some((index, usize::from(bytes[index + 1] == b'\n') + 1));
            }
            _ => {}
        }
    }
    None
}

pub(super) fn bare_error(status: StatusCode, message: &str) -> Response<Body> {
    let mut response = Response::new(Body::from(format!("{message}\n")));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

fn forwarded_headers(headers: &HeaderMap) -> HeaderMap {
    let mut remove: HashSet<String> = [
        "host",
        "connection",
        "proxy-connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ]
    .map(str::to_string)
    .into_iter()
    .collect();
    for value in headers.get_all(header::CONNECTION) {
        if let Ok(value) = value.to_str() {
            remove.extend(
                value
                    .split(',')
                    .map(|token| token.trim().to_ascii_lowercase()),
            );
        }
    }
    let mut forwarded = HeaderMap::new();
    for (name, value) in headers {
        if !remove.contains(name.as_str()) {
            forwarded.append(name.clone(), value.clone());
        }
    }
    forwarded
}

fn is_upgrade(headers: &HeaderMap) -> bool {
    headers.contains_key(header::UPGRADE)
        || headers.get_all(header::CONNECTION).iter().any(|value| {
            value.to_str().is_ok_and(|value| {
                value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
            })
        })
}

enum TargetError {
    Rejected(String),
    Upstream(String),
}

async fn validate_and_resolve(
    url: &Url,
    allow_private: bool,
) -> Result<Vec<SocketAddr>, TargetError> {
    let host = url
        .host_str()
        .ok_or_else(|| TargetError::Rejected("target URL has no host".to_string()))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| TargetError::Rejected("target URL has no usable port".to_string()))?;
    let mut addresses: Vec<_> = match url.host() {
        Some(Host::Ipv4(address)) => vec![SocketAddr::new(IpAddr::V4(address), port)],
        Some(Host::Ipv6(address)) => vec![SocketAddr::new(IpAddr::V6(address), port)],
        Some(Host::Domain(domain)) => tokio::net::lookup_host((domain, port))
            .await
            .map_err(|error| {
                TargetError::Upstream(format!("resolve upstream host {host}: {error}"))
            })?
            .collect(),
        None => return Err(TargetError::Rejected("target URL has no host".to_string())),
    };
    addresses.sort();
    addresses.dedup();
    if addresses.is_empty() {
        return Err(TargetError::Upstream(format!(
            "upstream host {host} resolved to no addresses"
        )));
    }
    require_allowed_addresses(host, &addresses, allow_private)?;
    Ok(addresses)
}

fn require_allowed_addresses(
    host: &str,
    addresses: &[SocketAddr],
    allow_private: bool,
) -> Result<(), TargetError> {
    if !allow_private
        && addresses
            .iter()
            .any(|address| !is_allowed_upstream_ip(address.ip()))
    {
        return Err(TargetError::Rejected(format!(
            "upstream host {host} resolved to a non-public address"
        )));
    }
    Ok(())
}

fn build_client(url: &Url, addresses: &[SocketAddr]) -> anyhow::Result<reqwest::Client> {
    let host = url.host_str().context("target URL has no host")?;
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .referer(false);
    if matches!(url.host(), Some(Host::Domain(_))) {
        builder = builder.resolve_to_addrs(host, addresses);
    }
    Ok(builder.build()?)
}

fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_v4(address),
        IpAddr::V6(address) => is_public_v6(address),
    }
}

fn is_allowed_upstream_ip(address: IpAddr) -> bool {
    is_public_ip(address) || is_fake_ip_v4(address)
}

fn is_fake_ip_v4(address: IpAddr) -> bool {
    let address = match address {
        IpAddr::V4(address) => address,
        IpAddr::V6(address) => match address.to_ipv4_mapped() {
            Some(address) => address,
            None => return false,
        },
    };
    matches_prefix(u32::from(address), 0xc612_0000, 15)
}

fn is_public_v4(address: Ipv4Addr) -> bool {
    let value = u32::from(address);
    !matches_prefix(value, 0x0000_0000, 8)
        && !matches_prefix(value, 0x0a00_0000, 8)
        && !matches_prefix(value, 0x6440_0000, 10)
        && !matches_prefix(value, 0x7f00_0000, 8)
        && !matches_prefix(value, 0xa9fe_0000, 16)
        && !matches_prefix(value, 0xac10_0000, 12)
        && !matches_prefix(value, 0xc000_0000, 24)
        && !matches_prefix(value, 0xc000_0200, 24)
        && !matches_prefix(value, 0xc058_6300, 24)
        && !matches_prefix(value, 0xc0a8_0000, 16)
        && !matches_prefix(value, 0xc612_0000, 15)
        && !matches_prefix(value, 0xc633_6400, 24)
        && !matches_prefix(value, 0xcb00_7100, 24)
        && !matches_prefix(value, 0xe000_0000, 4)
        && !matches_prefix(value, 0xf000_0000, 4)
}

fn is_public_v6(address: Ipv6Addr) -> bool {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return is_public_v4(mapped);
    }
    let value = u128::from(address);
    matches_prefix_v6(value, 0x2000_0000_0000_0000_0000_0000_0000_0000, 3)
        && address != Ipv6Addr::UNSPECIFIED
        && address != Ipv6Addr::LOCALHOST
        && !matches_prefix_v6(value, 0x0064_ff9b_0001_0000_0000_0000_0000_0000, 48)
        && !matches_prefix_v6(value, 0x0100_0000_0000_0000_0000_0000_0000_0000, 64)
        && !matches_prefix_v6(value, 0x2001_0000_0000_0000_0000_0000_0000_0000, 23)
        && !matches_prefix_v6(value, 0x2001_0db8_0000_0000_0000_0000_0000_0000, 32)
        && !matches_prefix_v6(value, 0x3fff_0000_0000_0000_0000_0000_0000_0000, 20)
        && !matches_prefix_v6(value, 0xfc00_0000_0000_0000_0000_0000_0000_0000, 7)
        && !matches_prefix_v6(value, 0xfe80_0000_0000_0000_0000_0000_0000_0000, 10)
        && !matches_prefix_v6(value, 0xff00_0000_0000_0000_0000_0000_0000_0000, 8)
}

fn matches_prefix(value: u32, network: u32, bits: u32) -> bool {
    value & (!0_u32 << (32 - bits)) == network
}

fn matches_prefix_v6(value: u128, network: u128, bits: u32) -> bool {
    value & (!0_u128 << (128 - bits)) == network
}

fn version_name(version: Version) -> &'static str {
    match version {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;
    use tokio_stream::wrappers::ReceiverStream;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn rejected_request_preserves_url_query_headers_and_body_without_a_socket() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::new(temp.path(), 9923, CancellationToken::new()).unwrap();
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
        let record = state.store.scan().unwrap().remove(0);
        assert_eq!(record.request.upstream_url.as_deref(), Some(target));
        assert_eq!(
            record
                .request
                .headers
                .iter()
                .filter(|header| header.name == "x-client-repeat")
                .count(),
            2
        );
        assert_eq!(
            std::fs::read(record.directory.join("request.body")).unwrap(),
            b"request\0\xffbody"
        );
        assert_eq!(record.result.unwrap().outcome, Outcome::Rejected);
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
        let measurements = measurements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(measurements.request_bytes, 13);
        assert!(measurements.request_body_duration.is_some());
        assert!(error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none());
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
        let summary = SummaryHandle::new(crate::traffic_store::SummaryMetadata {
            schema_version: FORMAT_VERSION,
            record_id: String::new(),
            kind: "summary".to_string(),
            observed_at: utc_now(),
            terminal: false,
            timing: crate::traffic_store::TimingMetadata::default(),
            protocol: Some(ProtocolSummary::for_url(Some(
                "https://example.test/v1/responses",
            ))),
            outcome: None,
            errors: Vec::new(),
            warnings: Vec::new(),
        });
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
                store: None,
                directory: temp.path().to_path_buf(),
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
        assert!(measurements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .request_body_duration
            .is_none());
        assert_eq!(
            error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_deref(),
            Some("client body failed")
        );
    }

    #[tokio::test]
    async fn sse_chunks_reach_disk_before_the_client_without_a_socket() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin(
                "GET",
                "/https://example.com/v1/responses",
                Some("https://example.com/v1/responses"),
                "HTTP/1.1",
                Vec::new(),
                Some("example.com"),
            )
            .unwrap();
        let id = record.id.clone();
        let response_path = record.directory.join("response.body");
        let response_file = tokio::fs::File::from_std(record.response_body.try_clone().unwrap());
        let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
        let protocol = Arc::new(Mutex::new(ProtocolObserver::new(Some(
            "https://example.com/v1/responses",
        ))));
        let guard = RecordGuard::new(
            store.clone(),
            record,
            measurements,
            protocol,
            Instant::now(),
        );
        let (upstream_sender, upstream_receiver) =
            mpsc::channel::<Result<Bytes, reqwest::Error>>(2);
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

        let record = store.find(&id).unwrap();
        assert_eq!(
            std::fs::read(&response_path).unwrap(),
            b"data: first\n\ndata: second\n\n"
        );
        let result = record.result.unwrap();
        assert_eq!(result.outcome, Outcome::Completed);
        assert_eq!(result.response_bytes, 27);
    }

    async fn run_client_close_after_terminal_sse(
        upstream_url: &'static str,
        body: &'static [u8],
    ) -> (Outcome, ProtocolSummary) {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin(
                "POST",
                upstream_url,
                Some(upstream_url),
                "HTTP/1.1",
                Vec::new(),
                Some("example.com"),
            )
            .unwrap();
        let id = record.id.clone();
        let response_file = tokio::fs::File::from_std(record.response_body.try_clone().unwrap());
        let event_index = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(true)
            .open(record.directory.join("response.events.jsonl"))
            .unwrap();
        let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
        let protocol = Arc::new(Mutex::new(ProtocolObserver::new(Some(upstream_url))));
        let guard = RecordGuard::new(
            store.clone(),
            record,
            measurements,
            protocol,
            Instant::now(),
        );
        let (upstream_sender, upstream_receiver) =
            mpsc::channel::<Result<Bytes, reqwest::Error>>(2);
        let (client_sender, mut client_receiver) = mpsc::channel(2);
        let task = tokio::spawn(async move {
            let mut guard = guard;
            record_response_stream_with_index(
                CancellationToken::new(),
                ReceiverStream::new(upstream_receiver),
                response_file,
                client_sender,
                ResponseStreamConfig {
                    is_sse: true,
                    status: 200,
                    event_index: Some(event_index),
                },
                &mut guard,
            )
            .await;
        });

        upstream_sender
            .send(Ok(Bytes::from_static(body)))
            .await
            .unwrap();
        assert_eq!(client_receiver.recv().await.unwrap().unwrap(), body);
        drop(client_receiver);
        task.await.unwrap();

        let record = store.find(&id).unwrap();
        (
            record.result.unwrap().outcome,
            record.summary.protocol.unwrap(),
        )
    }

    #[tokio::test]
    async fn client_close_after_claude_terminal_event_is_completed() {
        let (outcome, protocol) = run_client_close_after_terminal_sse(
            "https://example.com/v1/messages",
            b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
        )
        .await;
        assert_eq!(outcome, Outcome::Completed);
        assert!(!protocol.response_terminal);

        let (outcome, protocol) = run_client_close_after_terminal_sse(
            "https://example.com/v1/messages",
            b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        )
        .await;
        assert_eq!(outcome, Outcome::Completed);
        assert!(protocol.response_terminal);
    }

    #[tokio::test]
    async fn client_close_after_codex_terminal_event_is_completed() {
        let (outcome, protocol) = run_client_close_after_terminal_sse(
            "https://example.com/v1/responses",
            b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":7,\"output_tokens\":3}}}\n\n",
        )
        .await;
        assert_eq!(outcome, Outcome::Completed);
        assert!(protocol.response_terminal);
        assert_eq!(protocol.token_usage.unwrap().output_tokens, Some(3));
    }

    #[tokio::test]
    async fn client_close_before_sse_terminal_event_is_disconnected() {
        let (outcome, protocol) = run_client_close_after_terminal_sse(
            "https://example.com/v1/messages",
            b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\"}\n\n",
        )
        .await;
        assert_eq!(outcome, Outcome::ClientDisconnected);
        assert!(!protocol.response_terminal);
        assert!(protocol.token_usage.is_none());
    }

    #[test]
    fn terminal_sse_detection_does_not_require_an_event_index() {
        let mut indexer = SseIndexer::new(None, "record-1".to_string());
        let first = b"data: {\"type\":\"response.com";
        indexer.feed(first, 0, "1".to_string()).unwrap();
        indexer
            .feed(b"pleted\"}\n\n", first.len() as u64, "2".to_string())
            .unwrap();

        assert!(indexer.terminal_seen());
        assert_eq!(client_closed_terminal(Some(&indexer)).0, Outcome::Completed);
    }

    #[tokio::test]
    async fn upstream_eof_wins_when_client_closes_at_the_same_time() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin(
                "GET",
                "/https://example.com/v1/health",
                Some("https://example.com/v1/health"),
                "HTTP/1.1",
                Vec::new(),
                Some("example.com"),
            )
            .unwrap();
        let id = record.id.clone();
        let response_file = tokio::fs::File::from_std(record.response_body.try_clone().unwrap());
        let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
        let protocol = Arc::new(Mutex::new(ProtocolObserver::new(Some(
            "https://example.com/v1/responses",
        ))));
        let guard = RecordGuard::new(
            store.clone(),
            record,
            measurements,
            protocol,
            Instant::now(),
        );
        let (upstream_sender, upstream_receiver) =
            mpsc::channel::<Result<Bytes, reqwest::Error>>(1);
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
        let mut indexer = SseIndexer::new(Some(file), "record-1".to_string());
        indexer.feed(b"\xef", 0, "1".to_string()).unwrap();
        indexer
            .feed(b"\xbb\xbfdata: first\r", 1, "2".to_string())
            .unwrap();
        indexer
            .feed(b"\n\r\ndata: second\n\n", 15, "3".to_string())
            .unwrap();
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
            "x-internal, keep-alive".parse().unwrap(),
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
}
