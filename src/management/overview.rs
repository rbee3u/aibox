//! Ungated Overview and Topology observations for Control wire projection.
//! Per-Tenant and per-Agent failures remain local to the affected snapshot.

use super::{ManagementPaths, run_blocking, tenant_scopes};
use crate::agent::AgentKind;
use crate::component::{self, ComponentInspection};
use crate::config;
use crate::docker;
use crate::foundation::safe_fs;
use crate::session;
use crate::tenant::{self, Tenant};
use anyhow::Result;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct OverviewCoordinator {
    paths: ManagementPaths,
    image: Arc<String>,
}

/// What the Overview view observes, before wire projection.
pub(crate) struct OverviewSnapshot {
    pub(crate) aibox_root: String,
    /// The Runtime Image inspection, or why Docker could not be reached.
    pub(crate) runtime_image: Result<docker::RuntimeImageInspection, String>,
    pub(crate) image_reference: String,
    pub(crate) managed_tenants: usize,
    pub(crate) host_available: bool,
    pub(crate) host_home: String,
}

/// Topology Tenant state; `name` is `None` only for the Host Tenant.
pub(crate) struct TopologyTenantSnapshot {
    pub(crate) name: Option<String>,
    pub(crate) display_name: String,
    pub(crate) home: String,
    pub(crate) exists: bool,
    pub(crate) agents: Vec<TopologyAgentSnapshot>,
    pub(crate) components: Result<Vec<ComponentInspection>, String>,
}

/// One Agent's Config and Session state within a Topology Tenant row.
pub(crate) struct TopologyAgentSnapshot {
    pub(crate) agent: AgentKind,
    pub(crate) current_config: Result<config::CurrentConfigInspection, String>,
    pub(crate) named_configs: Result<Vec<config::ConfigCatalogEntry>, String>,
    pub(crate) application: config::ApplicationStatus,
    /// Transcript count without parsing, or the discovery error.
    pub(crate) sessions: Result<usize, String>,
}

impl OverviewCoordinator {
    pub(super) fn new(paths: ManagementPaths, image: Arc<String>) -> Self {
        Self { paths, image }
    }

    pub(crate) async fn overview(&self) -> Result<OverviewSnapshot> {
        let root = self.paths.root.clone();
        let host_home = self.paths.host_home.clone();
        let image = self.image.clone();
        run_blocking(move || {
            let tenants = tenant::list_tenants(&root)?;
            let host_available = safe_fs::real_dir_exists(&host_home, "Host Home")?;
            Ok(OverviewSnapshot {
                aibox_root: root.display().to_string(),
                runtime_image: docker::inspect_runtime_image(image.as_str())
                    .map_err(|error| format!("{error:#}")),
                image_reference: image.to_string(),
                managed_tenants: tenants.len(),
                host_available,
                host_home: host_home.display().to_string(),
            })
        })
        .await
    }

    pub(crate) async fn topology(&self) -> Result<Vec<TopologyTenantSnapshot>> {
        let root = self.paths.root.clone();
        let host_home = self.paths.host_home.clone();
        run_blocking(move || {
            Ok(tenant_scopes(&root, &host_home)?
                .into_iter()
                .map(|scope| TopologyTenantSnapshot {
                    home: scope.tenant.home_dir().display().to_string(),
                    agents: [AgentKind::Codex, AgentKind::Claude]
                        .into_iter()
                        .map(|agent| agent_snapshot(&scope.tenant, agent))
                        .collect(),
                    components: component::inspect_catalog(&scope.tenant)
                        .map_err(|error| format!("{error:#}")),
                    display_name: scope
                        .name
                        .clone()
                        .unwrap_or_else(|| "Host Tenant".to_string()),
                    name: scope.name,
                    exists: scope.exists,
                })
                .collect())
        })
        .await
    }
}

fn agent_snapshot(tenant: &Tenant, agent: AgentKind) -> TopologyAgentSnapshot {
    let selected = tenant.for_agent(agent);
    TopologyAgentSnapshot {
        agent,
        current_config: config::inspect_current_config(&selected)
            .map_err(|error| format!("{error:#}")),
        named_configs: config::inspect_named_configs(&selected)
            .map_err(|error| format!("{error:#}")),
        application: config::application_status(&selected),
        sessions: session_count(tenant, agent),
    }
}

fn session_count(tenant: &Tenant, agent: AgentKind) -> Result<usize, String> {
    session::session_discovery_summary(agent, tenant.home_dir())
        .map(|summary| summary.count)
        .map_err(|error| format!("{error:#}"))
}
