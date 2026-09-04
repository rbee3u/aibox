//! Overview and Topology Control API read projections.

use super::{
    ComponentRow, ComponentStatusWire, ControlResult, TenantRow, component_rows_from, json_response,
};
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
    host_home: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ServiceOverview {
    version: &'static str,
    listen: String,
    uptime_seconds: u64,
    aibox_root: String,
}

/// Whether the Docker client answered.
///
/// A closed enum rather than a `&'static str` so the generated Console binding
/// is a union of the states this actually emits. The Console decides what to
/// render from it, and a bare `string` there would push that check to runtime.
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub(crate) enum DockerStatus {
    Available,
    Unavailable,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DockerOverview {
    status: DockerStatus,
    error: Option<String>,
}

/// Whether the Runtime Image is present, absent, or unobservable.
///
/// `Unknown` is what Docker being unreachable looks like from here, which is why
/// this is not an `Option`: the Console renders all three differently.
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub(crate) enum RuntimeImageStatus {
    Built,
    Missing,
    Unknown,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct RuntimeImageOverview {
    reference: String,
    status: RuntimeImageStatus,
    id: Option<String>,
    created_at: Option<String>,
    size_bytes: Option<u64>,
    detail: Option<String>,
}

pub(super) async fn overview(State(state): State<ServiceState>) -> ControlResult {
    let snapshot = OverviewCoordinator::new(state).overview().await?;
    Ok(json_response(StatusCode::OK, &overview_response(snapshot)))
}

fn overview_response(snapshot: OverviewSnapshot) -> OverviewResponse {
    let (docker, runtime_image) = match snapshot.runtime_image {
        Ok(inspection) => (
            DockerOverview {
                status: DockerStatus::Available,
                error: None,
            },
            RuntimeImageOverview {
                reference: snapshot.image_reference,
                status: if inspection.present {
                    RuntimeImageStatus::Built
                } else {
                    RuntimeImageStatus::Missing
                },
                id: inspection.id,
                created_at: inspection.created_at,
                size_bytes: inspection.size_bytes,
                detail: inspection.detail,
            },
        ),
        Err(error) => (
            DockerOverview {
                status: DockerStatus::Unavailable,
                error: Some(error),
            },
            RuntimeImageOverview {
                reference: snapshot.image_reference,
                status: RuntimeImageStatus::Unknown,
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
        host_home: snapshot.host_home,
    }
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TopologyResponse {
    tenants: Vec<TopologyTenant>,
}

/// One Tenant of the Topology view: the Tenant catalog row plus its state.
///
/// The row is the same `TenantRow` the Tenants module lists, flattened in rather
/// than restated, so a Tenant identity is one shape everywhere on the wire and
/// the Console reads the Host/Managed distinction off the same discriminant.
#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TopologyTenant {
    #[serde(flatten)]
    row: TenantRow,
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
    #[cfg_attr(test, ts(optional))]
    error: Option<String>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TopologyNamedConfigs {
    count: usize,
    attention: Vec<config::ConfigCatalogEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    error: Option<String>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TopologyComponents {
    total: usize,
    installed: usize,
    attention: Vec<ComponentRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
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
    let TopologyTenantSnapshot {
        name,
        display_name,
        home,
        exists,
        agents,
        components,
    } = snapshot;
    // A Managed row carries its name and the Host row has none, which is the one
    // distinction between the two variants here.
    let row = match name {
        Some(name) => TenantRow::Managed {
            name,
            display_name,
            home,
            exists,
        },
        None => TenantRow::Host {
            name: None,
            display_name,
            home,
            exists,
        },
    };
    TopologyTenant {
        row,
        agents: agents.into_iter().map(topology_agent).collect(),
        components: match components {
            Ok(inspections) => {
                let rows = component_rows_from(inspections);
                let total = rows.len();
                let installed = rows
                    .iter()
                    .filter(|row| component_counts_as_installed(row.status))
                    .count();
                let attention = rows
                    .into_iter()
                    .filter(|row| {
                        row.error.is_some()
                            || matches!(
                                row.status,
                                Some(
                                    ComponentStatusWire::Modified
                                        | ComponentStatusWire::Incomplete
                                        | ComponentStatusWire::Unmanaged
                                )
                            )
                    })
                    .collect();
                TopologyComponents {
                    total,
                    installed,
                    attention,
                    error: None,
                }
            }
            Err(error) => TopologyComponents {
                total: 0,
                installed: 0,
                attention: Vec::new(),
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
                count: entries.len(),
                attention: entries
                    .into_iter()
                    .filter(config::ConfigCatalogEntry::needs_attention)
                    .collect(),
                error: None,
            },
            Err(error) => TopologyNamedConfigs {
                count: 0,
                attention: Vec::new(),
                error: Some(error),
            },
        },
        application: snapshot.application,
    }
}

/// Present Components, matching the Tenants catalog count.
///
/// `modified` is installed-but-dirty: the Component is there, and attention
/// already carries the dirty signal. Counting only exact `installed` made
/// Overview report fewer installed Components than Tenants for the same Tenant.
fn component_counts_as_installed(status: Option<ComponentStatusWire>) -> bool {
    matches!(
        status,
        Some(ComponentStatusWire::Installed | ComponentStatusWire::Modified)
    )
}

#[cfg(test)]
#[path = "overview_tests.rs"]
mod tests;
