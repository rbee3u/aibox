//! Shared state carried by the foreground Service and its Control API.

use crate::application_error::{ApplicationErrorKind, application_error};
use crate::component::updates as component_updates;
use crate::config;
use crate::docker;
use crate::request::RequestProxyState;
use crate::service::operation::{OperationContext, OperationManager, OperationSnapshot};
use anyhow::Result;
use axum::extract::FromRef;
use base64::Engine as _;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock, broadcast};
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct ServiceState {
    root: Arc<PathBuf>,
    host_home: Arc<PathBuf>,
    image: Arc<String>,
    listen: SocketAddr,
    started: Instant,
    csrf: Arc<String>,
    request: RequestProxyState,
    operations: OperationManager,
    mutation: Arc<Mutex<()>>,
    auth_propagation: Arc<std::sync::Mutex<Option<PendingAuthPropagation>>>,
    latest_snapshot: Arc<RwLock<Option<component_updates::LatestSnapshot>>>,
    latest_check: Arc<Mutex<()>>,
    latest_provider: Arc<dyn component_updates::LatestProvider>,
}

pub(crate) struct PendingAuthPropagation {
    id: String,
    plan: config::AuthPropagationPlan,
}

/// Exclusive ownership of one Service management mutation.
pub(crate) struct ManagementMutation {
    _guard: OwnedMutexGuard<()>,
}

#[derive(Clone)]
pub(crate) struct ConsoleCspNonce(String);

impl ConsoleCspNonce {
    pub(crate) fn new() -> Self {
        Self(base64::engine::general_purpose::STANDARD_NO_PAD.encode(Uuid::new_v4().as_bytes()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromRef<ServiceState> for RequestProxyState {
    fn from_ref(state: &ServiceState) -> Self {
        state.request()
    }
}

impl ServiceState {
    pub(crate) fn new(
        root: PathBuf,
        host_home: PathBuf,
        image: String,
        listen: SocketAddr,
        csrf: String,
        request: RequestProxyState,
        latest_provider: Arc<dyn component_updates::LatestProvider>,
    ) -> Self {
        Self {
            root: Arc::new(root),
            host_home: Arc::new(host_home),
            image: Arc::new(image),
            listen,
            started: Instant::now(),
            csrf: Arc::new(csrf),
            request,
            operations: OperationManager::new(),
            mutation: Arc::new(Mutex::new(())),
            auth_propagation: Arc::new(std::sync::Mutex::new(None)),
            latest_snapshot: Arc::new(RwLock::new(None)),
            latest_check: Arc::new(Mutex::new(())),
            latest_provider,
        }
    }

    pub(crate) fn root(&self) -> Arc<PathBuf> {
        self.root.clone()
    }

    pub(crate) fn host_home(&self) -> Arc<PathBuf> {
        self.host_home.clone()
    }

    pub(crate) fn image(&self) -> Arc<String> {
        self.image.clone()
    }

    pub(crate) fn listen(&self) -> SocketAddr {
        self.listen
    }

    pub(crate) fn csrf_token(&self) -> &str {
        &self.csrf
    }

    pub(crate) fn uptime_seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    pub(crate) fn request(&self) -> RequestProxyState {
        self.request.clone()
    }

    pub(crate) fn begin_management_mutation(&self) -> Result<ManagementMutation> {
        self.mutation
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

    pub(crate) fn auth_propagation_plan(&self, id: String, plan: config::AuthPropagationPlan) {
        *self
            .auth_propagation
            .lock()
            .expect("Credential Propagation plan store poisoned") =
            Some(PendingAuthPropagation { id, plan });
    }

    pub(crate) fn take_auth_propagation_plan(
        &self,
        id: &str,
    ) -> anyhow::Result<config::AuthPropagationPlan> {
        let mut pending = self
            .auth_propagation
            .lock()
            .expect("Credential Propagation plan store poisoned");
        if !pending.as_ref().is_some_and(|plan| plan.id == id) {
            anyhow::bail!("Credential Propagation plan is missing or obsolete");
        }
        Ok(pending.take().expect("plan checked above").plan)
    }

    pub(crate) async fn latest_component_snapshot(
        &self,
    ) -> Option<component_updates::LatestSnapshot> {
        self.latest_snapshot.read().await.clone()
    }

    pub(crate) async fn check_latest_components(
        &self,
    ) -> Result<component_updates::LatestSnapshot> {
        let _guard = self.latest_check.clone().try_lock_owned().map_err(|_| {
            application_error(
                ApplicationErrorKind::Busy,
                "another Component update check is running",
            )
        })?;
        let snapshot = component_updates::check_snapshot(self.latest_provider.clone()).await;
        *self.latest_snapshot.write().await = Some(snapshot.clone());
        Ok(snapshot)
    }

    #[cfg(test)]
    pub(crate) fn set_latest_provider(
        &mut self,
        provider: Arc<dyn component_updates::LatestProvider>,
    ) {
        self.latest_provider = provider;
    }

    pub(crate) fn operation_snapshot(&self) -> Option<OperationSnapshot> {
        self.operations.snapshot()
    }

    pub(crate) fn subscribe_operations(&self) -> broadcast::Receiver<()> {
        self.operations.subscribe()
    }

    pub(crate) fn start_management_operation<F>(
        &self,
        kind: impl Into<String>,
        operation: F,
    ) -> Result<OperationSnapshot>
    where
        F: FnOnce(OperationContext) -> Result<String> + Send + 'static,
    {
        self.operations.start(kind, operation)
    }

    pub(crate) fn management_operation_is_running(&self) -> bool {
        self.operations.is_running()
    }

    /// Cancel the management operation and the container operation it owns.
    /// OperationManager itself stays independent of Docker lifecycle policy.
    pub(crate) fn cancel_operation(&self, id: &str) -> anyhow::Result<()> {
        self.operations.cancel(id)?;
        docker::cancel_active_container_operation();
        Ok(())
    }

    pub(crate) fn cancel_current_operation(&self) {
        let Some(snapshot) = self.operation_snapshot() else {
            return;
        };
        if snapshot.state == crate::service::operation::OperationState::Running {
            let _ = self.cancel_operation(&snapshot.id);
        }
    }
}
