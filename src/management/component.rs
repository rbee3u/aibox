//! Component inspection, update observation, and mutation coordination.

use super::{
    ManagementGate, ManagementPaths, OperationCoordinator, OperationSnapshot, run_blocking,
};
use crate::component::{
    self, ComponentInspection, ComponentKind, ComponentSpec, LatestProvider, LatestSnapshot,
    check_snapshot,
};
use crate::docker;
use crate::tenant::TenantSelection;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

#[derive(Clone)]
pub(crate) struct ComponentCoordinator {
    paths: ManagementPaths,
    gate: ManagementGate,
    operations: OperationCoordinator,
    updates: ComponentUpdates,
}

pub(crate) enum ComponentInstallation {
    Completed(String),
    Started(OperationSnapshot),
}

impl ComponentCoordinator {
    pub(super) fn new(
        paths: ManagementPaths,
        gate: ManagementGate,
        operations: OperationCoordinator,
        provider: Arc<dyn LatestProvider>,
    ) -> Self {
        Self {
            paths,
            gate,
            operations,
            updates: ComponentUpdates::new(provider),
        }
    }

    pub(crate) async fn list(
        &self,
        selection: TenantSelection,
    ) -> Result<Vec<ComponentInspection>> {
        let selected = selection.resolve(&self.paths.root, &self.paths.host_home)?;
        run_blocking(move || component::inspect_catalog(&selected)).await
    }

    pub(crate) async fn prefetch(&self) {
        self.updates.refresh(ComponentUpdateTrigger::Startup).await;
    }

    #[cfg(test)]
    pub(crate) fn set_latest_provider(&mut self, provider: Arc<dyn LatestProvider>) {
        self.updates.provider = provider;
    }

    pub(crate) async fn latest(&self) -> Option<LatestSnapshot> {
        self.updates.snapshot().await
    }

    pub(crate) async fn check_latest(&self) -> LatestSnapshot {
        self.updates.refresh(ComponentUpdateTrigger::Explicit).await
    }

    pub(crate) async fn install(
        &self,
        selection: TenantSelection,
        kind: ComponentKind,
        version: Option<String>,
    ) -> Result<ComponentInstallation> {
        let selected = selection.resolve(&self.paths.root, &self.paths.host_home)?;
        let spec = ComponentSpec::new(kind, version).map_err(anyhow::Error::msg)?;
        let mutation = self.gate.begin()?;
        if spec.kind().is_statusline() {
            return mutation
                .run_blocking(move || {
                    component::install_component(&selected, &spec, None)?;
                    Ok(ComponentInstallation::Completed(spec.to_string()))
                })
                .await;
        }

        let label = format!("install {spec}");
        self.operations
            .start(label, move |context| {
                mutation.run(|| {
                    context.log(format!("Installing {spec}"));
                    let log_context = context.clone();
                    let log: docker::LogCallback = Arc::new(move |line| log_context.log(line));
                    component::install_component(&selected, &spec, Some(log))?;
                    Ok(format!("Installed {spec}"))
                })
            })
            .map(ComponentInstallation::Started)
    }

    pub(crate) async fn remove(
        &self,
        selection: TenantSelection,
        kind: ComponentKind,
    ) -> Result<&'static str> {
        let selected = selection.resolve(&self.paths.root, &self.paths.host_home)?;
        let mutation = self.gate.begin()?;
        mutation
            .run_blocking(move || {
                component::remove_component(&selected, kind)?;
                Ok(kind.name())
            })
            .await
    }
}

#[derive(Clone)]
struct ComponentUpdates {
    cache: Arc<RwLock<ComponentUpdateCache>>,
    check: Arc<Mutex<()>>,
    provider: Arc<dyn LatestProvider>,
}

#[derive(Default)]
struct ComponentUpdateCache {
    generation: u64,
    completed: Option<LatestSnapshot>,
    published: Option<LatestSnapshot>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ComponentUpdateTrigger {
    Startup,
    Explicit,
}

impl ComponentUpdates {
    fn new(provider: Arc<dyn LatestProvider>) -> Self {
        Self {
            cache: Arc::new(RwLock::new(ComponentUpdateCache::default())),
            check: Arc::new(Mutex::new(())),
            provider,
        }
    }

    async fn snapshot(&self) -> Option<LatestSnapshot> {
        self.cache.read().await.published.clone()
    }

    async fn refresh(&self, trigger: ComponentUpdateTrigger) -> LatestSnapshot {
        let observed_generation = self.cache.read().await.generation;
        let _guard = self.check.lock().await;

        {
            let mut cache = self.cache.write().await;
            if cache.generation != observed_generation {
                let snapshot = cache
                    .completed
                    .clone()
                    .expect("a completed Component update generation has a snapshot");
                if trigger == ComponentUpdateTrigger::Explicit {
                    cache.published = Some(snapshot.clone());
                }
                return snapshot;
            }
        }

        let snapshot = check_snapshot(self.provider.clone()).await;
        let mut cache = self.cache.write().await;
        cache.generation = cache.generation.wrapping_add(1);
        cache.completed = Some(snapshot.clone());
        if trigger == ComponentUpdateTrigger::Explicit || snapshot.has_available_release() {
            cache.published = Some(snapshot.clone());
        }
        snapshot
    }
}

#[cfg(test)]
#[path = "component_tests.rs"]
mod tests;
