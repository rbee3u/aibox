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

pub(crate) use components::ComponentRow;
use components::component_rows;
use response::content;

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

async fn blocking<T, F>(operation: F) -> Response<Body>
where
    T: Serialize + Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    match tokio::task::spawn_blocking(operation).await {
        Ok(Ok(value)) => json_response(StatusCode::OK, &value),
        Ok(Err(error)) => result_error(error),
        Err(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("management worker failed: {error}"),
        ),
    }
}

fn result_error(error: anyhow::Error) -> Response<Body> {
    let message = format!("{error:#}");
    let status = status_for_application_error(
        ApplicationError::kind(&error).unwrap_or(ApplicationErrorKind::InvalidInput),
    );
    api_error(status, &message)
}

/// What every fallible Control API handler returns.
pub(crate) type ControlResult = Result<Response<Body>, ControlError>;

/// A domain error on its way to a Control API response.
///
/// Handlers return `Result<Response<Body>, ControlError>` so wire decoding,
/// selector parsing, and coordinator calls can all use `?` instead of repeating
/// a `match` that maps every error to [`result_error`].
pub(crate) struct ControlError(anyhow::Error);

impl<E: Into<anyhow::Error>> From<E> for ControlError {
    fn from(error: E) -> Self {
        Self(error.into())
    }
}

impl axum::response::IntoResponse for ControlError {
    fn into_response(self) -> Response<Body> {
        result_error(self.0)
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

fn busy(message: &str) -> Response<Body> {
    api_error(StatusCode::CONFLICT, message)
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

fn api_error(status: StatusCode, message: &str) -> Response<Body> {
    let body = serde_json::to_vec(&ControlErrorResponse {
        error: ControlErrorBody {
            code: status.as_u16(),
            message,
        },
    })
    .unwrap_or_else(|_| b"{\"error\":{\"message\":\"Control API error\"}}".to_vec());
    content(status, "application/json; charset=utf-8", body)
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ControlErrorResponse<'a> {
    error: ControlErrorBody<'a>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ControlErrorBody<'a> {
    code: u16,
    message: &'a str,
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
