//! Shared state carried by the foreground Service and its Control API.

use crate::application_error::{ApplicationErrorKind, application_error};
use crate::component::{LatestProvider, LatestSnapshot, check_snapshot};
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
    management: ManagementState,
    component_updates: ComponentUpdateState,
}

/// Concrete management state owned by the Service composition root.
#[derive(Clone)]
struct ManagementState {
    operations: OperationManager,
    gate: ManagementGate,
    credential_propagation: CredentialPropagationState,
}

#[derive(Clone)]
struct ManagementGate {
    lock: Arc<Mutex<()>>,
}

#[derive(Clone)]
struct CredentialPropagationState {
    pending: Arc<std::sync::Mutex<Option<PendingAuthPropagation>>>,
}

#[derive(Clone)]
struct ComponentUpdateState {
    snapshot: Arc<RwLock<Option<LatestSnapshot>>>,
    check: Arc<Mutex<()>>,
    provider: Arc<dyn LatestProvider>,
}

pub(crate) struct PendingAuthPropagation {
    id: String,
    plan: config::AuthPropagationPlan,
}

/// Exclusive ownership of one mutation participating in the shared management
/// gate.
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
        latest_provider: Arc<dyn LatestProvider>,
    ) -> Self {
        Self {
            root: Arc::new(root),
            host_home: Arc::new(host_home),
            image: Arc::new(image),
            listen,
            started: Instant::now(),
            csrf: Arc::new(csrf),
            request,
            management: ManagementState {
                operations: OperationManager::new(),
                gate: ManagementGate {
                    lock: Arc::new(Mutex::new(())),
                },
                credential_propagation: CredentialPropagationState {
                    pending: Arc::new(std::sync::Mutex::new(None)),
                },
            },
            component_updates: ComponentUpdateState {
                snapshot: Arc::new(RwLock::new(None)),
                check: Arc::new(Mutex::new(())),
                provider: latest_provider,
            },
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
        self.management
            .gate
            .lock
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
            .management
            .credential_propagation
            .pending
            .lock()
            .expect("Credential Propagation plan store poisoned") =
            Some(PendingAuthPropagation { id, plan });
    }

    pub(crate) fn take_auth_propagation_plan(
        &self,
        id: &str,
    ) -> anyhow::Result<config::AuthPropagationPlan> {
        let mut pending = self
            .management
            .credential_propagation
            .pending
            .lock()
            .expect("Credential Propagation plan store poisoned");
        if !pending.as_ref().is_some_and(|plan| plan.id == id) {
            anyhow::bail!("Credential Propagation plan is missing or obsolete");
        }
        Ok(pending.take().expect("plan checked above").plan)
    }

    pub(crate) async fn latest_component_snapshot(&self) -> Option<LatestSnapshot> {
        self.component_updates.snapshot.read().await.clone()
    }

    pub(crate) async fn check_latest_components(&self) -> Result<LatestSnapshot> {
        let _guard = self
            .component_updates
            .check
            .clone()
            .try_lock_owned()
            .map_err(|_| {
                application_error(
                    ApplicationErrorKind::Busy,
                    "another Component update check is running",
                )
            })?;
        let snapshot = check_snapshot(self.component_updates.provider.clone()).await;
        *self.component_updates.snapshot.write().await = Some(snapshot.clone());
        Ok(snapshot)
    }

    #[cfg(test)]
    pub(crate) fn set_latest_provider(&mut self, provider: Arc<dyn LatestProvider>) {
        self.component_updates.provider = provider;
    }

    pub(crate) fn operation_snapshot(&self) -> Option<OperationSnapshot> {
        self.management.operations.snapshot()
    }

    pub(crate) fn subscribe_operations(&self) -> broadcast::Receiver<()> {
        self.management.operations.subscribe()
    }

    pub(crate) fn start_management_operation<F>(
        &self,
        kind: impl Into<String>,
        operation: F,
    ) -> Result<OperationSnapshot>
    where
        F: FnOnce(OperationContext) -> Result<String> + Send + 'static,
    {
        self.management.operations.start(kind, operation)
    }

    pub(crate) fn management_operation_is_running(&self) -> bool {
        self.management.operations.is_running()
    }

    /// Cancel the management operation and the container operation it owns.
    /// OperationManager itself stays independent of Docker lifecycle policy.
    pub(crate) fn cancel_operation(&self, id: &str) -> anyhow::Result<()> {
        self.management.operations.cancel(id)?;
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
