//! Transport-independent management use cases and their shared process-local capabilities.

mod component;
mod config;
mod operation;
mod operation_state;
mod overview;
mod request;
mod session;
mod tenant;

pub(crate) use component::{ComponentCoordinator, ComponentInstallation};
pub(crate) use config::{ConfigCoordinator, ConfigFileView, DeleteConfigsCommand};
pub(crate) use operation::OperationCoordinator;
#[cfg(test)]
pub(crate) use operation_state::OperationLog;
use operation_state::OperationManager;
pub(crate) use operation_state::{OperationContext, OperationSnapshot, OperationState};
pub(crate) use overview::{
    OverviewCoordinator, OverviewSnapshot, TopologyAgentSnapshot, TopologyTenantSnapshot,
};
pub(crate) use request::RequestCoordinator;
pub(crate) use session::{DeleteSessionsCommand, SessionCoordinator};
pub(crate) use tenant::{DeleteTenantsCommand, TenantCatalogEntry, TenantCoordinator};

use crate::application_error::{ApplicationErrorKind, application_error};
use crate::foundation::safe_fs;
use crate::tenant::{ManagedTenant, Tenant};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// Immutable paths shared by Tenant-scoped use cases, without transport state.
#[derive(Clone)]
struct ManagementPaths {
    root: Arc<PathBuf>,
    host_home: Arc<PathBuf>,
}

#[derive(Clone)]
struct ManagementGate {
    lock: Arc<Mutex<()>>,
}

/// Ownership of one mutation; held until the blocking use case completes.
struct ManagementMutation {
    _guard: OwnedMutexGuard<()>,
}

impl ManagementMutation {
    /// Hold mutation ownership until synchronous work returns or unwinds.
    fn run<T>(self, operation: impl FnOnce() -> Result<T>) -> Result<T> {
        let _mutation = self;
        operation()
    }

    /// Transfer ownership to the worker, independent of its async caller.
    async fn run_blocking<T, F>(self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        run_blocking(move || self.run(operation)).await
    }
}

impl ManagementGate {
    fn begin(&self) -> Result<ManagementMutation> {
        self.lock
            .clone()
            .try_lock_owned()
            .map(|guard| ManagementMutation { _guard: guard })
            .map_err(|_| {
                application_error(
                    ApplicationErrorKind::Busy,
                    "another management mutation is running",
                )
            })
    }
}

/// Composed use cases. Clones share the gate, operations, plans, and snapshots.
#[derive(Clone)]
pub(crate) struct Management {
    pub(crate) tenants: TenantCoordinator,
    pub(crate) configs: ConfigCoordinator,
    pub(crate) sessions: SessionCoordinator,
    pub(crate) components: ComponentCoordinator,
    pub(crate) requests: RequestCoordinator,
    pub(crate) operations: OperationCoordinator,
    pub(crate) overview: OverviewCoordinator,
}
impl Management {
    pub(crate) fn new(
        root: Arc<PathBuf>,
        host_home: Arc<PathBuf>,
        image: Arc<String>,
        inspection: crate::request::RequestInspection,
        provider: Arc<dyn crate::component::LatestProvider>,
    ) -> Self {
        let paths = ManagementPaths { root, host_home };
        let gate = ManagementGate {
            lock: Arc::new(Mutex::new(())),
        };
        let operations = OperationCoordinator::new(image.clone(), OperationManager::new());
        Self {
            tenants: TenantCoordinator::new(paths.clone(), gate.clone()),
            configs: ConfigCoordinator::new(paths.clone(), gate.clone()),
            sessions: SessionCoordinator::new(paths.clone(), gate.clone()),
            components: ComponentCoordinator::new(
                paths.clone(),
                gate.clone(),
                operations.clone(),
                provider,
            ),
            requests: RequestCoordinator::new(inspection, gate),
            operations,
            overview: OverviewCoordinator::new(paths, image),
        }
    }
}

pub(super) async fn run_blocking<T, F>(operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    match tokio::task::spawn_blocking(operation).await {
        Ok(result) => result,
        Err(error) => Err(application_error(
            ApplicationErrorKind::Internal,
            format!("management worker failed: {error}"),
        )),
    }
}

/// One Tenant a Tenant-scoped view covers.
pub(super) struct TenantScope {
    pub(super) tenant: Tenant,
    /// The Managed Tenant name, or `None` for the Host Tenant.
    pub(super) name: Option<String>,
    /// Whether the Home exists. A Managed Tenant exists by definition, while the
    /// Host Home may be absent.
    pub(super) exists: bool,
}

/// Every Tenant a Console Tenant-scoped view covers, Host first.
///
/// The Host Home may be absent, while a Managed Tenant exists by definition.
/// Tenant catalog and Topology projections share this order and membership.
pub(super) fn tenant_scopes(root: &Path, host_home: &Path) -> Result<Vec<TenantScope>> {
    let mut scopes = vec![TenantScope {
        tenant: Tenant::Host {
            home_dir: host_home.to_path_buf(),
            root_dir: root.to_path_buf(),
        },
        name: None,
        exists: safe_fs::real_dir_exists(host_home, "Host Home")?,
    }];
    for name in crate::tenant::list_tenants(root)? {
        scopes.push(TenantScope {
            tenant: Tenant::Managed(ManagedTenant::resolve(root, &name)?),
            name: Some(name),
            exists: true,
        });
    }
    Ok(scopes)
}

#[cfg(test)]
#[path = "management_tests.rs"]
mod tests;
