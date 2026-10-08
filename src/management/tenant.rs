//! Managed Tenant catalog and lifecycle coordination.

use super::{ManagementGate, ManagementPaths, run_blocking, tenant_scopes};
use crate::application_error::{ApplicationErrorKind, application_error};
use crate::tenant::{self, ManagedTenant};
use anyhow::Result;

#[derive(Clone)]
pub(crate) struct TenantCoordinator {
    paths: ManagementPaths,
    gate: ManagementGate,
}

pub(crate) enum TenantCatalogEntry {
    Host { home: String, exists: bool },
    Managed { name: String, home: String },
}

pub(crate) struct CreatedTenant {
    pub(crate) name: String,
    pub(crate) home: String,
}

pub(crate) struct DeleteTenantsCommand {
    pub(crate) names: Vec<String>,
    pub(crate) all: bool,
    pub(crate) confirmation: String,
}

pub(crate) struct DeletedTenants {
    pub(crate) names: Vec<String>,
    pub(crate) all: bool,
}

impl TenantCoordinator {
    pub(super) fn new(paths: ManagementPaths, gate: ManagementGate) -> Self {
        Self { paths, gate }
    }

    #[cfg(test)]
    pub(crate) fn hold_mutation_for_test(&self) -> Result<impl Send + use<>> {
        self.gate.begin()
    }

    pub(crate) async fn list(&self) -> Result<Vec<TenantCatalogEntry>> {
        let root = self.paths.root.clone();
        let host_home = self.paths.host_home.clone();
        run_blocking(move || {
            Ok(tenant_scopes(&root, &host_home)?
                .into_iter()
                .map(|scope| {
                    let home = scope.tenant.home_dir().display().to_string();
                    match scope.name {
                        Some(name) => TenantCatalogEntry::Managed { name, home },
                        None => TenantCatalogEntry::Host {
                            home,
                            exists: scope.exists,
                        },
                    }
                })
                .collect())
        })
        .await
    }

    pub(crate) async fn create(&self, name: String) -> Result<CreatedTenant> {
        let mutation = self.gate.begin()?;
        let root = self.paths.root.clone();
        mutation
            .run_blocking(move || {
                let tenant = ManagedTenant::resolve(&root, &name)?;
                tenant.ensure_initialized()?;
                Ok(CreatedTenant {
                    name: tenant.name().to_string(),
                    home: tenant.home_dir().display().to_string(),
                })
            })
            .await
    }

    pub(crate) async fn delete(&self, command: DeleteTenantsCommand) -> Result<DeletedTenants> {
        validate_delete_command(&command)?;
        let mutation = self.gate.begin()?;
        let root = self.paths.root.clone();
        mutation
            .run_blocking(move || {
                tenant::delete_tenants(&root, &command.names, command.all)?;
                Ok(DeletedTenants {
                    names: command.names,
                    all: command.all,
                })
            })
            .await
    }
}

fn validate_delete_command(command: &DeleteTenantsCommand) -> Result<()> {
    if command
        .names
        .iter()
        .any(|name| name == tenant::DEFAULT_TENANT_NAME)
    {
        return Err(application_error(
            ApplicationErrorKind::Conflict,
            "Default Managed Tenant is protected and cannot be deleted",
        ));
    }
    if command.all && command.confirmation != "delete all tenants" {
        return Err(application_error(
            ApplicationErrorKind::InvalidInput,
            "confirmation does not match",
        ));
    }
    if !command.all && command.names.len() == 1 && command.confirmation != command.names[0] {
        return Err(application_error(
            ApplicationErrorKind::InvalidInput,
            "confirmation does not match Tenant name",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "tenant_tests.rs"]
mod tests;
