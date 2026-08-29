//! Embedded Console routes and the UI-internal Control API.

use super::state::{ConsoleCspNonce, ServiceState};
use crate::agent::AgentKind;
use crate::application_error::{ApplicationError, ApplicationErrorKind};
use crate::component::updates as component_updates;
use crate::component::{self, ComponentStatus};
use crate::config::model::{CustomProviderInput, VisualConfigOptionInput};
use crate::request::assessment::effective_assessment;
use crate::request::model::AssessmentLevel;
use crate::tenant::{self, ManagedTenant, Tenant, TenantSelection};
use crate::{config, docker, session};
use anyhow::{Context, Result};
use async_stream::stream;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderValue, Response, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use base64::Engine as _;
use bytes::Bytes;
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::Infallible;
use std::fs;
use std::path::Path as FsPath;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;

mod components;
mod configs;
mod operations;
mod overview;
mod requests;
mod routes;
mod sessions;
mod tenants;

pub(crate) use components::ComponentRow;
use components::component_rows;
#[cfg(test)]
pub(crate) use components::{
    ComponentMutation, ComponentQuery, InstalledComponentResponse, RemovedComponentResponse,
};
#[cfg(test)]
pub(crate) use configs::{
    AuthPropagationPreviewResponse, ConfigAuthResponse, ConfigDiagnostic, ConfigFileRequest,
    ConfigFileResponse, ConfigListResponse, ConfigMutationBase, CreatedConfigResponse,
    DeleteConfigsRequest, DeletedConfigsResponse, DiagnoseConfigRequest, DiagnoseConfigResponse,
    ExecuteAuthPropagationRequest, LinkedConfigFileResponse, SaveConfigFileRequest,
};
#[cfg(test)]
pub(crate) use operations::{
    BuildRequest, CancelledOperationResponse, OperationEnvelope, OperationQuery,
};
#[cfg(test)]
pub(crate) use overview::BootstrapResponse;
#[cfg(test)]
pub(crate) use overview::{
    DockerOverview, OverviewResponse, RequestOverview, RuntimeImageOverview, ServiceOverview,
    TopologyAgent, TopologyComponents, TopologyCurrentConfig, TopologyNamedConfigs,
    TopologyResponse, TopologyTenant,
};
#[cfg(test)]
pub(crate) use requests::{
    BodyQuery, DeleteRequest, DeletedRequestsResponse, DiagnosticGroups, EventTimingEntry,
    EventTimingQuery, EventTimingResponse, EventTimingState, ListQuery, RequestApiError,
    RequestDetail, RequestList, RequestState, RequestSummary, ResponseDetail,
};
#[cfg(test)]
pub(crate) use sessions::{
    DeleteSessionsRequest, DeletedSessionsResponse, SessionDetailFrame, SessionDetailQuery,
    SessionEvidenceQuery,
};
#[cfg(test)]
pub(crate) use tenants::TenantRow;
#[cfg(test)]
pub(crate) use tenants::{
    CreateTenantRequest, CreatedTenantResponse, DeleteSelection, DeletedTenantsResponse,
};

pub(crate) fn router() -> Router<ServiceState> {
    routes::router()
}

async fn index(Extension(csp_nonce): Extension<ConsoleCspNonce>) -> Response<Body> {
    requests::index(csp_nonce.as_str()).await
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

fn content(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_error_categories_have_stable_http_statuses() {
        let cases = [
            (ApplicationErrorKind::InvalidInput, StatusCode::BAD_REQUEST),
            (ApplicationErrorKind::NotFound, StatusCode::NOT_FOUND),
            (ApplicationErrorKind::Conflict, StatusCode::CONFLICT),
            (
                ApplicationErrorKind::InputTooLarge,
                StatusCode::PAYLOAD_TOO_LARGE,
            ),
            (ApplicationErrorKind::Busy, StatusCode::CONFLICT),
            (
                ApplicationErrorKind::Internal,
                StatusCode::INTERNAL_SERVER_ERROR,
            ),
        ];
        for (kind, expected) in cases {
            assert_eq!(status_for_application_error(kind), expected, "{kind:?}");
        }
    }

    #[test]
    fn unclassified_domain_errors_remain_bad_requests() {
        let response = result_error(anyhow::anyhow!("invalid selector"));
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
