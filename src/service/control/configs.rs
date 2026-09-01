//! Config Control API handlers and wire types.

use super::{
    AgentTenantQuery, ControlResult, default_agent, default_tenant_selection, json_response,
};
use crate::agent::AgentKind;
use crate::application_error::{ApplicationErrorKind, application_error};
use crate::config::{self, CustomProviderInput, VisualConfigOptionInput};
use crate::service::coordination::{ConfigCoordinator, ConfigFileView, DeleteConfigsCommand};
use crate::service::state::ServiceState;
use crate::tenant::TenantSelection;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ConfigListResponse {
    configs: Vec<config::ConfigCatalogEntry>,
    files: &'static [&'static str],
    application: config::ApplicationStatus,
    credential_propagation_available: bool,
}

/// Decode a base64 wire field, keeping the original Control API wording.
fn decode_base64(value: &str) -> Result<Vec<u8>, anyhow::Error> {
    base64::engine::general_purpose::STANDARD
        .decode(value.as_bytes())
        .map_err(|error| {
            application_error(
                ApplicationErrorKind::InvalidInput,
                format!("invalid base64: {error}"),
            )
        })
}

pub(super) async fn list_configs(
    State(state): State<ServiceState>,
    Query(query): Query<AgentTenantQuery>,
) -> ControlResult {
    let selection = TenantSelection::parse(&query.tenant)?;
    let catalog = ConfigCoordinator::new(state)
        .list(selection, query.agent)
        .await?;
    Ok(json_response(
        StatusCode::OK,
        &ConfigListResponse {
            configs: catalog.configs,
            files: catalog.files,
            application: catalog.application,
            credential_propagation_available: catalog.credential_propagation_available,
        },
    ))
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct AuthPropagationPreviewResponse {
    plan_id: String,
    preview: config::AuthPropagationPreview,
}

pub(super) async fn preview_auth_propagation(
    State(state): State<ServiceState>,
    Json(_request): Json<Value>,
) -> ControlResult {
    let preview = ConfigCoordinator::new(state)
        .preview_auth_propagation()
        .await?;
    Ok(json_response(
        StatusCode::OK,
        &AuthPropagationPreviewResponse {
            plan_id: preview.plan_id,
            preview: preview.preview,
        },
    ))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ExecuteAuthPropagationRequest {
    plan_id: String,
}

pub(super) async fn execute_auth_propagation(
    State(state): State<ServiceState>,
    Json(request): Json<ExecuteAuthPropagationRequest>,
) -> ControlResult {
    let report = ConfigCoordinator::new(state)
        .execute_auth_propagation(request.plan_id)
        .await?;
    Ok(json_response(StatusCode::OK, &report))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigMutationBase {
    #[serde(default = "default_tenant_selection")]
    tenant: String,
    #[serde(default = "default_agent")]
    agent: AgentKind,
    config: String,
}

pub(super) async fn create_config(
    State(state): State<ServiceState>,
    Json(request): Json<ConfigMutationBase>,
) -> ControlResult {
    let selection = TenantSelection::parse(&request.tenant)?;
    let name = config::NamedConfigName::parse(&request.config)?;
    let created = ConfigCoordinator::new(state)
        .create(selection, request.agent, name)
        .await?;
    Ok(json_response(
        StatusCode::OK,
        &CreatedConfigResponse { created },
    ))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigFileRequest {
    #[serde(default = "default_tenant_selection")]
    tenant: String,
    #[serde(default = "default_agent")]
    agent: AgentKind,
    #[serde(default)]
    current: bool,
    config: Option<String>,
    file: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ConfigFileResponse {
    file: String,
    exists: bool,
    revision: String,
    content_base64: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    visual_options: Option<Vec<config::VisualConfigOptionState>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    custom_provider: Option<crate::config::CustomProviderState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visual_error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    auth: Option<ConfigAuthResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    linked_file: Option<LinkedConfigFileResponse>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct LinkedConfigFileResponse {
    file: String,
    exists: bool,
    revision: String,
    content_base64: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ConfigAuthResponse {
    mode: &'static str,
    api_key: Option<String>,
    extra_fields: bool,
    warnings: Vec<String>,
}

pub(super) async fn reveal_config_file(
    State(state): State<ServiceState>,
    Json(request): Json<ConfigFileRequest>,
) -> ControlResult {
    let selection = TenantSelection::parse(&request.tenant)?;
    let target = config::ConfigTarget::from_wire(request.config.as_deref(), request.current)?;
    let file = config::ConfigFile::parse(request.agent, &request.file)?;
    let view = ConfigCoordinator::new(state)
        .reveal(selection, request.agent, target, file)
        .await?;
    Ok(json_response(StatusCode::OK, &config_file_response(view)))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct SaveConfigFileRequest {
    #[serde(default = "default_tenant_selection")]
    tenant: String,
    #[serde(default = "default_agent")]
    agent: AgentKind,
    #[serde(default)]
    current: bool,
    config: Option<String>,
    file: String,
    revision: String,
    content_base64: String,
    #[serde(default)]
    visual_options: Option<Vec<VisualConfigOptionInput>>,
    #[serde(default)]
    custom_provider: Option<CustomProviderInput>,
    #[serde(default)]
    visual_auth: Option<crate::config::VisualAuthInput>,
}

pub(super) async fn save_config_file(
    State(state): State<ServiceState>,
    Json(request): Json<SaveConfigFileRequest>,
) -> ControlResult {
    let selection = TenantSelection::parse(&request.tenant)?;
    let content = decode_base64(&request.content_base64)?;
    let target = config::ConfigTarget::from_wire(request.config.as_deref(), request.current)?;
    let file = config::ConfigFile::parse(request.agent, &request.file)?;
    let edit = config::ConfigEdit::from_wire(
        content,
        request.custom_provider,
        request.visual_options,
        request.visual_auth,
    )?;
    let view = ConfigCoordinator::new(state)
        .save(
            selection,
            request.agent,
            target,
            file,
            request.revision,
            edit,
        )
        .await?;
    Ok(json_response(StatusCode::OK, &config_file_response(view)))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct DiagnoseConfigRequest {
    #[serde(default = "default_tenant_selection")]
    tenant: String,
    #[serde(default = "default_agent")]
    agent: AgentKind,
    #[serde(default)]
    current: bool,
    config: Option<String>,
    file: String,
    content_base64: String,
}

/// One Config syntax error as the Console reads it.
///
/// This mirrors [`config::ConfigDiagnostic`] rather than reusing it because the
/// wire shape carries a `severity` the domain has no notion of. Where a wire
/// shape matches its domain type exactly, pass the domain type straight through
/// instead — `ProtocolSummary` does — and keep a second
/// name only for a genuine difference like this one. A field added to the
/// domain type has to be added here too before it reaches the Console.
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ConfigDiagnostic {
    severity: &'static str,
    message: String,
    line: usize,
    column: usize,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DiagnoseConfigResponse {
    diagnostics: Vec<ConfigDiagnostic>,
}

pub(super) async fn diagnose_config_file(
    State(state): State<ServiceState>,
    Json(request): Json<DiagnoseConfigRequest>,
) -> ControlResult {
    let selection = TenantSelection::parse(&request.tenant)?;
    let content = decode_base64(&request.content_base64)?;
    let target = config::ConfigTarget::from_wire(request.config.as_deref(), request.current)?;
    let file = config::ConfigFile::parse(request.agent, &request.file)?;
    let diagnostics = ConfigCoordinator::new(state)
        .diagnose(selection, request.agent, target, file, content)
        .await?;
    Ok(json_response(
        StatusCode::OK,
        &DiagnoseConfigResponse {
            diagnostics: diagnostics
                .into_iter()
                .map(|diagnostic| ConfigDiagnostic {
                    severity: "error",
                    message: diagnostic.message,
                    line: diagnostic.line,
                    column: diagnostic.column,
                })
                .collect(),
        },
    ))
}

pub(super) async fn apply_config(
    State(state): State<ServiceState>,
    Json(request): Json<ConfigMutationBase>,
) -> ControlResult {
    let selection = TenantSelection::parse(&request.tenant)?;
    let name = config::NamedConfigName::parse(&request.config)?;
    let application = ConfigCoordinator::new(state)
        .apply(selection, request.agent, name)
        .await?;
    Ok(json_response(StatusCode::OK, &application))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct DeleteConfigsRequest {
    #[serde(default = "default_tenant_selection")]
    tenant: String,
    #[serde(default = "default_agent")]
    agent: AgentKind,
    #[serde(default)]
    configs: Vec<String>,
    #[serde(default)]
    all: bool,
    confirmation: String,
}

pub(super) async fn delete_configs(
    State(state): State<ServiceState>,
    Json(request): Json<DeleteConfigsRequest>,
) -> ControlResult {
    let selection = TenantSelection::parse(&request.tenant)?;
    let command = DeleteConfigsCommand {
        selection,
        agent: request.agent,
        configs: request.configs,
        all: request.all,
        confirmation: request.confirmation,
    };
    let deleted = ConfigCoordinator::new(state).delete(command).await?;
    Ok(json_response(
        StatusCode::OK,
        &DeletedConfigsResponse {
            deleted: deleted.configs,
            all: deleted.all,
        },
    ))
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct CreatedConfigResponse {
    created: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DeletedConfigsResponse {
    deleted: Vec<String>,
    all: bool,
}

fn config_file_response(view: ConfigFileView) -> ConfigFileResponse {
    let visual_error = view.visual.error;
    let (visual_options, custom_provider) = view.visual.state.map_or((None, None), |state| {
        (Some(state.options), state.custom_provider)
    });
    let auth = view.auth.map(|auth| ConfigAuthResponse {
        mode: auth.mode,
        api_key: auth.api_key,
        extra_fields: auth.extra_fields,
        warnings: auth.warnings,
    });
    ConfigFileResponse {
        file: view.snapshot.file,
        exists: view.snapshot.exists,
        revision: view.snapshot.revision,
        content_base64: base64::engine::general_purpose::STANDARD.encode(view.snapshot.content),
        visual_options,
        custom_provider,
        visual_error,
        warnings: view.warnings,
        auth,
        linked_file: view.linked.map(linked_config_file_response),
    }
}

fn linked_config_file_response(snapshot: config::ConfigFileSnapshot) -> LinkedConfigFileResponse {
    LinkedConfigFileResponse {
        file: snapshot.file,
        exists: snapshot.exists,
        revision: snapshot.revision,
        content_base64: base64::engine::general_purpose::STANDARD.encode(snapshot.content),
    }
}
