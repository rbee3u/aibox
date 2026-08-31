//! The forwarding path: proxy one request upstream and capture it as it streams.

mod attempt;
mod headers;
mod request_stream;
mod response_stream;
mod target;

use attempt::RequestAttempt;
use headers::{forwarded_headers, recorded_headers};
use request_stream::prepare_recorded_request_stream;
use response_stream::{
    bare_error, declared_content_length, finish_proxy_response, reject_with_body,
    stream_upstream_response,
};
use target::{
    ReqwestUpstreamSender, UpstreamRequest, UpstreamSender, prepare_upstream, request_rejection,
    upstream_host, upstream_request_failure, version_name,
};

#[cfg(test)]
use crate::foundation::sync::lock_unpoisoned;
use crate::request::RequestProxyState;
use crate::request::interpretation::ProtocolObserver;
use crate::request::model::{ErrorKind, Outcome, RequestMetadata};
use crate::request::store::{ObservedRequest, RuntimeMeasurements};
use axum::body::Body;
use axum::http::request::Parts;
use axum::http::{Request, Response, StatusCode};
use std::sync::{Arc, Mutex};
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
    let upstream_request = UpstreamRequest {
        method: parts.method.clone(),
        url,
        headers,
        body: reqwest::Body::wrap_stream(request_stream),
    };

    let upstream_response = tokio::select! {
        () = state.shutdown.cancelled() => {
            return finish_proxy_response(
                &mut guard,
                StatusCode::SERVICE_UNAVAILABLE,
                "AIBox Request Proxy is shutting down",
                Outcome::ServerShutdown,
                ErrorKind::ServerShutdown,
            );
        }
        result = sender.send(connection, upstream_request) => result,
    };
    let upstream_response = match upstream_response {
        Ok(response) => response,
        Err(error) => return upstream_request_failure(&mut guard, &error),
    };

    stream_upstream_response(&state, upstream_response, guard)
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
