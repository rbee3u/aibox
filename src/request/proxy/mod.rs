//! The forwarding path: proxy one request upstream and capture it as it streams.

mod attempt;
mod capture;
mod error_response;
mod headers;
mod request_stream;
mod response_stream;
mod retry;
mod target;

pub(super) use retry::RetryRules;

use attempt::RequestAttempt;
use error_response::{bare_error, finish_proxy_response, recording_failure};
use headers::{declared_content_length, forwarded_headers, recorded_headers, version_name};
use request_stream::{prepare_recorded_request_stream, reject_with_body, replay_body};
use response_stream::stream_upstream_response;
use target::{
    ReqwestUpstreamSender, UpstreamConnectError, UpstreamRequest, UpstreamSender, prepare_upstream,
    request_rejection, upstream_host, upstream_request_failure,
};

#[cfg(test)]
use crate::foundation::sync::lock_unpoisoned;
use crate::request::RequestProxyState;
use crate::request::interpretation::ProtocolObserver;
use crate::request::model::{ErrorKind, Outcome, RequestMetadata};
use crate::request::store::{ObservedRequest, RuntimeMeasurements};
use axum::body::Body;
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, Request, Response, StatusCode};
use futures_util::TryStreamExt as _;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use url::Url;

pub(crate) async fn handle(state: RequestProxyState, request: Request<Body>) -> Response<Body> {
    handle_with_sender(state, request, &ReqwestUpstreamSender).await
}

async fn handle_with_sender<S>(
    state: RequestProxyState,
    request: Request<Body>,
    sender: &S,
) -> Response<Body>
where
    S: UpstreamSender,
{
    let (parts, body) = request.into_parts();
    let incoming_uri = parts.uri.to_string();
    let candidate = incoming_uri.strip_prefix('/').unwrap_or_default();
    let parsed = Url::parse(candidate).ok();
    let upstream = parsed
        .as_ref()
        .filter(|url| matches!(url.scheme(), "http" | "https"));
    let ActiveRequest {
        mut guard,
        request_metadata,
    } = match begin_request(&state, &parts, &incoming_uri, upstream) {
        Ok(active_request) => active_request,
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
    let retry_enabled = state.retry_rules.matches(&url);

    let (connection, body) = match prepare_upstream(&state, &mut guard, body, &url, sender).await {
        Ok(prepared) => prepared,
        Err(response) => return *response,
    };

    let expected_body_bytes = declared_content_length(&parts.headers);
    let request_context = guard.request_stream_context(
        request_metadata.headers,
        expected_body_bytes,
        state.shutdown.clone(),
    );
    let request_stream =
        match prepare_recorded_request_stream(&mut guard, body, request_context).await {
            Ok(stream) => stream,
            Err(response) => return *response,
        };
    let headers = forwarded_headers(&parts.headers);
    let upstream_response = if retry_enabled {
        if let Err(error) = request_stream.try_for_each(|_| async { Ok(()) }).await {
            return upstream_request_failure(
                &mut guard,
                &target::UpstreamSendError {
                    message: error.to_string(),
                    timeout: false,
                },
            );
        }
        match send_with_retries(
            &state,
            sender,
            connection,
            &mut guard,
            parts.method,
            url,
            headers,
        )
        .await
        {
            Ok(response) => response,
            Err(response) => return *response,
        }
    } else {
        let upstream_request = UpstreamRequest {
            method: parts.method,
            url,
            headers,
            body: reqwest::Body::wrap_stream(request_stream),
        };
        let result = tokio::select! {
            () = state.shutdown.cancelled() => return shutdown_response(&mut guard),
            result = sender.send(connection, upstream_request) => result,
        };
        match result {
            Ok(response) => response,
            Err(error) => return upstream_request_failure(&mut guard, &error),
        }
    };

    stream_upstream_response(&state, upstream_response, guard)
}

async fn send_with_retries<S: UpstreamSender>(
    state: &RequestProxyState,
    sender: &S,
    mut connection: S::Connection,
    guard: &mut RequestAttempt,
    method: Method,
    url: Url,
    headers: HeaderMap,
) -> Result<reqwest::Response, Box<Response<Body>>> {
    let mut deadline = None;
    let mut is_retry = false;
    let mut previous_429 = None;
    loop {
        if is_retry && tokio::time::Instant::now() >= deadline.expect("retry has a deadline") {
            return Ok(previous_429.take().expect("retry follows HTTP 429"));
        }
        let body = replay_body(guard)?;
        if is_retry && tokio::time::Instant::now() >= deadline.expect("retry has a deadline") {
            return Ok(previous_429.take().expect("retry follows HTTP 429"));
        }
        if is_retry {
            guard.mark_retry_started().map_err(|error| {
                Box::new(recording_failure(
                    guard,
                    format!("checkpoint HTTP 429 retry: {error:#}"),
                ))
            })?;
        }
        let request = UpstreamRequest {
            method: method.clone(),
            url: url.clone(),
            headers: headers.clone(),
            body,
        };
        if is_retry && tokio::time::Instant::now() >= deadline.expect("retry has a deadline") {
            return Ok(previous_429.take().expect("retry follows HTTP 429"));
        }
        drop(previous_429.take());
        let response = tokio::select! {
            () = state.shutdown.cancelled() => return Err(Box::new(shutdown_response(guard))),
            result = sender.send(connection, request) => result,
        }
        .map_err(|error| Box::new(upstream_request_failure(guard, &error)))?;
        if response.status() != StatusCode::TOO_MANY_REQUESTS {
            return Ok(response);
        }
        guard.mark_rate_limited().map_err(|error| {
            Box::new(recording_failure(
                guard,
                format!("checkpoint HTTP 429 retry: {error:#}"),
            ))
        })?;
        let now = tokio::time::Instant::now();
        let end = *deadline.get_or_insert(now + Duration::from_secs(600));
        let next = now + Duration::from_secs(10);
        if next >= end {
            return Ok(response);
        }
        tokio::select! {
            () = state.shutdown.cancelled() => return Err(Box::new(shutdown_response(guard))),
            () = tokio::time::sleep_until(next) => {},
        }
        if tokio::time::Instant::now() >= end {
            return Ok(response);
        }
        connection = tokio::select! {
            () = state.shutdown.cancelled() => return Err(Box::new(shutdown_response(guard))),
            result = sender.connect(&url) => result,
        }
        .map_err(|error| Box::new(retry_connection_failure(guard, error)))?;
        if tokio::time::Instant::now() >= end {
            return Ok(response);
        }
        previous_429 = Some(response);
        is_retry = true;
    }
}

fn retry_connection_failure(
    guard: &mut RequestAttempt,
    error: UpstreamConnectError,
) -> Response<Body> {
    let (status, kind, message) = match error {
        UpstreamConnectError::InvalidTarget(message) => (
            StatusCode::BAD_REQUEST,
            ErrorKind::InvalidTargetUrl,
            message,
        ),
        UpstreamConnectError::Dns(message) => {
            (StatusCode::BAD_GATEWAY, ErrorKind::DnsError, message)
        }
        UpstreamConnectError::ClientConfiguration(message) => (
            StatusCode::BAD_GATEWAY,
            ErrorKind::ClientConfiguration,
            message,
        ),
    };
    finish_proxy_response(guard, status, &message, Outcome::UpstreamError, kind)
}

fn shutdown_response(guard: &mut RequestAttempt) -> Response<Body> {
    finish_proxy_response(
        guard,
        StatusCode::SERVICE_UNAVAILABLE,
        "AIBox Request Proxy is shutting down",
        Outcome::ServerShutdown,
        ErrorKind::ServerShutdown,
    )
}

struct ActiveRequest {
    guard: RequestAttempt,
    request_metadata: RequestMetadata,
}

fn begin_request(
    state: &RequestProxyState,
    parts: &Parts,
    incoming_uri: &str,
    upstream: Option<&Url>,
) -> anyhow::Result<ActiveRequest> {
    let host_hint = upstream.map(upstream_host);
    let (captured_request, request_metadata) = state.store.begin(ObservedRequest {
        method: parts.method.as_str(),
        incoming_uri,
        upstream_url: upstream.map(Url::as_str),
        http_version: version_name(parts.version),
        headers: recorded_headers(&parts.headers),
        host_hint: host_hint.as_deref(),
    })?;
    let measurements = Arc::new(Mutex::new(RuntimeMeasurements::default()));
    let protocol = Arc::new(Mutex::new(ProtocolObserver::new(
        request_metadata.upstream_url.as_deref(),
    )));
    let guard = RequestAttempt::new(
        state.store.clone(),
        captured_request,
        measurements.clone(),
        protocol.clone(),
    )
    .with_reporter(state.reporter.clone());
    Ok(ActiveRequest {
        guard,
        request_metadata,
    })
}

#[cfg(test)]
#[path = "proxy_tests.rs"]
mod tests;
