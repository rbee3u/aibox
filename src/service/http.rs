//! HTTP composition, loopback management guard, and Request Proxy fallback.

use super::state::{ConsoleCspNonce, ServiceState};
use crate::request::handle_proxy;
use axum::Router;
use axum::body::Body;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderValue, Method, Response, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use std::net::{IpAddr, SocketAddr};

pub(super) fn router(state: ServiceState) -> Router {
    let protected = Router::new()
        .route("/", get(root_redirect))
        .merge(super::control_router())
        .route(
            "/_aibox",
            get(management_not_found).post(management_not_found),
        )
        .route(
            "/_aibox/{*path}",
            get(management_not_found).post(management_not_found),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            management_guard,
        ));
    Router::new()
        .merge(protected)
        .fallback(proxy_fallback)
        .with_state(state)
}

async fn root_redirect() -> Redirect {
    Redirect::temporary("/_aibox/ui/overview")
}

async fn management_not_found() -> Response<Body> {
    plain_error(StatusCode::NOT_FOUND, "AIBox management route not found")
}

async fn proxy_fallback(State(state): State<ServiceState>, request: Request) -> Response<Body> {
    handle_proxy(state.request(), request).await
}

async fn management_guard(
    State(state): State<ServiceState>,
    mut request: Request,
    next: Next,
) -> Response<Body> {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect| connect.0);
    if !peer.is_some_and(|peer| peer.ip().is_loopback()) {
        return plain_error(StatusCode::FORBIDDEN, "management access requires loopback");
    }
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    if !host.is_some_and(loopback_host) {
        return plain_error(
            StatusCode::FORBIDDEN,
            "management Host must resolve to loopback",
        );
    }
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        let content_type = request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok());
        if !content_type.is_some_and(|value| value.starts_with("application/json")) {
            return plain_error(StatusCode::UNSUPPORTED_MEDIA_TYPE, "expected JSON request");
        }
        let origin = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok());
        let expected_origin = format!("http://{}", host.expect("host checked above"));
        if origin != Some(expected_origin.as_str()) {
            return plain_error(StatusCode::FORBIDDEN, "request Origin is not same-origin");
        }
        let csrf = request
            .headers()
            .get("x-aibox-csrf")
            .and_then(|value| value.to_str().ok());
        if csrf != Some(state.csrf_token()) {
            return plain_error(StatusCode::FORBIDDEN, "invalid CSRF token");
        }
    }
    let csp_nonce = ConsoleCspNonce::new();
    request.extensions_mut().insert(csp_nonce.clone());
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&format!(
            "default-src 'self'; connect-src 'self'; img-src 'self' data:; script-src 'self'; style-src 'self' 'nonce-{}'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
            csp_nonce.as_str()
        ))
        .expect("generated CSP nonce produces a valid header"),
    );
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

fn loopback_host(value: &str) -> bool {
    let host = value
        .parse::<axum::http::uri::Authority>()
        .ok()
        .map(|authority| {
            authority
                .host()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string()
        });
    let Some(host) = host else { return false };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

fn plain_error(status: StatusCode, message: &str) -> Response<Body> {
    (status, message.to_string()).into_response()
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
