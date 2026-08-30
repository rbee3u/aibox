//! Overview and Topology Control API read projections.

use super::{ComponentRow, blocking, component_rows};
use crate::agent::AgentKind;
use crate::service::state::ServiceState;
use crate::tenant::{self, ManagedTenant, Tenant};
use crate::{config, docker};
use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::Response;
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

pub(super) async fn overview(State(state): State<ServiceState>) -> Response<Body> {
    let root = state.root();
    let host_home = state.host_home();
    let image = state.image();
    let request = state.request().inspection();
    let listen = state.listen();
    let uptime = state.uptime_seconds();
    blocking(move || {
        let tenants = tenant::list_tenants(&root)?;
        let host_available = crate::foundation::safe_fs::real_dir_exists(&host_home, "Host Home")?;
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
        let captured_requests = request.overview()?;
        let requests = RequestOverview {
            total: captured_requests.total,
            active: captured_requests.active,
            warning: captured_requests.warning,
            error: captured_requests.error,
            bytes: captured_requests.bytes,
        };
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

pub(super) async fn topology(State(state): State<ServiceState>) -> Response<Body> {
    let root = state.root();
    let host_home = state.host_home();
    blocking(move || {
        let host_exists = crate::foundation::safe_fs::real_dir_exists(&host_home, "Host Home")?;
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
