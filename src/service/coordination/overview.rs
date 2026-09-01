//! Overview and Topology read projections.
//!
//! These are read-only observations, so no coordinator here takes the
//! management gate. They exist so the Control adapter maps domain snapshots to
//! wire types instead of reaching into `tenant`, `docker`, `config`, and
//! `foundation` itself.
//!
//! A per-Tenant or per-Agent failure is carried as evidence rather than
//! failing the whole projection: the Console draws a partially readable
//! topology, matching how `ComponentInspection` already reports its own error.

use super::{run_blocking, tenant_scopes};
use crate::agent::AgentKind;
use crate::component::{self, ComponentInspection};
use crate::config;
use crate::docker;
use crate::foundation::safe_fs;
use crate::service::state::ServiceState;
use crate::tenant::{self, Tenant};
use anyhow::Result;

#[derive(Clone)]
pub(crate) struct OverviewCoordinator {
    state: ServiceState,
}

/// What the Overview view observes, before wire projection.
pub(crate) struct OverviewSnapshot {
    pub(crate) listen: String,
    pub(crate) uptime_seconds: u64,
    pub(crate) aibox_root: String,
    /// The Runtime Image inspection, or why Docker could not be reached.
    pub(crate) runtime_image: Result<docker::RuntimeImageInspection, String>,
    pub(crate) image_reference: String,
    pub(crate) managed_tenants: usize,
    pub(crate) host_available: bool,
    pub(crate) requests: crate::request::RequestOverview,
}

/// One Tenant row of the Topology view, before wire projection.
///
/// A present `name` is what makes the row Managed; the Host Tenant has none. That
/// is the only Host/Managed carrier here, so the two cannot disagree.
pub(crate) struct TopologyTenantSnapshot {
    pub(crate) name: Option<String>,
    pub(crate) display_name: String,
    pub(crate) home: String,
    pub(crate) exists: bool,
    pub(crate) agents: Vec<TopologyAgentSnapshot>,
    pub(crate) components: Result<Vec<ComponentInspection>, String>,
}

/// One Coding Agent's Config state within a Topology Tenant row.
pub(crate) struct TopologyAgentSnapshot {
    pub(crate) agent: AgentKind,
    pub(crate) current_config: Result<config::CurrentConfigInspection, String>,
    pub(crate) named_configs: Result<Vec<config::ConfigCatalogEntry>, String>,
    pub(crate) application: config::ApplicationStatus,
}

impl OverviewCoordinator {
    pub(crate) fn new(state: ServiceState) -> Self {
        Self { state }
    }

    pub(crate) async fn overview(&self) -> Result<OverviewSnapshot> {
        let root = self.state.root();
        let host_home = self.state.host_home();
        let image = self.state.image();
        let inspection = self.state.request().inspection();
        let listen = self.state.listen();
        let uptime = self.state.uptime_seconds();
        run_blocking(move || {
            let tenants = tenant::list_tenants(&root)?;
            let host_available = safe_fs::real_dir_exists(&host_home, "Host Home")?;
            Ok(OverviewSnapshot {
                listen: listen.to_string(),
                uptime_seconds: uptime,
                aibox_root: root.display().to_string(),
                runtime_image: docker::inspect_runtime_image(image.as_str())
                    .map_err(|error| format!("{error:#}")),
                image_reference: image.to_string(),
                managed_tenants: tenants.len(),
                host_available,
                requests: inspection.overview()?,
            })
        })
        .await
    }

    pub(crate) async fn topology(&self) -> Result<Vec<TopologyTenantSnapshot>> {
        let root = self.state.root();
        let host_home = self.state.host_home();
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
    }
}
