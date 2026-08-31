//! Upstream target parsing, address policy, connection, and transport errors.

use super::attempt::RequestAttempt;
use super::headers::is_upgrade;
use super::response_stream::{finish_proxy_response, reject_with_body};
use crate::request::RequestProxyState;
use crate::request::model::{ErrorKind, Outcome};
use anyhow::Context as _;
use axum::body::Body;
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, Response, StatusCode, Version};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::pin::Pin;
use std::time::Duration;
use url::{Host, Url};

pub(super) type UpstreamFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(super) trait UpstreamSender: Send + Sync {
    type Connection: Send + 'static;

    fn connect(
        &self,
        url: &Url,
        allow_private_upstream: bool,
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
    NonPublic(String),
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
        allow_private_upstream: bool,
    ) -> UpstreamFuture<'_, Result<Self::Connection, UpstreamConnectError>> {
        let url = url.clone();
        Box::pin(async move {
            let resolved = validate_and_resolve(&url, allow_private_upstream)
                .await
                .map_err(|error| match error {
                    TargetError::Rejected(message) => UpstreamConnectError::NonPublic(message),
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
        result = sender.connect(url, state.allow_private_upstream) => result,
    };
    match connection {
        Ok(connection) => Ok((connection, body)),
        Err(UpstreamConnectError::NonPublic(message)) => Err(Box::new(
            reject_with_body(
                guard,
                body,
                state.shutdown.clone(),
                StatusCode::FORBIDDEN,
                &message,
                Outcome::Rejected,
                ErrorKind::NonPublicTarget,
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

pub(super) enum TargetError {
    Rejected(String),
    Upstream(String),
}

pub(super) async fn validate_and_resolve(
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

pub(super) fn require_allowed_addresses(
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

pub(super) fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_v4(address),
        IpAddr::V6(address) => is_public_v6(address),
    }
}

pub(super) fn is_allowed_upstream_ip(address: IpAddr) -> bool {
    is_public_ip(address) || is_fake_ip_v4(address)
}

pub(super) fn is_fake_ip_v4(address: IpAddr) -> bool {
    let address = match address {
        IpAddr::V4(address) => address,
        IpAddr::V6(address) => match address.to_ipv4_mapped() {
            Some(address) => address,
            None => return false,
        },
    };
    matches_prefix(u32::from(address), 0xc612_0000, 15)
}

pub(super) fn is_public_v4(address: Ipv4Addr) -> bool {
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

pub(super) fn is_public_v6(address: Ipv6Addr) -> bool {
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

pub(super) fn matches_prefix(value: u32, network: u32, bits: u32) -> bool {
    value & (!0_u32 << (32 - bits)) == network
}

pub(super) fn matches_prefix_v6(value: u128, network: u128, bits: u32) -> bool {
    value & (!0_u128 << (128 - bits)) == network
}

pub(super) fn version_name(version: Version) -> &'static str {
    match version {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_11 => "HTTP/1.1",
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/unknown",
    }
}
