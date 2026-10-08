//! Embedded Console routes and the UI-internal Control API.

use super::state::{ConsoleCspNonce, ServiceState};
use crate::agent::AgentKind;
use crate::application_error::{ApplicationError, ApplicationErrorKind};
use anyhow::Result;
use axum::Router;
use axum::body::Body;
use axum::extract::Extension;
use axum::http::{Response, StatusCode};
use serde::{Deserialize, Serialize};

mod assets;
mod components;
mod configs;
#[cfg(test)]
mod contract;
mod operations;
mod overview;
mod requests;
mod response;
mod routes;
mod sessions;
mod tenants;

use components::component_rows_from;
pub(crate) use components::{ComponentRow, ComponentStatusWire};
use response::content;
pub(crate) use tenants::TenantRow;

pub(crate) fn router() -> Router<ServiceState> {
    routes::router()
}

async fn index(Extension(csp_nonce): Extension<ConsoleCspNonce>) -> Response<Body> {
    assets::index(csp_nonce.as_str()).await
}

fn default_tenant_selection() -> String {
    "managed:default".to_string()
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentTenantQuery {
    #[serde(default = "default_tenant_selection")]
    tenant: String,
    #[serde(default = "default_agent")]
    agent: AgentKind,
}

fn default_agent() -> AgentKind {
    AgentKind::Codex
}

/// What every fallible Control API handler returns.
pub(crate) type ControlResult = Result<Response<Body>, ControlError>;

/// A Control API failure, with its HTTP status and shared error envelope.
pub(crate) struct ControlError {
    status: StatusCode,
    message: String,
}

impl ControlError {
    fn new(status: StatusCode, message: impl ToString) -> Self {
        Self {
            status,
            message: message.to_string(),
        }
    }
}

impl<E: Into<anyhow::Error>> From<E> for ControlError {
    fn from(error: E) -> Self {
        let error = error.into();
        let status = status_for_application_error(
            ApplicationError::kind(&error).unwrap_or(ApplicationErrorKind::InvalidInput),
        );
        Self::new(status, format!("{error:#}"))
    }
}

impl axum::response::IntoResponse for ControlError {
    fn into_response(self) -> Response<Body> {
        api_error(self.status, &self.message)
    }
}

fn status_for_application_error(kind: ApplicationErrorKind) -> StatusCode {
    match kind {
        ApplicationErrorKind::InvalidInput => StatusCode::BAD_REQUEST,
        ApplicationErrorKind::NotFound => StatusCode::NOT_FOUND,
        ApplicationErrorKind::Conflict => StatusCode::CONFLICT,
        ApplicationErrorKind::InputTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        ApplicationErrorKind::Busy => StatusCode::CONFLICT,
        ApplicationErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn json_response<T: Serialize>(status: StatusCode, value: &T) -> Response<Body> {
    match serde_json::to_vec(value) {
        Ok(bytes) => content(status, "application/json; charset=utf-8", bytes),
        Err(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("serialize Control API response: {error}"),
        ),
    }
}

/// Render one Control API failure.
///
/// Every Control route returns the same `{"error":"<message>"}` envelope; the
/// HTTP status carries the failure category.
fn api_error(status: StatusCode, message: &str) -> Response<Body> {
    let body = serde_json::to_vec(&ControlErrorResponse { error: message })
        .unwrap_or_else(|_| b"{\"error\":\"Control API error\"}".to_vec());
    content(status, "application/json; charset=utf-8", body)
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ControlErrorResponse<'a> {
    error: &'a str,
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
