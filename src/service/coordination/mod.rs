//! Concrete Service coordinators for management use cases.

mod component;
mod config;
mod operation;
mod overview;
mod request;
mod session;
mod tenant;

pub(super) use component::{ComponentCoordinator, ComponentInstallation};
pub(super) use config::{ConfigCoordinator, ConfigFileView, DeleteConfigsCommand};
pub(super) use operation::OperationCoordinator;
pub(super) use overview::{
    OverviewCoordinator, OverviewSnapshot, TopologyAgentSnapshot, TopologyTenantSnapshot,
};
pub(super) use request::RequestCoordinator;
pub(super) use session::{DeleteSessionsCommand, SessionCoordinator};
pub(super) use tenant::{DeleteTenantsCommand, TenantCatalogEntry, TenantCoordinator};

use crate::application_error::{ApplicationErrorKind, application_error};
use crate::foundation::safe_fs;
use crate::tenant::{ManagedTenant, Tenant};
use anyhow::Result;
use std::path::Path;

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
