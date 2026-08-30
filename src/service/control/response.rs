//! Small HTTP response construction helpers shared by Control adapters.

use axum::body::Body;
use axum::http::{HeaderValue, Response, StatusCode, header};

pub(super) fn content(
    status: StatusCode,
    content_type: &'static str,
    body: impl Into<Body>,
) -> Response<Body> {
    let mut response = Response::new(body.into());
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}
