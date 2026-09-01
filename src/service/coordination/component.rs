//! Component inspection, update observation, and mutation coordination.

use super::OperationCoordinator;
use super::run_blocking;
use crate::component::{self, ComponentInspection, ComponentKind, ComponentSpec, LatestSnapshot};
use crate::docker;
use crate::service::operation::OperationSnapshot;
use crate::service::state::ServiceState;
use crate::tenant::TenantSelection;
use anyhow::Result;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct ComponentCoordinator {
    state: ServiceState,
}

pub(crate) enum ComponentInstallation {
    Completed(String),
    Started(OperationSnapshot),
}

impl ComponentCoordinator {
    pub(crate) fn new(state: ServiceState) -> Self {
        Self { state }
    }

    pub(crate) async fn list(
        &self,
        selection: TenantSelection,
    ) -> Result<Vec<ComponentInspection>> {
        let selected = selection.resolve(&self.state.root(), &self.state.host_home())?;
        run_blocking(move || component::inspect_catalog(&selected)).await
    }

    pub(crate) async fn latest(&self) -> Option<LatestSnapshot> {
        self.state.latest_component_snapshot().await
    }

    pub(crate) async fn check_latest(&self) -> Result<LatestSnapshot> {
        self.state.check_latest_components().await
    }

    pub(crate) async fn install(
        &self,
        selection: TenantSelection,
        kind: ComponentKind,
        version: Option<String>,
    ) -> Result<ComponentInstallation> {
        let selected = selection.resolve(&self.state.root(), &self.state.host_home())?;
        let spec = ComponentSpec::new(kind, version).map_err(anyhow::Error::msg)?;
        let guard = self.state.begin_management_mutation()?;
        if spec.kind().is_statusline() {
            return run_blocking(move || {
                let _guard = guard;
                component::install_component(&selected, &spec, None)?;
                Ok(ComponentInstallation::Completed(spec.to_string()))
            })
            .await;
        }

        let label = format!("install {spec}");
        OperationCoordinator::new(self.state.clone())
            .start(label, move |context| {
                let _guard = guard;
                context.log(format!("Installing {spec}"));
                let log_context = context.clone();
                let log: docker::LogCallback = Arc::new(move |line| log_context.log(line));
                component::install_component(&selected, &spec, Some(log))?;
                Ok(format!("Installed {spec}"))
            })
            .map(ComponentInstallation::Started)
    }

    pub(crate) async fn remove(
        &self,
        selection: TenantSelection,
        kind: ComponentKind,
    ) -> Result<&'static str> {
        let selected = selection.resolve(&self.state.root(), &self.state.host_home())?;
        let guard = self.state.begin_management_mutation()?;
        run_blocking(move || {
            let _guard = guard;
            component::remove_component(&selected, kind)?;
            Ok(kind.name())
        })
        .await
    }
}
