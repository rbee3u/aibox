//! Service process information, HTTP security state, and composed capabilities.

use crate::management::Management;
use crate::request::RequestProxyState;
use axum::extract::FromRef;
use base64::Engine as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct ServiceState {
    listen: SocketAddr,
    started: Instant,
    csrf: Arc<String>,
    request: RequestProxyState,
    shutdown: CancellationToken,
    pub(crate) management: Management,
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
        listen: SocketAddr,
        csrf: String,
        shutdown: CancellationToken,
        request: RequestProxyState,
        management: Management,
    ) -> Self {
        Self {
            listen,
            started: Instant::now(),
            csrf: Arc::new(csrf),
            request,
            shutdown,
            management,
        }
    }
    pub(crate) fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
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
}
