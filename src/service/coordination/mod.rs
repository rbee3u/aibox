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
use anyhow::Result;

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
