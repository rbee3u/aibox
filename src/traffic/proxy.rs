use super::store::{
    utc_now, ErrorMetadata, NewRecord, Outcome, RecordedHeader, ResponseMetadata, ResponseSource,
    RuntimeMeasurements, TrafficStore,
};
use super::AppState;
use anyhow::Context as _;
use axum::body::Body;
use axum::http::{header, HeaderMap, Method, Request, Response, StatusCode, Version};
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt};
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
    let started = Instant::now();
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
    let (record, _) = match begin {
        Ok(value) => value,
        Err(error) => return bare_error(StatusCode::INSUFFICIENT_STORAGE, &error.to_string()),
    };
    let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
    let mut guard = RecordGuard::new(state.store.clone(), record, measurements.clone(), started);

    if parts.method == Method::CONNECT {
        return reject_with_body(
            &mut guard,
            body,
            state.shutdown.clone(),
            StatusCode::METHOD_NOT_ALLOWED,
            "CONNECT is not supported by aibox Traffic",
            Outcome::Rejected,
            "connect_not_supported",
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
            "upgrade_not_supported",
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
            "invalid_target_url",
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
                "server_shutdown",
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
                "non_public_target",
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
                "dns_error",
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
                "client_configuration",
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
    let request_stream = recorded_request_stream(
        body,
        request_file,
        measurements.clone(),
        request_error.clone(),
        started,
        state.shutdown.clone(),
    );
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
                "server_shutdown",
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
                    "request_recording_failed",
                );
            }
            let status = if error.is_timeout() {
                StatusCode::GATEWAY_TIMEOUT
            } else {
                StatusCode::BAD_GATEWAY
            };
            let kind = if error.is_timeout() {
                "connect_timeout"
            } else {
                "upstream_request_failed"
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
    {
        let mut values = measurements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        values.ttfb = Some(started.elapsed());
    }
    let metadata = ResponseMetadata {
        format_version: 1,
        source: ResponseSource::Upstream,
        headers_at: utc_now(),
        status: status.as_u16(),
        http_version: version_name(version).to_string(),
        headers: RecordedHeader::from_headers(&original_headers),
    };
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
    state.response_tasks.spawn(async move {
        record_response_stream(
            state_for_task.shutdown.clone(),
            upstream_response.bytes_stream(),
            response_file,
            sender,
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
    started: Instant,
    finished: bool,
}

impl RecordGuard {
    fn new(
        store: TrafficStore,
        record: NewRecord,
        measurements: Arc<Mutex<RuntimeMeasurements>>,
        started: Instant,
    ) -> Self {
        Self {
            store,
            record,
            measurements,
            started,
            finished: false,
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
        self.finished = true;
        self.store
            .finish(&self.record, self.started, &values, outcome, error)?;
        Ok(())
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
                kind: "client_disconnected".to_string(),
                message: "client disconnected before the proxy attempt completed".to_string(),
            }),
        );
        self.store.abandon_active(&self.record.id);
    }
}

fn recorded_request_stream(
    body: Body,
    mut file: tokio::fs::File,
    measurements: Arc<Mutex<RuntimeMeasurements>>,
    error_slot: Arc<Mutex<Option<String>>>,
    started: Instant,
    shutdown: tokio_util::sync::CancellationToken,
) -> impl futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let mut stream = body.into_data_stream();
    async_stream::stream! {
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
            let Some(chunk) = next else { break };
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
            yield Ok(chunk);
        }
        match file.sync_all().await {
            Ok(()) => {
                measurements.lock().unwrap_or_else(std::sync::PoisonError::into_inner).request_body_duration = Some(started.elapsed());
            }
            Err(error) => {
                *error_slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.to_string());
                yield Err(error);
            }
        }
    }
}

async fn record_response_stream(
    shutdown: tokio_util::sync::CancellationToken,
    stream: impl futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    mut file: tokio::fs::File,
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
    guard: &mut RecordGuard,
) {
    let mut stream = Box::pin(stream);
    let terminal = loop {
        let next = tokio::select! {
            biased;
            _ = shutdown.cancelled() => {
                break (
                    Outcome::ServerShutdown,
                    Some(ErrorMetadata { kind: "server_shutdown".to_string(), message: "Traffic Proxy stopped while the response was streaming".to_string() })
                );
            }
            _ = sender.closed() => {
                break (
                    Outcome::ClientDisconnected,
                    Some(ErrorMetadata { kind: "client_disconnected".to_string(), message: "client disconnected while the upstream response was streaming".to_string() })
                );
            }
            next = stream.try_next() => next,
        };
        match next {
            Ok(Some(chunk)) => {
                if let Err(error) = file.write_all(&chunk).await {
                    let message = format!("record response body: {error}");
                    let _ = sender
                        .send(Err(io::Error::new(error.kind(), message.clone())))
                        .await;
                    break (
                        Outcome::RecordingFailed,
                        Some(ErrorMetadata {
                            kind: "response_recording_failed".to_string(),
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
                            kind: "response_recording_failed".to_string(),
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
                if sender.send(Ok(chunk)).await.is_err() {
                    break (
                        Outcome::ClientDisconnected,
                        Some(ErrorMetadata {
                            kind: "client_disconnected".to_string(),
                            message:
                                "client disconnected while the upstream response was streaming"
                                    .to_string(),
                        }),
                    );
                }
            }
            Ok(None) => break (Outcome::Completed, None),
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
                        kind: "upstream_response_failed".to_string(),
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
                kind: "response_recording_failed".to_string(),
                message,
            }),
        ) {
            eprintln!("warning: cannot finalize failed Traffic Record: {finish_error:#}");
        }
    } else if let Err(error) = guard.finish(terminal.0, terminal.1) {
        let message = format!("finalize Traffic Record: {error:#}");
        let _ = sender.send(Err(io::Error::other(message.clone()))).await;
        eprintln!("warning: {message}");
    }
}

async fn reject_with_body(
    guard: &mut RecordGuard,
    body: Body,
    shutdown: tokio_util::sync::CancellationToken,
    status: StatusCode,
    message: &str,
    outcome: Outcome,
    kind: &str,
) -> Response<Body> {
    let started = guard.started;
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
                    "server_shutdown",
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
                    "request_body_failed",
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
        .request_body_duration = Some(started.elapsed());
    finish_proxy_response(guard, status, message, outcome, kind)
}

fn finish_proxy_response(
    guard: &mut RecordGuard,
    status: StatusCode,
    message: &str,
    outcome: Outcome,
    kind: &str,
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
    if let Err(error) = write_proxy_response_record(guard, status, &headers, body.as_bytes()) {
        return recording_failure(guard, error.to_string());
    }
    let finish = guard.finish(
        outcome,
        Some(ErrorMetadata {
            kind: kind.to_string(),
            message: message.to_string(),
        }),
    );
    if let Err(error) = finish {
        return bare_error(StatusCode::INSUFFICIENT_STORAGE, &error.to_string());
    }
    response_with_headers(status, headers, Body::from(body))
}

fn write_proxy_response_record(
    guard: &mut RecordGuard,
    status: StatusCode,
    headers: &HeaderMap,
    body: &[u8],
) -> anyhow::Result<()> {
    guard.record.response_body.write_all(body)?;
    guard.record.response_body.flush()?;
    guard.record.response_body.sync_all()?;
    {
        let mut values = guard
            .measurements
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        values.response_bytes = body.len() as u64;
        values.ttfb = Some(guard.started.elapsed());
    }
    guard.store.write_response(
        &guard.record.directory,
        &ResponseMetadata {
            format_version: 1,
            source: ResponseSource::Proxy,
            headers_at: utc_now(),
            status: status.as_u16(),
            http_version: "HTTP/1.1".to_string(),
            headers: RecordedHeader::from_headers(headers),
        },
    )
}

fn recording_failure(guard: &mut RecordGuard, message: String) -> Response<Body> {
    let _ = guard.finish(
        Outcome::RecordingFailed,
        Some(ErrorMetadata {
            kind: "recording_failed".to_string(),
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
        let guard = RecordGuard::new(store.clone(), record, measurements, Instant::now());
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
