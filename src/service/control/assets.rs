//! Embedded Console static asset handlers.

use super::response::content;
use axum::body::Body;
use axum::http::{Response, StatusCode};

const HTML: &str = include_str!("../../../assets/console.html");
const CSS: &str = include_str!("../../../assets/console.css");
const JS: &str = include_str!("../../../assets/console.js");
const CSP_NONCE_PLACEHOLDER: &str = "__AIBOX_CSP_NONCE__";

pub(super) async fn index(csp_nonce: &str) -> Response<Body> {
    content(
        StatusCode::OK,
        "text/html; charset=utf-8",
        HTML.replacen(CSP_NONCE_PLACEHOLDER, csp_nonce, 1),
    )
}

pub(super) async fn css() -> Response<Body> {
    content(StatusCode::OK, "text/css; charset=utf-8", CSS)
}

pub(super) async fn js() -> Response<Body> {
    content(StatusCode::OK, "application/javascript; charset=utf-8", JS)
}
