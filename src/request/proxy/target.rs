//! Upstream target parsing, resolution, connection, and transport errors.

use super::attempt::RequestAttempt;
use super::error_response::finish_proxy_response;
use super::headers::is_upgrade;
use super::request_stream::reject_with_body;
use crate::request::RequestProxyState;
use crate::request::model::{ErrorKind, Outcome};
use anyhow::Context as _;
use axum::body::Body;
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, Response, StatusCode};
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::time::Duration;
use url::{Host, Url};

pub(super) type UpstreamFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(super) trait UpstreamSender: Send + Sync {
    type Connection: Send + 'static;

    fn connect(
        &self,
        url: &Url,
    ) -> UpstreamFuture<'_, Result<Self::Connection, UpstreamConnectError>>;

    fn send(
        &self,
        connection: Self::Connection,
        request: UpstreamRequest,
    ) -> UpstreamFuture<'_, Result<reqwest::Response, UpstreamSendError>>;
}

pub(super) struct UpstreamRequest {
    pub(super) method: Method,
    pub(super) url: Url,
    pub(super) headers: HeaderMap,
    pub(super) body: reqwest::Body,
}

pub(super) enum UpstreamConnectError {
    InvalidTarget(String),
    Dns(String),
    ClientConfiguration(String),
}

pub(super) struct UpstreamSendError {
    pub(super) message: String,
    pub(super) timeout: bool,
}

pub(super) struct ReqwestUpstreamSender;

impl UpstreamSender for ReqwestUpstreamSender {
    type Connection = reqwest::Client;

    fn connect(
        &self,
        url: &Url,
    ) -> UpstreamFuture<'_, Result<Self::Connection, UpstreamConnectError>> {
        let url = url.clone();
        Box::pin(async move {
            let resolved = validate_and_resolve(&url)
                .await
                .map_err(|error| match error {
                    TargetError::Rejected(message) => UpstreamConnectError::InvalidTarget(message),
                    TargetError::Upstream(message) => UpstreamConnectError::Dns(message),
                })?;
            build_client(&url, &resolved)
                .map_err(|error| UpstreamConnectError::ClientConfiguration(error.to_string()))
        })
    }

    fn send(
        &self,
        connection: Self::Connection,
        request: UpstreamRequest,
    ) -> UpstreamFuture<'_, Result<reqwest::Response, UpstreamSendError>> {
        Box::pin(async move {
            connection
                .request(request.method, request.url)
                .headers(request.headers)
                .body(request.body)
                .send()
                .await
                .map_err(|error| UpstreamSendError {
                    timeout: error.is_timeout(),
                    message: error.to_string(),
                })
        })
    }
}

pub(super) fn upstream_host(url: &Url) -> String {
    match (url.host(), url.port()) {
        (Some(Host::Ipv6(address)), Some(port)) => format!("[{address}]:{port}"),
        (Some(Host::Ipv6(address)), None) => format!("[{address}]"),
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_string(),
        (None, _) => "invalid".to_string(),
    }
}

pub(super) struct RequestRejection {
    pub(super) status: StatusCode,
    pub(super) message: &'static str,
    pub(super) outcome: Outcome,
    pub(super) kind: ErrorKind,
}

pub(super) fn request_rejection(parts: &Parts, upstream: Option<&Url>) -> Option<RequestRejection> {
    if parts.method == Method::CONNECT {
        Some(RequestRejection {
            status: StatusCode::METHOD_NOT_ALLOWED,
            message: "CONNECT is not supported by AIBox Request Proxy",
            outcome: Outcome::Rejected,
            kind: ErrorKind::ConnectNotSupported,
        })
    } else if is_upgrade(&parts.headers) {
        Some(RequestRejection {
            status: StatusCode::UPGRADE_REQUIRED,
            message: "Upgrade and WebSocket request are not supported by AIBox Request Proxy",
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

pub(super) async fn prepare_upstream<S>(
    state: &RequestProxyState,
    guard: &mut RequestAttempt,
    body: Body,
    url: &Url,
    sender: &S,
) -> Result<(S::Connection, Body), Box<Response<Body>>>
where
    S: UpstreamSender,
{
    let connection = tokio::select! {
        () = state.shutdown.cancelled() => {
            return Err(Box::new(finish_proxy_response(
                guard,
                StatusCode::SERVICE_UNAVAILABLE,
                "AIBox Request Proxy is shutting down",
                Outcome::ServerShutdown,
                ErrorKind::ServerShutdown,
            )));
        }
        result = sender.connect(url) => result,
    };
    match connection {
        Ok(connection) => Ok((connection, body)),
        Err(UpstreamConnectError::InvalidTarget(message)) => Err(Box::new(
            reject_with_body(
                guard,
                body,
                state.shutdown.clone(),
                StatusCode::BAD_REQUEST,
                &message,
                Outcome::Rejected,
                ErrorKind::InvalidTargetUrl,
            )
            .await,
        )),
        Err(UpstreamConnectError::Dns(message)) => Err(Box::new(
            reject_with_body(
                guard,
                body,
                state.shutdown.clone(),
                StatusCode::BAD_GATEWAY,
                &message,
                Outcome::UpstreamError,
                ErrorKind::DnsError,
            )
            .await,
        )),
        Err(UpstreamConnectError::ClientConfiguration(message)) => Err(Box::new(
            reject_with_body(
                guard,
                body,
                state.shutdown.clone(),
                StatusCode::BAD_GATEWAY,
                &message,
                Outcome::UpstreamError,
                ErrorKind::ClientConfiguration,
            )
            .await,
        )),
    }
}

pub(super) fn upstream_request_failure(
    guard: &mut RequestAttempt,
    error: &UpstreamSendError,
) -> Response<Body> {
    let recording = guard.request_stream_failure();
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
    let (status, kind) = if error.timeout {
        (StatusCode::GATEWAY_TIMEOUT, ErrorKind::ConnectTimeout)
    } else {
        (StatusCode::BAD_GATEWAY, ErrorKind::UpstreamRequestFailed)
    };
    finish_proxy_response(
        guard,
        status,
        &format!("upstream request failed: {}", error.message),
        Outcome::UpstreamError,
        kind,
    )
}

#[derive(Debug)]
pub(super) enum TargetError {
    Rejected(String),
    Upstream(String),
}

pub(super) async fn validate_and_resolve(url: &Url) -> Result<Vec<SocketAddr>, TargetError> {
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
    Ok(addresses)
}

pub(super) fn build_client(url: &Url, addresses: &[SocketAddr]) -> anyhow::Result<reqwest::Client> {
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
