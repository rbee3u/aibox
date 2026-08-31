//! Overview and Topology Control API read projections.

use super::{ComponentRow, ControlResult, component_rows_from, json_response};
use crate::agent::AgentKind;
use crate::config;
use crate::service::coordination::{
    OverviewCoordinator, OverviewSnapshot, TopologyAgentSnapshot, TopologyTenantSnapshot,
};
use crate::service::state::ServiceState;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Serialize;

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct BootstrapResponse {
    pub(crate) version: &'static str,
    pub(crate) csrf_token: String,
    pub(crate) listen: String,
}

pub(super) async fn bootstrap(State(state): State<ServiceState>) -> Json<BootstrapResponse> {
    Json(BootstrapResponse {
        version: env!("CARGO_PKG_VERSION"),
        csrf_token: state.csrf_token().to_string(),
        listen: state.listen().to_string(),
    })
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct OverviewResponse {
    service: ServiceOverview,
    docker: DockerOverview,
    runtime_image: RuntimeImageOverview,
    managed_tenants: usize,
    host_available: bool,
    requests: RequestOverview,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ServiceOverview {
    version: &'static str,
    listen: String,
    uptime_seconds: u64,
    aibox_root: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DockerOverview {
    status: &'static str,
    error: Option<String>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct RuntimeImageOverview {
    reference: String,
    status: &'static str,
    id: Option<String>,
    created_at: Option<String>,
    size_bytes: Option<u64>,
    detail: Option<String>,
}

#[derive(Default, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct RequestOverview {
    total: usize,
    active: usize,
    warning: usize,
    error: usize,
    bytes: u64,
}

pub(super) async fn overview(State(state): State<ServiceState>) -> ControlResult {
    let snapshot = OverviewCoordinator::new(state).overview().await?;
    Ok(json_response(StatusCode::OK, &overview_response(snapshot)))
}

fn overview_response(snapshot: OverviewSnapshot) -> OverviewResponse {
    let (docker, runtime_image) = match snapshot.runtime_image {
        Ok(inspection) => (
            DockerOverview {
                status: "available",
                error: None,
            },
            RuntimeImageOverview {
                reference: snapshot.image_reference,
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
                error: Some(error),
            },
            RuntimeImageOverview {
                reference: snapshot.image_reference,
                status: "unknown",
                id: None,
                created_at: None,
                size_bytes: None,
                detail: None,
            },
        ),
    };
    OverviewResponse {
        service: ServiceOverview {
            version: env!("CARGO_PKG_VERSION"),
            listen: snapshot.listen,
            uptime_seconds: snapshot.uptime_seconds,
            aibox_root: snapshot.aibox_root,
        },
        docker,
        runtime_image,
        managed_tenants: snapshot.managed_tenants,
        host_available: snapshot.host_available,
        requests: RequestOverview {
            total: snapshot.requests.total,
            active: snapshot.requests.active,
            warning: snapshot.requests.warning,
            error: snapshot.requests.error,
            bytes: snapshot.requests.bytes,
        },
    }
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TopologyResponse {
    tenants: Vec<TopologyTenant>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TopologyTenant {
    kind: &'static str,
    name: Option<String>,
    display_name: String,
    home: String,
    exists: bool,
    agents: Vec<TopologyAgent>,
    components: TopologyComponents,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TopologyAgent {
    agent: AgentKind,
    current_config: TopologyCurrentConfig,
    named_configs: TopologyNamedConfigs,
    application: config::ApplicationStatus,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TopologyCurrentConfig {
    present_files: usize,
    expected_files: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TopologyNamedConfigs {
    entries: Vec<config::ConfigCatalogEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TopologyComponents {
    entries: Vec<ComponentRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub(super) async fn topology(State(state): State<ServiceState>) -> ControlResult {
    let tenants = OverviewCoordinator::new(state).topology().await?;
    Ok(json_response(
        StatusCode::OK,
        &TopologyResponse {
            tenants: tenants.into_iter().map(topology_tenant).collect(),
        },
    ))
}

fn topology_tenant(snapshot: TopologyTenantSnapshot) -> TopologyTenant {
    TopologyTenant {
        kind: if snapshot.managed { "managed" } else { "host" },
        name: snapshot.name,
        display_name: snapshot.display_name,
        home: snapshot.home,
        exists: snapshot.exists,
        agents: snapshot.agents.into_iter().map(topology_agent).collect(),
        components: match snapshot.components {
            Ok(inspections) => TopologyComponents {
                entries: component_rows_from(inspections),
                error: None,
            },
            Err(error) => TopologyComponents {
                entries: Vec::new(),
                error: Some(error),
            },
        },
    }
}

fn topology_agent(snapshot: TopologyAgentSnapshot) -> TopologyAgent {
    let agent = snapshot.agent;
    TopologyAgent {
        agent,
        current_config: match snapshot.current_config {
            Ok(inspection) => TopologyCurrentConfig {
                present_files: inspection.present_files,
                expected_files: inspection.expected_files,
                error: None,
            },
            Err(error) => TopologyCurrentConfig {
                present_files: 0,
                expected_files: agent.config_files().len(),
                error: Some(error),
            },
        },
        named_configs: match snapshot.named_configs {
            Ok(entries) => TopologyNamedConfigs {
                entries,
                error: None,
            },
            Err(error) => TopologyNamedConfigs {
                entries: Vec::new(),
                error: Some(error),
            },
        },
        application: snapshot.application,
    }
}
