use crate::traffic::AppState;
use crate::traffic_interpretation::{
    BodyContentCoding, ProtocolFamily, ProtocolObserver, ProtocolSummary, ResponseModeValue,
    body_content_coding,
};
#[cfg(test)]
use crate::traffic_sse::is_first_token_data;
use crate::traffic_sse::{ObservedSseEvent, PrefixSniff, SseIndexer, SsePrefixSniffer};
use crate::traffic_store::{
    ErrorKind, ErrorMetadata, FORMAT_VERSION, NewRecord, Outcome, RecordLocator, RecordedHeader,
    RequestMetadata, ResponseMetadata, ResponseSource, RuntimeMeasurements, SummaryHandle,
    TrafficStore, offset_ns, utc_now,
};
use anyhow::Context as _;
use axum::body::Body;
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, Request, Response, StatusCode, Version, header};
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
use std::collections::HashSet;
use std::io::{self, Read as _};
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
    let ActiveRecord {
        mut guard,
        measurements,
        protocol,
        request_metadata,
    } = match begin_record(&state, &parts, &incoming_uri, upstream) {
        Ok(record) => record,
        Err(error) => return bare_error(StatusCode::INSUFFICIENT_STORAGE, &error.to_string()),
    };

    if let Some(rejection) = request_rejection(&parts, upstream) {
        return reject_with_body(
            &mut guard,
            body,
            state.shutdown.clone(),
            rejection.status,
            rejection.message,
            rejection.outcome,
            rejection.kind,
        )
        .await;
    }
    let url = upstream
        .cloned()
        .expect("a rejected request cannot have a missing target URL");

    let (client, body) = match prepare_upstream(&state, &mut guard, body, &url).await {
        Ok(prepared) => prepared,
        Err(response) => return response,
    };

    let request_file = match guard.record.request_body.try_clone() {
        Ok(file) => tokio::fs::File::from_std(file),
        Err(error) => {
            return recording_failure(&mut guard, format!("clone request body file: {error}"));
        }
    };
    let request_error = Arc::new(Mutex::new(None::<RequestStreamFailure>));
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
            locator: Some(guard.record.locator.clone()),
            directory: std::path::PathBuf::new(),
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
        () = state.shutdown.cancelled() => {
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
        Err(error) => return upstream_request_failure(&mut guard, &request_error, &error),
    };

    stream_upstream_response(&state, upstream_response, guard)
}

fn stream_upstream_response(
    state: &AppState,
    upstream_response: reqwest::Response,
    mut guard: RecordGuard,
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
        headers: RecordedHeader::from_headers(&original_headers),
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
    if let Err(error) =
        state
            .store
            .write_response(&guard.record.locator, &guard.record.summary, &metadata)
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
    state.response_tasks.spawn(async move {
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

struct ActiveRecord {
    guard: RecordGuard,
    measurements: Arc<Mutex<RuntimeMeasurements>>,
    protocol: Arc<Mutex<ProtocolObserver>>,
    request_metadata: RequestMetadata,
}

fn begin_record(
    state: &AppState,
    parts: &Parts,
    incoming_uri: &str,
    upstream: Option<&Url>,
) -> anyhow::Result<ActiveRecord> {
    let host_hint = upstream.and_then(Url::host_str);
    let (record, request_metadata) = state.store.begin(
        parts.method.as_str(),
        incoming_uri,
        upstream.map(Url::as_str),
        version_name(parts.version),
        RecordedHeader::from_headers(&parts.headers),
        host_hint,
    )?;
    let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
    let protocol = Arc::new(Mutex::new(ProtocolObserver::new(
        request_metadata.upstream_url.as_deref(),
    )));
    let guard = RecordGuard::new(
        state.store.clone(),
        record,
        measurements.clone(),
        protocol.clone(),
    );
    Ok(ActiveRecord {
        guard,
        measurements,
        protocol,
        request_metadata,
    })
}

struct RequestRejection {
    status: StatusCode,
    message: &'static str,
    outcome: Outcome,
    kind: ErrorKind,
}

fn request_rejection(parts: &Parts, upstream: Option<&Url>) -> Option<RequestRejection> {
    if parts.method == Method::CONNECT {
        Some(RequestRejection {
            status: StatusCode::METHOD_NOT_ALLOWED,
            message: "CONNECT is not supported by aibox Traffic",
            outcome: Outcome::Rejected,
            kind: ErrorKind::ConnectNotSupported,
        })
    } else if is_upgrade(&parts.headers) {
        Some(RequestRejection {
            status: StatusCode::UPGRADE_REQUIRED,
            message: "Upgrade and WebSocket traffic are not supported by aibox Traffic",
            outcome: Outcome::Rejected,
            kind: ErrorKind::UpgradeNotSupported,
        })
    } else if upstream.is_none() {
        Some(RequestRejection {
            status: StatusCode::BAD_REQUEST,
            message: "proxy path must contain an absolute http:// or https:// target URL",
            outcome: Outcome::Rejected,
            kind: ErrorKind::InvalidTargetUrl,
        })
    } else {
        None
    }
}

async fn prepare_upstream(
    state: &AppState,
    guard: &mut RecordGuard,
    body: Body,
    url: &Url,
) -> Result<(reqwest::Client, Body), Response<Body>> {
    let resolved = tokio::select! {
        () = state.shutdown.cancelled() => {
            return Err(finish_proxy_response(
                guard,
                StatusCode::SERVICE_UNAVAILABLE,
                "aibox Traffic is shutting down",
                Outcome::ServerShutdown,
                ErrorKind::ServerShutdown,
            ));
        }
        result = validate_and_resolve(url, state.allow_private_upstream) => result,
    };
    let resolved = match resolved {
        Ok(addresses) => addresses,
        Err(TargetError::Rejected(message)) => {
            return Err(reject_with_body(
                guard,
                body,
                state.shutdown.clone(),
                StatusCode::FORBIDDEN,
                &message,
                Outcome::Rejected,
                ErrorKind::NonPublicTarget,
            )
            .await);
        }
        Err(TargetError::Upstream(message)) => {
            return Err(reject_with_body(
                guard,
                body,
                state.shutdown.clone(),
                StatusCode::BAD_GATEWAY,
                &message,
                Outcome::UpstreamError,
                ErrorKind::DnsError,
            )
            .await);
        }
    };
    match build_client(url, &resolved) {
        Ok(client) => Ok((client, body)),
        Err(error) => Err(reject_with_body(
            guard,
            body,
            state.shutdown.clone(),
            StatusCode::BAD_GATEWAY,
            &error.to_string(),
            Outcome::UpstreamError,
            ErrorKind::ClientConfiguration,
        )
        .await),
    }
}

fn upstream_request_failure(
    guard: &mut RecordGuard,
    request_error: &Mutex<Option<RequestStreamFailure>>,
    error: &reqwest::Error,
) -> Response<Body> {
    let recording = request_error
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(failure) = recording {
        let (status, outcome) = match failure.kind {
            ErrorKind::ClientDisconnected | ErrorKind::RequestBodyFailed => {
                (StatusCode::BAD_REQUEST, Outcome::ClientDisconnected)
            }
            ErrorKind::ServerShutdown => (StatusCode::SERVICE_UNAVAILABLE, Outcome::ServerShutdown),
            _ => (StatusCode::INSUFFICIENT_STORAGE, Outcome::RecordingFailed),
        };
        return finish_proxy_response(guard, status, &failure.message, outcome, failure.kind);
    }
    let (status, kind) = if error.is_timeout() {
        (StatusCode::GATEWAY_TIMEOUT, ErrorKind::ConnectTimeout)
    } else {
        (StatusCode::BAD_GATEWAY, ErrorKind::UpstreamRequestFailed)
    };
    finish_proxy_response(
        guard,
        status,
        &format!("upstream request failed: {error}"),
        Outcome::UpstreamError,
        kind,
    )
}

struct RecordGuard {
    store: TrafficStore,
    record: NewRecord,
    measurements: Arc<Mutex<RuntimeMeasurements>>,
    protocol: Arc<Mutex<ProtocolObserver>>,
    pending_terminal: Option<(Outcome, Option<ErrorMetadata>)>,
    finished: bool,
}

impl RecordGuard {
    fn new(
        store: TrafficStore,
        record: NewRecord,
        measurements: Arc<Mutex<RuntimeMeasurements>>,
        protocol: Arc<Mutex<ProtocolObserver>>,
    ) -> Self {
        Self {
            store,
            record,
            measurements,
            protocol,
            pending_terminal: None,
            finished: false,
        }
    }

    fn mark_timing(
        &self,
        update: impl FnOnce(&mut crate::traffic_store::TimingMetadata),
    ) -> anyhow::Result<()> {
        self.store
            .update_summary(&self.record.locator, &self.record.summary, |summary| {
                update(&mut summary.timing);
                true
            })?;
        Ok(())
    }

    fn observe_response_headers(
        &self,
        headers: &[RecordedHeader],
        event_stream: Option<bool>,
    ) -> anyhow::Result<()> {
        let at_ns = offset_ns(self.record.origin);
        let mut observer = self
            .protocol
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        observer.observe_response_headers(headers, event_stream, at_ns.clone());
        let protocol = observer.snapshot();
        self.store
            .update_summary(&self.record.locator, &self.record.summary, |summary| {
                summary.timing.upstream_response_headers_at_ns = Some(at_ns);
                summary.protocol = Some(protocol);
                true
            })?;
        Ok(())
    }

    fn observe_response_mode(&self, event_stream: bool) -> anyhow::Result<()> {
        let mut observer = self
            .protocol
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if observer.observe_response_mode(event_stream, offset_ns(self.record.origin)) {
            self.publish_protocol(observer.snapshot())?;
        }
        Ok(())
    }

    fn protocol_summary(&self) -> ProtocolSummary {
        self.protocol
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
    }

    fn observe_sse_events(&self, events: &[ObservedSseEvent]) -> anyhow::Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let mut observer = self
            .protocol
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let changed = events
            .iter()
            .fold(false, |changed, (event_name, data, at_ns)| {
                observer.observe_sse_event(event_name.as_deref(), data, at_ns.clone()) | changed
            });
        if changed {
            self.publish_protocol(observer.snapshot())?;
        }
        Ok(())
    }

    fn observe_first_token(&self, at_ns: String) -> anyhow::Result<()> {
        let mut observer = self
            .protocol
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if observer.observe_first_token(at_ns) {
            self.publish_protocol(observer.snapshot())?;
        }
        Ok(())
    }

    fn observe_json_response(&self, status: u16, headers: &[RecordedHeader]) -> anyhow::Result<()> {
        let at_ns = offset_ns(self.record.origin);
        let mut observer = self
            .protocol
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.store
            .with_record_path(&self.record.locator, |directory| {
                observer.observe_json_response(
                    &directory.join("response.body"),
                    status,
                    headers,
                    at_ns,
                )
            })?;
        self.publish_protocol(observer.snapshot())
    }

    fn observe_encoded_sse_response(&self) -> anyhow::Result<()> {
        let at_ns = offset_ns(self.record.origin);
        let decoded = self
            .store
            .with_record_path(&self.record.locator, |directory| {
                let file = crate::tenant::open_real_file(
                    &directory.join("response.body"),
                    "Traffic response body",
                )?;
                let mut decoder = zstd::stream::read::Decoder::new(file)
                    .context("create zstd response decoder")?;
                let mut bytes = Vec::new();
                decoder
                    .read_to_end(&mut bytes)
                    .context("decode zstd response body")?;
                Ok::<_, anyhow::Error>(bytes)
            })?;
        let decoded = match decoded {
            Ok(bytes) => bytes,
            Err(error) => {
                self.add_warning("response_interpretation_failed", error.to_string());
                return Ok(());
            }
        };
        let mut indexer = SseIndexer::new(None, self.record.id.clone());
        if let Err(error) = indexer.feed(&decoded, 0, at_ns.clone()) {
            self.add_warning("response_interpretation_failed", error.to_string());
        }
        if let Err(error) = indexer.finish() {
            self.add_warning("response_interpretation_failed", error.to_string());
        }
        self.observe_sse_events(&indexer.take_protocol_events())
    }

    fn publish_protocol(&self, protocol: ProtocolSummary) -> anyhow::Result<()> {
        self.store
            .update_summary(&self.record.locator, &self.record.summary, |summary| {
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
                .update_summary(&self.record.locator, &self.record.summary, |summary| {
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
        self.pending_terminal = Some((outcome, error.clone()));
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
            self.pending_terminal = None;
        }
        result.map(|_| ())
    }
}

impl Drop for RecordGuard {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let (outcome, error) = self.pending_terminal.clone().unwrap_or_else(|| {
            (
                Outcome::ClientDisconnected,
                Some(ErrorMetadata {
                    kind: ErrorKind::ClientDisconnected,
                    message: "client disconnected before the proxy attempt completed".to_string(),
                }),
            )
        });
        let _ = self.finish(outcome, error);
        self.store.abandon_active(&self.record.id);
    }
}

struct RequestStreamContext {
    measurements: Arc<Mutex<RuntimeMeasurements>>,
    error_slot: Arc<Mutex<Option<RequestStreamFailure>>>,
    summary: SummaryHandle,
    protocol: Arc<Mutex<ProtocolObserver>>,
    request_headers: Vec<RecordedHeader>,
    store: Option<TrafficStore>,
    locator: Option<RecordLocator>,
    directory: std::path::PathBuf,
    origin: Instant,
    shutdown: tokio_util::sync::CancellationToken,
}

#[derive(Clone, Debug)]
struct RequestStreamFailure {
    kind: ErrorKind,
    message: String,
}

fn request_failure(
    slot: &Mutex<Option<RequestStreamFailure>>,
    kind: ErrorKind,
    error: impl ToString,
) {
    *slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(RequestStreamFailure {
        kind,
        message: error.to_string(),
    });
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
        locator,
        directory,
        origin,
        shutdown,
    } = context;
    let mut stream = body.into_data_stream();
    async_stream::stream! {
        let mut body_complete = false;
        loop {
            let next = tokio::select! {
                () = shutdown.cancelled() => {
                    let error = io::Error::new(io::ErrorKind::Interrupted, "Traffic Proxy is shutting down");
                    request_failure(&error_slot, ErrorKind::ServerShutdown, &error);
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
                    request_failure(&error_slot, ErrorKind::RequestBodyFailed, &error);
                    yield Err(error);
                    break;
                }
            };
            if let Err(error) = file.write_all(&chunk).await {
                request_failure(&error_slot, ErrorKind::RequestRecordingFailed, &error);
                yield Err(error);
                break;
            }
            if let Err(error) = file.flush().await {
                request_failure(&error_slot, ErrorKind::RequestRecordingFailed, &error);
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
                locator.as_ref(),
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
                request_failure(&error_slot, ErrorKind::RequestRecordingFailed, &error);
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
                    locator.as_ref(),
                    &directory,
                    &summary,
                    &protocol,
                    &request_headers,
                    at_ns,
                );
                if let Err(error) = checkpoint {
                    request_failure(&error_slot, ErrorKind::RequestRecordingFailed, &error);
                    yield Err(io::Error::other(error.to_string()));
                }
            }
            Ok(()) => {}
            Err(error) => {
                request_failure(&error_slot, ErrorKind::RequestRecordingFailed, &error);
                if body_complete {
                    yield Err(error);
                }
            }
        }
    }
}

fn checkpoint_request_complete(
    store: Option<&TrafficStore>,
    locator: Option<&RecordLocator>,
    directory: &std::path::Path,
    summary: &SummaryHandle,
    protocol: &Mutex<ProtocolObserver>,
    request_headers: &[RecordedHeader],
    at_ns: String,
) -> anyhow::Result<bool> {
    if summary.read(|summary| summary.terminal) {
        return Ok(false);
    }
    let mut observer = protocol
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _ = match (store, locator) {
        (Some(store), Some(locator)) => store.with_record_path(locator, |directory| {
            observer.observe_request(
                &directory.join("request.body"),
                request_headers,
                at_ns.clone(),
            )
        })?,
        _ => observer.observe_request(
            &directory.join("request.body"),
            request_headers,
            at_ns.clone(),
        ),
    };
    let protocol_snapshot = observer.snapshot();
    checkpoint_summary_update(store, locator, summary, |value| {
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
    error_slot: Arc<Mutex<Option<RequestStreamFailure>>>,
    started: Instant,
    shutdown: tokio_util::sync::CancellationToken,
) -> impl futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let summary = SummaryHandle::new(crate::traffic_store::SummaryMetadata::test(
        String::new(),
        Some(ProtocolSummary::default()),
    ));
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
            locator: None,
            directory: std::path::PathBuf::new(),
            origin: started,
            shutdown,
        },
    )
}

fn checkpoint_summary_update(
    store: Option<&TrafficStore>,
    locator: Option<&RecordLocator>,
    summary: &SummaryHandle,
    update: impl FnOnce(&mut crate::traffic_store::SummaryMetadata) -> bool,
) -> anyhow::Result<bool> {
    match store {
        Some(store) => store.update_summary(
            locator.context("Traffic Record locator is unavailable")?,
            summary,
            update,
        ),
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
            mode: ResponseStreamMode::Normal,
            status: 200,
            headers: Vec::new(),
        },
        guard,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ResponseStreamMode {
    Normal,
    EventStream,
    OpaqueEventStream,
    Detect,
}

impl ResponseStreamMode {
    fn observed_event_stream(self) -> Option<bool> {
        match self {
            Self::Normal => Some(false),
            Self::EventStream | Self::OpaqueEventStream => Some(true),
            Self::Detect => None,
        }
    }
}

struct ResponseStreamConfig {
    mode: ResponseStreamMode,
    status: u16,
    headers: Vec<RecordedHeader>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DownstreamSend {
    Sent,
    Closed,
    Shutdown,
}

async fn send_downstream(
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

enum ResponseBodyTracker {
    Normal,
    OpaqueEventStream,
    EventStream(SseIndexer),
    Detect {
        sniffer: SsePrefixSniffer,
        pending: Vec<(Bytes, String)>,
    },
}

impl ResponseBodyTracker {
    fn new(mode: ResponseStreamMode, guard: &RecordGuard) -> Self {
        match mode {
            ResponseStreamMode::Normal => Self::Normal,
            ResponseStreamMode::EventStream => Self::EventStream(new_sse_indexer(guard)),
            ResponseStreamMode::OpaqueEventStream => Self::OpaqueEventStream,
            ResponseStreamMode::Detect => Self::Detect {
                sniffer: SsePrefixSniffer::default(),
                pending: Vec::new(),
            },
        }
    }

    fn observe_chunk(
        &mut self,
        chunk: &Bytes,
        at_ns: String,
        guard: &RecordGuard,
    ) -> anyhow::Result<()> {
        match self {
            Self::Normal | Self::OpaqueEventStream => Ok(()),
            Self::EventStream(indexer) => feed_sse_chunk(indexer, chunk, at_ns, guard),
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
                            feed_sse_chunk(&mut indexer, &buffered_chunk, buffered_at_ns, guard)?;
                        }
                        *self = Self::EventStream(indexer);
                        Ok(())
                    }
                }
            }
        }
    }

    fn finish(&mut self, guard: &RecordGuard) -> anyhow::Result<()> {
        match self {
            Self::Normal => Ok(()),
            Self::OpaqueEventStream => guard.observe_encoded_sse_response(),
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
                if let Some(at_ns) = indexer.take_first_token_at_ns() {
                    guard.observe_first_token(at_ns)?;
                }
                let events = indexer.take_protocol_events();
                guard.observe_sse_events(&events)
            }
        }
    }

    fn is_event_stream(&self) -> bool {
        matches!(self, Self::EventStream(_) | Self::OpaqueEventStream)
    }

    fn terminal_seen(&self) -> bool {
        matches!(self, Self::EventStream(indexer) if indexer.terminal_seen())
    }
}

fn new_sse_indexer(guard: &RecordGuard) -> SseIndexer {
    let event_index = match guard.store.create_event_index(&guard.record) {
        Ok(file) => Some(file),
        Err(error) => {
            guard.add_warning("event_index_failed", error.to_string());
            None
        }
    };
    SseIndexer::new(event_index, guard.record.id.clone())
}

fn feed_sse_chunk(
    indexer: &mut SseIndexer,
    chunk: &Bytes,
    at_ns: String,
    guard: &RecordGuard,
) -> anyhow::Result<()> {
    let body_offset = indexer.body_offset();
    if let Err(error) = indexer.feed(chunk, body_offset, at_ns) {
        guard.add_warning("event_index_failed", error.to_string());
        indexer.disable_indexing();
    }
    if let Some(at_ns) = indexer.take_first_token_at_ns() {
        guard.observe_first_token(at_ns)?;
    }
    guard.observe_sse_events(&indexer.take_protocol_events())
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
        mode,
        status: response_status,
        headers: response_headers,
    } = config;
    let mut stream = Box::pin(stream);
    let mut tracker = ResponseBodyTracker::new(mode, guard);
    let mut response_completed = false;
    let mut terminal = loop {
        let next = tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                break (
                    Outcome::ServerShutdown,
                    Some(ErrorMetadata { kind: ErrorKind::ServerShutdown, message: "Traffic Proxy stopped while the response was streaming".to_string() })
                );
            }
            // Prefer an already-ready upstream EOF when the client closes at
            // the same time. This avoids turning a normal response into a
            // disconnect solely because the downstream body was dropped first.
            next = stream.try_next() => next,
            () = sender.closed() => {
                break client_closed_terminal(&tracker);
            }
        };
        match next {
            Ok(Some(chunk)) => {
                if guard.record.summary.read(|summary| {
                    summary
                        .timing
                        .upstream_response_body_first_byte_at_ns
                        .is_none()
                }) && let Err(error) = guard.mark_timing(|timing| {
                    timing.upstream_response_body_first_byte_at_ns =
                        Some(offset_ns(guard.record.origin));
                }) {
                    let _ = send_downstream(
                        &sender,
                        &shutdown,
                        Err(io::Error::other(error.to_string())),
                    )
                    .await;
                    break (
                        Outcome::RecordingFailed,
                        Some(ErrorMetadata {
                            kind: ErrorKind::ResponseRecordingFailed,
                            message: error.to_string(),
                        }),
                    );
                }
                if let Err(error) = file.write_all(&chunk).await {
                    let message = format!("record response body: {error}");
                    let _ = send_downstream(
                        &sender,
                        &shutdown,
                        Err(io::Error::new(error.kind(), message.clone())),
                    )
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
                    let _ = send_downstream(
                        &sender,
                        &shutdown,
                        Err(io::Error::new(error.kind(), message.clone())),
                    )
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
                if let Err(error) =
                    tracker.observe_chunk(&chunk, offset_ns(guard.record.origin), guard)
                {
                    let _ = send_downstream(
                        &sender,
                        &shutdown,
                        Err(io::Error::other(error.to_string())),
                    )
                    .await;
                    break (
                        Outcome::RecordingFailed,
                        Some(ErrorMetadata {
                            kind: ErrorKind::ResponseRecordingFailed,
                            message: error.to_string(),
                        }),
                    );
                }
                match send_downstream(&sender, &shutdown, Ok(chunk)).await {
                    DownstreamSend::Sent => {}
                    DownstreamSend::Closed => break client_closed_terminal(&tracker),
                    DownstreamSend::Shutdown => {
                        break (
                            Outcome::ServerShutdown,
                            Some(ErrorMetadata {
                                kind: ErrorKind::ServerShutdown,
                                message: "Traffic Proxy stopped while the response was streaming"
                                    .to_string(),
                            }),
                        );
                    }
                }
            }
            Ok(None) => {
                if let Err(error) = tracker.finish(guard) {
                    break (
                        Outcome::RecordingFailed,
                        Some(ErrorMetadata {
                            kind: ErrorKind::ResponseRecordingFailed,
                            message: error.to_string(),
                        }),
                    );
                }
                response_completed = true;
                break (Outcome::Completed, None);
            }
            Err(error) => {
                let message = format!("upstream response stream failed: {error}");
                let _ = send_downstream(
                    &sender,
                    &shutdown,
                    Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        message.clone(),
                    )),
                )
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
        let _ = send_downstream(
            &sender,
            &shutdown,
            Err(io::Error::new(error.kind(), message.clone())),
        )
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
        let semantic_result = if response_completed && !tracker.is_event_stream() {
            guard.observe_json_response(response_status, &response_headers)
        } else {
            Ok(())
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
        if response_completed
            && let Err(error) = guard.mark_timing(|timing| {
                timing.upstream_response_body_completed_at_ns =
                    Some(offset_ns(guard.record.origin));
            })
        {
            terminal = (
                Outcome::RecordingFailed,
                Some(ErrorMetadata {
                    kind: ErrorKind::ResponseRecordingFailed,
                    message: error.to_string(),
                }),
            );
        }
        if let Err(error) = guard.finish(terminal.0, terminal.1) {
            let message = format!("finalize Traffic Record: {error:#}");
            let _ =
                send_downstream(&sender, &shutdown, Err(io::Error::other(message.clone()))).await;
            eprintln!("warning: {message}");
        }
    }
}

fn client_closed_terminal(tracker: &ResponseBodyTracker) -> (Outcome, Option<ErrorMetadata>) {
    if tracker.terminal_seen() {
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
            () = shutdown.cancelled() => {
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

fn response_stream_mode(
    headers: &HeaderMap,
    status: StatusCode,
    protocol: &ProtocolSummary,
) -> ResponseStreamMode {
    if is_event_stream(headers) {
        let recorded = RecordedHeader::from_headers(headers);
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
    use base64::Engine as _;
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

    #[test]
    fn terminal_retry_preserves_the_original_outcome() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin("GET", "/failed", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let id = record.id.clone();
        let summary_path = record.directory.join("summary.json");
        let saved_summary_path = record.directory.join("summary.saved");
        let mut guard = RecordGuard::new(
            store.clone(),
            record,
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
        let measurements = measurements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(measurements.request_bytes, 13);
        assert!(measurements.request_body_duration.is_some());
        assert!(
            error
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none()
        );
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
        let summary = SummaryHandle::new(crate::traffic_store::SummaryMetadata::test(
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
                store: None,
                locator: None,
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
        assert!(
            measurements
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .request_body_duration
                .is_none()
        );
        let failure = error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap();
        assert_eq!(failure.kind, ErrorKind::RequestBodyFailed);
        assert_eq!(failure.message, "client body failed");
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
        let guard = RecordGuard::new(store.clone(), record, measurements, protocol);
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
            std::fs::read(record.directory.join("response.body")).unwrap(),
            b"data: first\n\ndata: second\n\n"
        );
        let result = record.result.unwrap();
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
        let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
        let protocol = Arc::new(Mutex::new(ProtocolObserver::new(Some(upstream_url))));
        let guard = RecordGuard::new(store.clone(), record, measurements, protocol);
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

        let record = store.find(&id).unwrap();
        (
            record.result.unwrap().outcome,
            record.summary.protocol.unwrap(),
        )
    }

    #[tokio::test]
    async fn client_close_after_claude_terminal_event_is_completed() {
        let (outcome, protocol) = run_client_close_after_response(
            "https://example.com/v1/messages",
            &[b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n"],
            ResponseStreamMode::EventStream,
        )
        .await;
        assert_eq!(outcome, Outcome::Completed);
        assert!(!protocol.response_terminal);

        let (outcome, protocol) = run_client_close_after_response(
            "https://example.com/v1/messages",
            &[b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"],
            ResponseStreamMode::EventStream,
        )
        .await;
        assert_eq!(outcome, Outcome::Completed);
        assert!(protocol.response_terminal);
    }

    #[tokio::test]
    async fn client_close_after_codex_terminal_event_is_completed() {
        let (outcome, protocol) = run_client_close_after_response(
            "https://example.com/v1/responses",
            &[b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"input_tokens\":7,\"output_tokens\":3}}}\n\n"],
            ResponseStreamMode::EventStream,
        )
        .await;
        assert_eq!(outcome, Outcome::Completed);
        assert!(protocol.response_terminal);
        assert_eq!(protocol.token_usage.unwrap().output_tokens, Some(3));
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
        ] {
            let (outcome, protocol) =
                run_client_close_after_response(url, &[event], ResponseStreamMode::EventStream)
                    .await;

            assert_eq!(outcome, Outcome::ClientDisconnected);
            assert!(protocol.first_token_at_ns.is_some());
            assert_eq!(protocol.model.effective.as_deref(), Some(model));
        }
    }

    #[tokio::test]
    async fn malformed_sse_data_still_publishes_first_token_and_diagnostics() {
        let (_, protocol) = run_client_close_after_response(
            "https://example.com/v1/messages",
            &[b"data: {malformed json\n\n"],
            ResponseStreamMode::EventStream,
        )
        .await;

        assert!(protocol.first_token_at_ns.is_some());
        assert_eq!(protocol.warnings[0].kind, "sse_event_invalid");
    }

    #[tokio::test]
    async fn zstd_sse_is_interpreted_only_after_eof_without_event_timing() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let upstream_url = "https://example.com/v1/responses";
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
        let guard = RecordGuard::new(
            store.clone(),
            record,
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
        let response_file =
            tokio::fs::File::from_std(guard.record.response_body.try_clone().unwrap());
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

        let record = store.find(&id).unwrap();
        let protocol = record.summary.protocol.unwrap();
        assert!(protocol.response_terminal);
        assert_eq!(protocol.errors[0].kind, "service_unavailable_error");
        assert!(protocol.first_token_at_ns.is_none());
        assert!(!record.directory.join("response.events.jsonl").exists());
    }

    #[tokio::test]
    async fn client_close_before_sse_terminal_event_is_disconnected() {
        let (outcome, protocol) = run_client_close_after_response(
            "https://example.com/v1/messages",
            &[b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\"}\n\n"],
            ResponseStreamMode::EventStream,
        )
        .await;
        assert_eq!(outcome, Outcome::ClientDisconnected);
        assert!(!protocol.response_terminal);
        assert!(protocol.first_token_at_ns.is_some());
        assert!(protocol.token_usage.is_none());
    }

    #[tokio::test]
    async fn headerless_split_sse_is_completed_when_client_closes_after_terminal_event() {
        let (outcome, protocol) = run_client_close_after_response(
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
        assert!(protocol.response_terminal);
        assert_eq!(protocol.token_usage.unwrap().output_tokens, Some(3));
    }

    #[tokio::test]
    async fn headerless_json_response_remains_normal_and_keeps_usage() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let upstream_url = "https://example.com/v1/responses";
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
        let event_index_path = record.directory.join("response.events.jsonl");
        let response_file = tokio::fs::File::from_std(record.response_body.try_clone().unwrap());
        let guard = RecordGuard::new(
            store.clone(),
            record,
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

        let record = store.find(&id).unwrap();
        assert_eq!(record.result.unwrap().outcome, Outcome::Completed);
        assert_eq!(
            record
                .summary
                .protocol
                .as_ref()
                .unwrap()
                .response_mode
                .observed,
            Some(ResponseModeValue::Normal)
        );
        assert!(record.summary.protocol.as_ref().unwrap().response_terminal);
        assert_eq!(
            record
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
        let mut indexer = SseIndexer::new(None, "record-1".to_string());
        indexer.feed(ignored, 0, "1".to_string()).unwrap();
        assert!(indexer.take_first_token_at_ns().is_none());

        let message_start = b"data:\ndata: {\"type\":\"message_start\"}\n\n";
        indexer
            .feed(message_start, ignored.len() as u64, "2".to_string())
            .unwrap();
        assert_eq!(indexer.take_first_token_at_ns().as_deref(), Some("2"));

        indexer
            .feed(
                b"data: ping\n\n",
                (ignored.len() + message_start.len()) as u64,
                "3".to_string(),
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
            let mut indexer = SseIndexer::new(None, "record-1".to_string());
            indexer.feed(line, 0, "7".to_string()).unwrap();
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
            let mut indexer = SseIndexer::new(None, "record-1".to_string());
            indexer.feed(body, 0, "11".to_string()).unwrap();
            indexer.finish().unwrap();
            assert_eq!(indexer.take_first_token_at_ns().as_deref(), Some("11"));
        }
    }

    #[test]
    fn sse_first_token_uses_line_completion_and_eof_arrival_times() {
        let mut indexer = SseIndexer::new(None, "record-1".to_string());
        let first = b"\xef\xbb\xbfdata: {\"type\":\"response.created\"}";
        indexer.feed(first, 0, "1".to_string()).unwrap();
        assert!(indexer.take_first_token_at_ns().is_none());
        indexer
            .feed(b"\r", first.len() as u64, "2".to_string())
            .unwrap();
        assert!(indexer.take_first_token_at_ns().is_none());
        indexer
            .feed(b"\n", (first.len() + 1) as u64, "3".to_string())
            .unwrap();
        assert_eq!(indexer.take_first_token_at_ns().as_deref(), Some("3"));

        let mut eof = SseIndexer::new(None, "record-2".to_string());
        eof.feed(b"data: ping", 0, "8".to_string()).unwrap();
        assert!(eof.take_first_token_at_ns().is_none());
        assert!(eof.finish().unwrap());
        assert_eq!(eof.take_first_token_at_ns().as_deref(), Some("8"));
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
        let tracker = ResponseBodyTracker::EventStream(indexer);
        assert_eq!(client_closed_terminal(&tracker).0, Outcome::Completed);
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
        let guard = RecordGuard::new(store.clone(), record, measurements, protocol);
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
