//! Plain-text error responses that end a Request attempt.
//!
//! Request streaming, response streaming, and upstream target validation share
//! this terminal path.

use super::attempt::RequestAttempt;
use crate::request::model::{ErrorKind, ErrorMetadata, Outcome};
use axum::body::Body;
use axum::http::{HeaderMap, Response, StatusCode, header};

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

fn response_with_headers(status: StatusCode, headers: HeaderMap, body: Body) -> Response<Body> {
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

/// An error response for a Request that has no attempt to finish, either because
/// opening it failed or because finishing it just did.
pub(super) fn bare_error(status: StatusCode, message: &str) -> Response<Body> {
    let mut response = Response::new(Body::from(format!("{message}\n")));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        axum::http::HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}
