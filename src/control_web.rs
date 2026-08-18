//! Embedded Console routes and the UI-internal Control API.

use crate::agent::AgentKind;
use crate::component::{self, ComponentKind, ComponentSpec, ComponentStatus};
use crate::request_assessment::effective_assessment;
use crate::request_store::AssessmentLevel;
use crate::service::{PendingAuthPropagation, ServiceState};
use crate::tenant::{self, ManagedTenant, Tenant};
use crate::{config, docker, request_web, session};
use anyhow::{Context, Result};
use async_stream::stream;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, Response, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use base64::Engine as _;
use bytes::Bytes;
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::fs;
use std::path::Path as FsPath;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::ReceiverStream;

pub(crate) fn router() -> Router<ServiceState> {
    Router::new()
        .route("/_aibox/ui", get(index))
        .route("/_aibox/ui/app.css", get(request_web::css))
        .route("/_aibox/ui/app.js", get(request_web::js))
        .route("/_aibox/ui/{*path}", get(index))
        .route("/_aibox/requests/app.css", get(request_web::css))
        .route("/_aibox/requests/app.js", get(request_web::js))
        .route("/_aibox/api/bootstrap", get(bootstrap))
        .route("/_aibox/api/overview", get(overview))
        .route("/_aibox/api/topology", get(topology))
        .route("/_aibox/api/tenants", get(list_tenants).post(create_tenant))
        .route("/_aibox/api/tenants/delete", post(delete_tenants))
        .route("/_aibox/api/components", get(list_components))
        .route("/_aibox/api/components/install", post(install_component))
        .route("/_aibox/api/components/remove", post(remove_component))
        .route("/_aibox/api/configs", get(list_configs))
        .route("/_aibox/api/configs/create", post(create_config))
        .route("/_aibox/api/configs/reveal", post(reveal_config_file))
        .route("/_aibox/api/configs/save", post(save_config_file))
        .route("/_aibox/api/configs/apply", post(apply_config))
        .route("/_aibox/api/configs/delete", post(delete_configs))
        .route(
            "/_aibox/api/configs/propagate-auth/preview",
            post(preview_auth_propagation),
        )
        .route(
            "/_aibox/api/configs/propagate-auth/execute",
            post(execute_auth_propagation),
        )
        .route("/_aibox/api/sessions", get(list_sessions))
        .route("/_aibox/api/sessions/summary", get(session_summary))
        .route("/_aibox/api/sessions/prompts", get(session_prompts))
        .route("/_aibox/api/sessions/delete", post(delete_sessions))
        .route("/_aibox/api/operations/current", get(current_operation))
        .route("/_aibox/api/operations/events", get(operation_events))
        .route("/_aibox/api/operations/build", post(start_build))
        .route("/_aibox/api/operations/{id}/cancel", post(cancel_operation))
}

async fn index() -> Response<Body> {
    request_web::index().await
}

#[derive(Serialize)]
struct BootstrapResponse {
    version: &'static str,
    csrf_token: String,
}

async fn bootstrap(State(state): State<ServiceState>) -> Json<BootstrapResponse> {
    Json(BootstrapResponse {
        version: env!("CARGO_PKG_VERSION"),
        csrf_token: state.csrf.to_string(),
    })
}

#[derive(Serialize)]
struct OverviewResponse {
    service: ServiceOverview,
    docker: DockerOverview,
    runtime_image: RuntimeImageOverview,
    managed_tenants: usize,
    host_available: bool,
    requests: RequestOverview,
}

#[derive(Serialize)]
struct ServiceOverview {
    version: &'static str,
    listen: String,
    uptime_seconds: u64,
    aibox_root: String,
}

#[derive(Serialize)]
struct DockerOverview {
    status: &'static str,
    error: Option<String>,
}

#[derive(Serialize)]
struct RuntimeImageOverview {
    reference: String,
    status: &'static str,
    id: Option<String>,
    created_at: Option<String>,
    size_bytes: Option<u64>,
    detail: Option<String>,
}

#[derive(Default, Serialize)]
struct RequestOverview {
    total: usize,
    active: usize,
    warning: usize,
    error: usize,
    bytes: u64,
}

async fn overview(State(state): State<ServiceState>) -> Response<Body> {
    let root = state.root.clone();
    let host_home = state.host_home.clone();
    let image = state.image.clone();
    let request = state.request.store.clone();
    let listen = state.listen;
    let uptime = state.started.elapsed().as_secs();
    blocking(move || {
        let tenants = tenant::list_tenants(&root)?;
        let host_available = tenant::real_dir_exists(&host_home, "Host Home")?;
        let (docker, runtime_image) = match docker::inspect_runtime_image(image.as_str()) {
            Ok(inspection) => (
                DockerOverview {
                    status: "available",
                    error: None,
                },
                RuntimeImageOverview {
                    reference: image.to_string(),
                    status: if inspection.present {
                        "built"
                    } else {
                        "missing"
                    },
                    id: inspection.id,
                    created_at: inspection.created_at,
                    size_bytes: inspection.size_bytes,
                    detail: inspection.detail,
                },
            ),
            Err(error) => (
                DockerOverview {
                    status: "unavailable",
                    error: Some(format!("{error:#}")),
                },
                RuntimeImageOverview {
                    reference: image.to_string(),
                    status: "unknown",
                    id: None,
                    created_at: None,
                    size_bytes: None,
                    detail: None,
                },
            ),
        };
        let records = request.scan_summaries()?;
        let mut requests = RequestOverview {
            total: records.len(),
            bytes: directory_size(request.root())?,
            ..RequestOverview::default()
        };
        for record in records {
            match effective_assessment(&record.summary, record.active).level {
                AssessmentLevel::Active => requests.active += 1,
                AssessmentLevel::Warning => requests.warning += 1,
                AssessmentLevel::Error => requests.error += 1,
                AssessmentLevel::Ok => {}
            }
        }
        Ok(OverviewResponse {
            service: ServiceOverview {
                version: env!("CARGO_PKG_VERSION"),
                listen: listen.to_string(),
                uptime_seconds: uptime,
                aibox_root: root.display().to_string(),
            },
            docker,
            runtime_image,
            managed_tenants: tenants.len(),
            host_available,
            requests,
        })
    })
    .await
}

#[derive(Serialize)]
struct TopologyResponse {
    tenants: Vec<TopologyTenant>,
}

#[derive(Serialize)]
struct TopologyTenant {
    kind: &'static str,
    name: Option<String>,
    display_name: String,
    home: String,
    exists: bool,
    agents: Vec<TopologyAgent>,
    components: TopologyComponents,
}

#[derive(Serialize)]
struct TopologyAgent {
    agent: AgentKind,
    current_config: TopologyCurrentConfig,
    named_configs: TopologyNamedConfigs,
    application: config::ApplicationStatus,
}

#[derive(Serialize)]
struct TopologyCurrentConfig {
    present_files: usize,
    expected_files: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct TopologyNamedConfigs {
    entries: Vec<config::ConfigCatalogEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct TopologyComponents {
    entries: Vec<ComponentRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

async fn topology(State(state): State<ServiceState>) -> Response<Body> {
    let root = state.root.clone();
    let host_home = state.host_home.clone();
    blocking(move || {
        let host_exists = tenant::real_dir_exists(&host_home, "Host Home")?;
        let mut selected = vec![(
            Tenant::Host {
                home_dir: host_home.as_ref().clone(),
                root_dir: root.as_ref().clone(),
            },
            "host",
            None,
            "Host Tenant".to_string(),
            host_exists,
        )];
        for name in tenant::list_tenants(&root)? {
            let managed = ManagedTenant::resolve(&root, &name)?;
            selected.push((
                Tenant::Managed(managed),
                "managed",
                Some(name.clone()),
                name,
                true,
            ));
        }
        let tenants = selected
            .into_iter()
            .map(|(tenant, kind, name, display_name, exists)| {
                let agents = [AgentKind::Codex, AgentKind::Claude]
                    .into_iter()
                    .map(|agent| topology_agent(&tenant, agent))
                    .collect();
                let components = match component_rows(&tenant) {
                    Ok(entries) => TopologyComponents {
                        entries,
                        error: None,
                    },
                    Err(error) => TopologyComponents {
                        entries: Vec::new(),
                        error: Some(format!("{error:#}")),
                    },
                };
                TopologyTenant {
                    kind,
                    name,
                    display_name,
                    home: tenant.home_dir().display().to_string(),
                    exists,
                    agents,
                    components,
                }
            })
            .collect();
        Ok(TopologyResponse { tenants })
    })
    .await
}

fn topology_agent(tenant: &Tenant, agent: AgentKind) -> TopologyAgent {
    let selected = tenant.for_agent(agent);
    let current_config = match config::inspect_current_config(&selected) {
        Ok(inspection) => TopologyCurrentConfig {
            present_files: inspection.present_files,
            expected_files: inspection.expected_files,
            error: None,
        },
        Err(error) => TopologyCurrentConfig {
            present_files: 0,
            expected_files: agent.config_files().len(),
            error: Some(format!("{error:#}")),
        },
    };
    let named_configs = match config::inspect_named_configs(&selected) {
        Ok(entries) => TopologyNamedConfigs {
            entries,
            error: None,
        },
        Err(error) => TopologyNamedConfigs {
            entries: Vec::new(),
            error: Some(format!("{error:#}")),
        },
    };
    TopologyAgent {
        agent,
        current_config,
        named_configs,
        application: config::application_status(&selected),
    }
}

#[derive(Serialize)]
struct TenantRow {
    kind: &'static str,
    name: Option<String>,
    display_name: String,
    home: String,
    exists: bool,
}

async fn list_tenants(State(state): State<ServiceState>) -> Response<Body> {
    let root = state.root.clone();
    let host_home = state.host_home.clone();
    blocking(move || {
        let host_exists = tenant::real_dir_exists(&host_home, "Host Home")?;
        let mut rows = vec![TenantRow {
            kind: "host",
            name: None,
            display_name: "Host Tenant".to_string(),
            home: host_home.display().to_string(),
            exists: host_exists,
        }];
        for name in tenant::list_tenants(&root)? {
            let managed = ManagedTenant::resolve(&root, &name)?;
            rows.push(TenantRow {
                kind: "managed",
                name: Some(name.clone()),
                display_name: name,
                home: managed.home_dir.display().to_string(),
                exists: true,
            });
        }
        Ok(rows)
    })
    .await
}

#[derive(Deserialize)]
struct CreateTenantRequest {
    name: String,
}

async fn create_tenant(
    State(state): State<ServiceState>,
    Json(request): Json<CreateTenantRequest>,
) -> Response<Body> {
    let guard = match state.mutation.clone().try_lock_owned() {
        Ok(guard) => guard,
        Err(_) => return busy("another management mutation is running"),
    };
    let root = state.root.clone();
    blocking(move || {
        let _guard = guard;
        let tenant = ManagedTenant::resolve(&root, &request.name)?;
        tenant.ensure_initialized()?;
        Ok(json!({"created": request.name, "home": tenant.home_dir}))
    })
    .await
}

#[derive(Deserialize)]
struct DeleteSelection {
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    all: bool,
    confirmation: String,
}

async fn delete_tenants(
    State(state): State<ServiceState>,
    Json(request): Json<DeleteSelection>,
) -> Response<Body> {
    if request.all && request.confirmation != "delete all tenants" {
        return api_error(StatusCode::BAD_REQUEST, "confirmation does not match");
    }
    if !request.all && request.names.len() == 1 && request.confirmation != request.names[0] {
        return api_error(
            StatusCode::BAD_REQUEST,
            "confirmation does not match Tenant name",
        );
    }
    let guard = match state.mutation.clone().try_lock_owned() {
        Ok(guard) => guard,
        Err(_) => return busy("another management mutation is running"),
    };
    let root = state.root.clone();
    blocking(move || {
        let _guard = guard;
        tenant::delete_tenants(&root, &request.names, request.all, true)?;
        Ok(json!({"deleted": request.names, "all": request.all}))
    })
    .await
}

#[derive(Clone, Deserialize)]
struct Scope {
    #[serde(default = "managed_scope")]
    scope: String,
    tenant: Option<String>,
}

fn managed_scope() -> String {
    "managed".to_string()
}

#[derive(Deserialize)]
struct ComponentQuery {
    #[serde(flatten)]
    scope: Scope,
}

#[derive(Serialize)]
struct ComponentRow {
    kind: String,
    supports_version: bool,
    status: Option<String>,
    version: Option<String>,
    error: Option<String>,
}

async fn list_components(
    State(state): State<ServiceState>,
    Query(query): Query<ComponentQuery>,
) -> Response<Body> {
    let selected = match resolve_tenant(&state, &query.scope) {
        Ok(selected) => selected,
        Err(error) => return result_error(error),
    };
    blocking(move || component_rows(&selected)).await
}

fn component_rows(selected: &Tenant) -> Result<Vec<ComponentRow>> {
    Ok(component::inspect_catalog(selected)?
        .into_iter()
        .map(|inspection| {
            let (status, version) = inspection.status.map_or((None, None), |status| {
                let version = match &status {
                    ComponentStatus::Installed { version } => version.clone(),
                    _ => None,
                };
                (Some(component_status_name(&status).to_string()), version)
            });
            ComponentRow {
                kind: inspection.kind.name().to_string(),
                supports_version: inspection.kind.supports_version(),
                status,
                version,
                error: inspection.error,
            }
        })
        .collect())
}

#[derive(Deserialize)]
struct ComponentMutation {
    #[serde(flatten)]
    scope: Scope,
    component: String,
    version: Option<String>,
}

async fn install_component(
    State(state): State<ServiceState>,
    Json(request): Json<ComponentMutation>,
) -> Response<Body> {
    let selected = match resolve_tenant(&state, &request.scope) {
        Ok(selected) => selected,
        Err(error) => return result_error(error),
    };
    let spec_text = request.version.as_ref().map_or_else(
        || request.component.clone(),
        |version| format!("{}@{version}", request.component),
    );
    let spec = match spec_text.parse::<ComponentSpec>() {
        Ok(spec) => spec,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, &error),
    };
    if spec.kind.is_statusline() {
        let guard = match state.mutation.clone().try_lock_owned() {
            Ok(guard) => guard,
            Err(_) => return busy("another management mutation is running"),
        };
        return blocking(move || {
            let _guard = guard;
            component::install_component(&selected, &spec)?;
            Ok(json!({"installed": spec.to_string()}))
        })
        .await;
    }
    let guard = match state.mutation.clone().try_lock_owned() {
        Ok(guard) => guard,
        Err(_) => return busy("another management mutation is running"),
    };
    let label = format!("install {spec}");
    match state.operations.start(label, move |context| {
        let _guard = guard;
        context.log(format!("Installing {spec}"));
        component::install_component_for_service(&selected, &spec)?;
        Ok(format!("Installed {spec}"))
    }) {
        Ok(operation) => json_response(StatusCode::ACCEPTED, &operation),
        Err(error) => busy(&error.to_string()),
    }
}

async fn remove_component(
    State(state): State<ServiceState>,
    Json(request): Json<ComponentMutation>,
) -> Response<Body> {
    let selected = match resolve_tenant(&state, &request.scope) {
        Ok(selected) => selected,
        Err(error) => return result_error(error),
    };
    let kind = match request.component.parse::<ComponentKind>() {
        Ok(kind) => kind,
        Err(error) => return api_error(StatusCode::BAD_REQUEST, &error),
    };
    let guard = match state.mutation.clone().try_lock_owned() {
        Ok(guard) => guard,
        Err(_) => return busy("another management mutation is running"),
    };
    blocking(move || {
        let _guard = guard;
        component::remove_component(&selected, kind)?;
        Ok(json!({"removed": kind.name()}))
    })
    .await
}

#[derive(Deserialize)]
struct AgentScopeQuery {
    #[serde(flatten)]
    scope: Scope,
    #[serde(default = "default_agent")]
    agent: AgentKind,
}

fn default_agent() -> AgentKind {
    AgentKind::Codex
}

#[derive(Serialize)]
struct ConfigListResponse {
    named_configs: Vec<String>,
    configs: Vec<config::ConfigCatalogEntry>,
    files: &'static [&'static str],
    application: config::ApplicationStatus,
    credential_propagation_available: bool,
}

async fn list_configs(
    State(state): State<ServiceState>,
    Query(query): Query<AgentScopeQuery>,
) -> Response<Body> {
    let selected = match resolve_agent(&state, &query.scope, query.agent) {
        Ok(selected) => selected,
        Err(error) => return result_error(error),
    };
    let check_credential_propagation =
        query.scope.scope == "host" && query.agent == AgentKind::Codex;
    let root = state.root.clone();
    let host_home = state.host_home.clone();
    blocking(move || {
        let configs = config::inspect_named_configs(&selected)?;
        let credential_propagation_available = check_credential_propagation
            && config::credential_propagation_source_available(&root, &host_home)?;
        Ok(ConfigListResponse {
            named_configs: configs
                .iter()
                .filter(|entry| entry.state == "ready")
                .map(|entry| entry.name.clone())
                .collect(),
            configs,
            files: selected.agent.config_files(),
            application: config::application_status(&selected),
            credential_propagation_available,
        })
    })
    .await
}

#[derive(Serialize)]
struct AuthPropagationPreviewResponse {
    plan_id: String,
    preview: config::AuthPropagationPreview,
}

async fn preview_auth_propagation(
    State(state): State<ServiceState>,
    Json(_request): Json<Value>,
) -> Response<Body> {
    let root = state.root.clone();
    let host_home = state.host_home.clone();
    let plans = state.auth_propagation.clone();
    blocking(move || {
        let plan = config::plan_auth_propagation_from(&root, &host_home)?;
        let preview = config::preview_auth_propagation(&plan);
        let plan_id = uuid::Uuid::now_v7().to_string();
        *plans
            .lock()
            .expect("Credential Propagation plan store poisoned") = Some(PendingAuthPropagation {
            id: plan_id.clone(),
            plan,
        });
        Ok(AuthPropagationPreviewResponse { plan_id, preview })
    })
    .await
}

#[derive(Deserialize)]
struct ExecuteAuthPropagationRequest {
    plan_id: String,
}

async fn execute_auth_propagation(
    State(state): State<ServiceState>,
    Json(request): Json<ExecuteAuthPropagationRequest>,
) -> Response<Body> {
    let plans = state.auth_propagation.clone();
    mutate_blocking(state, move || {
        let plan = {
            let mut pending = plans
                .lock()
                .expect("Credential Propagation plan store poisoned");
            if !pending
                .as_ref()
                .is_some_and(|plan| plan.id == request.plan_id)
            {
                anyhow::bail!("Credential Propagation plan is missing or obsolete");
            }
            pending.take().expect("plan checked above").plan
        };
        Ok(config::execute_auth_propagation(plan))
    })
    .await
}

#[derive(Deserialize)]
struct ConfigMutationBase {
    #[serde(flatten)]
    scope: Scope,
    #[serde(default = "default_agent")]
    agent: AgentKind,
    config: String,
}

async fn create_config(
    State(state): State<ServiceState>,
    Json(request): Json<ConfigMutationBase>,
) -> Response<Body> {
    let selected = match resolve_agent(&state, &request.scope, request.agent) {
        Ok(selected) => selected,
        Err(error) => return result_error(error),
    };
    mutate_blocking(state, move || {
        config::create_named_config(&selected, &request.config)?;
        Ok(json!({"created": request.config}))
    })
    .await
}

#[derive(Deserialize)]
struct ConfigFileRequest {
    #[serde(flatten)]
    scope: Scope,
    #[serde(default = "default_agent")]
    agent: AgentKind,
    #[serde(default)]
    current: bool,
    config: Option<String>,
    file: String,
}

#[derive(Serialize)]
struct ConfigFileResponse {
    file: String,
    exists: bool,
    revision: String,
    content_base64: String,
}

async fn reveal_config_file(
    State(state): State<ServiceState>,
    Json(request): Json<ConfigFileRequest>,
) -> Response<Body> {
    let selected = match resolve_agent(&state, &request.scope, request.agent) {
        Ok(selected) => selected,
        Err(error) => return result_error(error),
    };
    blocking(move || {
        let snapshot = config::read_config_file(
            &selected,
            request.config.as_deref(),
            request.current,
            &request.file,
        )?;
        Ok(config_file_response(snapshot))
    })
    .await
}

#[derive(Deserialize)]
struct SaveConfigFileRequest {
    #[serde(flatten)]
    selection: ConfigFileRequest,
    revision: String,
    content_base64: String,
}

async fn save_config_file(
    State(state): State<ServiceState>,
    Json(request): Json<SaveConfigFileRequest>,
) -> Response<Body> {
    let selected = match resolve_agent(&state, &request.selection.scope, request.selection.agent) {
        Ok(selected) => selected,
        Err(error) => return result_error(error),
    };
    let content =
        match base64::engine::general_purpose::STANDARD.decode(request.content_base64.as_bytes()) {
            Ok(content) => content,
            Err(error) => {
                return api_error(StatusCode::BAD_REQUEST, &format!("invalid base64: {error}"));
            }
        };
    mutate_blocking(state, move || {
        let snapshot = config::save_config_file(
            &selected,
            request.selection.config.as_deref(),
            request.selection.current,
            &request.selection.file,
            &request.revision,
            &content,
        )?;
        Ok(config_file_response(snapshot))
    })
    .await
}

async fn apply_config(
    State(state): State<ServiceState>,
    Json(request): Json<ConfigMutationBase>,
) -> Response<Body> {
    let selected = match resolve_agent(&state, &request.scope, request.agent) {
        Ok(selected) => selected,
        Err(error) => return result_error(error),
    };
    mutate_blocking(state, move || {
        config::apply_named_config(&selected, &request.config)?;
        Ok(config::application_status(&selected))
    })
    .await
}

#[derive(Deserialize)]
struct DeleteConfigsRequest {
    #[serde(flatten)]
    scope: Scope,
    #[serde(default = "default_agent")]
    agent: AgentKind,
    #[serde(default)]
    configs: Vec<String>,
    #[serde(default)]
    all: bool,
    confirmation: String,
}

async fn delete_configs(
    State(state): State<ServiceState>,
    Json(request): Json<DeleteConfigsRequest>,
) -> Response<Body> {
    if request.all && request.confirmation != "delete all configs" {
        return api_error(StatusCode::BAD_REQUEST, "confirmation does not match");
    }
    let selected = match resolve_agent(&state, &request.scope, request.agent) {
        Ok(selected) => selected,
        Err(error) => return result_error(error),
    };
    mutate_blocking(state, move || {
        config::delete_named_configs(&selected, &request.configs, request.all, true)?;
        Ok(json!({"deleted": request.configs, "all": request.all}))
    })
    .await
}

async fn list_sessions(
    State(state): State<ServiceState>,
    Query(query): Query<AgentScopeQuery>,
) -> Response<Body> {
    let tenant = match resolve_tenant(&state, &query.scope) {
        Ok(tenant) => tenant,
        Err(error) => return result_error(error),
    };
    if let Err(error) = tenant.validate_session_home() {
        return result_error(error);
    }
    let home = tenant.home_dir().to_path_buf();
    blocking(move || {
        let backend = session::backend_for(query.agent);
        session::list_data(backend.as_ref(), &home)
    })
    .await
}

async fn session_summary(
    State(state): State<ServiceState>,
    Query(query): Query<AgentScopeQuery>,
) -> Response<Body> {
    let tenant = match resolve_tenant(&state, &query.scope) {
        Ok(tenant) => tenant,
        Err(error) => return result_error(error),
    };
    if let Err(error) = tenant.validate_session_home() {
        return result_error(error);
    }
    let home = tenant.home_dir().to_path_buf();
    blocking(move || {
        let backend = session::backend_for(query.agent);
        session::discovery_summary(backend.as_ref(), &home)
    })
    .await
}

#[derive(Deserialize)]
struct SessionDetailQuery {
    #[serde(flatten)]
    selection: AgentScopeQuery,
    id: String,
}

async fn session_prompts(
    State(state): State<ServiceState>,
    Query(query): Query<SessionDetailQuery>,
) -> Response<Body> {
    let tenant = match resolve_tenant(&state, &query.selection.scope) {
        Ok(tenant) => tenant,
        Err(error) => return result_error(error),
    };
    if let Err(error) = tenant.validate_session_home() {
        return result_error(error);
    }
    let home = tenant.home_dir().to_path_buf();
    let agent = query.selection.agent;
    let id = query.id;
    let (sender, receiver) = tokio::sync::mpsc::channel::<Bytes>(8);
    tokio::task::spawn_blocking(move || {
        let backend = session::backend_for(agent);
        let result = session::stream_prompt_data(backend.as_ref(), &home, &id, &mut |prompt| {
            send_ndjson(&sender, &json!({"type": "prompt", "prompt": prompt}))
        });
        match result {
            Ok((resolved_id, warnings)) => {
                let _ = send_ndjson(
                    &sender,
                    &json!({"type": "complete", "id": resolved_id, "warnings": warnings}),
                );
            }
            Err(error) => {
                let _ = send_ndjson(
                    &sender,
                    &json!({"type": "error", "agent": agent, "error": format!("{error:#}")}),
                );
            }
        }
    });
    let stream = ReceiverStream::new(receiver).map(Ok::<Bytes, Infallible>);
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    response
}

fn send_ndjson(sender: &tokio::sync::mpsc::Sender<Bytes>, value: &Value) -> Result<bool> {
    let mut line = serde_json::to_vec(value).context("serialize Session stream record")?;
    line.push(b'\n');
    Ok(sender.blocking_send(Bytes::from(line)).is_ok())
}

#[derive(Deserialize)]
struct DeleteSessionsRequest {
    #[serde(flatten)]
    selection: AgentScopeQuery,
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default)]
    all: bool,
    confirmation: String,
}

async fn delete_sessions(
    State(state): State<ServiceState>,
    Json(request): Json<DeleteSessionsRequest>,
) -> Response<Body> {
    if request.all && request.confirmation != "delete all sessions" {
        return api_error(StatusCode::BAD_REQUEST, "confirmation does not match");
    }
    let tenant = match resolve_tenant(&state, &request.selection.scope) {
        Ok(tenant) => tenant,
        Err(error) => return result_error(error),
    };
    if let Err(error) = tenant.validate_session_home() {
        return result_error(error);
    }
    let home = tenant.home_dir().to_path_buf();
    mutate_blocking(state, move || {
        let backend = session::backend_for(request.selection.agent);
        let deleted = session::delete_sessions(backend.as_ref(), &home, &request.ids, request.all)?;
        Ok(json!({"deleted": deleted}))
    })
    .await
}

#[derive(Deserialize)]
struct OperationQuery {
    after_sequence: Option<u64>,
}

async fn current_operation(
    State(state): State<ServiceState>,
    Query(query): Query<OperationQuery>,
) -> Response<Body> {
    let mut operation = state.operations.snapshot();
    let gap = operation.as_ref().is_some_and(|snapshot| {
        query
            .after_sequence
            .is_some_and(|sequence| sequence < snapshot.first_sequence)
    });
    if let (Some(snapshot), Some(sequence)) = (&mut operation, query.after_sequence) {
        snapshot.logs.retain(|entry| entry.sequence >= sequence);
    }
    json_response(StatusCode::OK, &json!({"operation": operation, "gap": gap}))
}

async fn operation_events(
    State(state): State<ServiceState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let manager = state.operations.clone();
    let shutdown = state.request.shutdown.clone();
    let mut changes = manager.subscribe();
    let events = stream! {
        let mut operation_id: Option<String> = None;
        let mut after_sequence = 0_u64;
        loop {
            let mut operation = manager.snapshot();
            if operation.as_ref().map(|snapshot| &snapshot.id) != operation_id.as_ref() {
                operation_id = operation.as_ref().map(|snapshot| snapshot.id.clone());
                after_sequence = 0;
            }
            let gap = operation.as_ref().is_some_and(|snapshot| {
                after_sequence < snapshot.first_sequence
            });
            if let Some(snapshot) = &mut operation {
                snapshot.logs.retain(|entry| entry.sequence >= after_sequence);
                after_sequence = snapshot.next_sequence;
            } else {
                after_sequence = 0;
            }
            let payload = serde_json::to_string(&json!({"operation": operation, "gap": gap}))
                .unwrap_or_else(|_| "{\"operation\":null,\"gap\":false}".to_string());
            yield Ok(Event::default().event("operation").data(payload));
            tokio::select! {
                change = changes.recv() => match change {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                () = shutdown.cancelled() => break,
            }
        }
    };
    Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[derive(Deserialize)]
struct BuildRequest {
    #[serde(default)]
    force: bool,
}

async fn start_build(
    State(state): State<ServiceState>,
    Json(request): Json<BuildRequest>,
) -> Response<Body> {
    let image = state.image.clone();
    let kind = if request.force {
        "build image without cache"
    } else {
        "build image"
    };
    match state.operations.start(kind, move |context| {
        let cache = if request.force {
            docker::BuildCache::NoCachePull
        } else {
            docker::BuildCache::Cached
        };
        context.log(format!("Building {image}"));
        let log_context = context.clone();
        let log: Arc<dyn Fn(String) + Send + Sync> = Arc::new(move |line| {
            log_context.log(line);
        });
        docker::build_image_for_service(
            &docker::DockerCli::system(),
            docker::DOCKERFILE,
            &image,
            cache,
            context.cancellation(),
            log,
        )?;
        Ok(format!("Built {image}"))
    }) {
        Ok(operation) => json_response(StatusCode::ACCEPTED, &operation),
        Err(error) => busy(&error.to_string()),
    }
}

async fn cancel_operation(
    State(state): State<ServiceState>,
    Path(id): Path<String>,
    Json(_request): Json<Value>,
) -> Response<Body> {
    match state.operations.cancel(&id) {
        Ok(()) => json_response(StatusCode::ACCEPTED, &json!({"cancelled": id})),
        Err(error) => result_error(error),
    }
}

fn resolve_tenant(state: &ServiceState, scope: &Scope) -> Result<Tenant> {
    match scope.scope.as_str() {
        "host" => Ok(Tenant::Host {
            home_dir: state.host_home.as_ref().clone(),
            root_dir: state.root.as_ref().clone(),
        }),
        "managed" => Ok(Tenant::Managed(ManagedTenant::resolve(
            &state.root,
            scope.tenant.as_deref().unwrap_or("default"),
        )?)),
        value => anyhow::bail!("unknown Tenant scope: {value}"),
    }
}

fn resolve_agent(
    state: &ServiceState,
    scope: &Scope,
    agent: AgentKind,
) -> Result<crate::tenant::TenantAgent> {
    Ok(resolve_tenant(state, scope)?.for_agent(agent))
}

fn config_file_response(snapshot: config::ConfigFileSnapshot) -> ConfigFileResponse {
    ConfigFileResponse {
        file: snapshot.file,
        exists: snapshot.exists,
        revision: snapshot.revision,
        content_base64: base64::engine::general_purpose::STANDARD.encode(snapshot.content),
    }
}

fn component_status_name(status: &ComponentStatus) -> &'static str {
    match status {
        ComponentStatus::Installed { .. } => "installed",
        ComponentStatus::Modified => "modified",
        ComponentStatus::Incomplete => "incomplete",
        ComponentStatus::Unmanaged => "unmanaged",
        ComponentStatus::NotInstalled => "not-installed",
    }
}

async fn mutate_blocking<T, F>(state: ServiceState, operation: F) -> Response<Body>
where
    T: Serialize + Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    let guard = match state.mutation.clone().try_lock_owned() {
        Ok(guard) => guard,
        Err(_) => return busy("another management mutation is running"),
    };
    blocking(move || {
        let _guard = guard;
        operation()
    })
    .await
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
    let status = if message.contains("changed since")
        || message.contains("already running")
        || message.contains("active Request")
    {
        StatusCode::CONFLICT
    } else if message.contains("does not exist") || message.contains("not found") {
        StatusCode::NOT_FOUND
    } else if message.contains("exceeds 16777216 bytes") {
        StatusCode::PAYLOAD_TOO_LARGE
    } else {
        StatusCode::BAD_REQUEST
    };
    api_error(status, &message)
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
    let body = serde_json::to_vec(&json!({
        "error": {"code": status.as_u16(), "message": message}
    }))
    .unwrap_or_else(|_| b"{\"error\":{\"message\":\"Control API error\"}}".to_vec());
    content(status, "application/json; charset=utf-8", body)
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

fn directory_size(root: &FsPath) -> Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        for child in fs::read_dir(entry.path())? {
            let child = child?;
            let kind = child.file_type()?;
            if kind.is_file() && !kind.is_symlink() {
                total = total.saturating_add(child.metadata()?.len());
            }
        }
    }
    Ok(total)
}
