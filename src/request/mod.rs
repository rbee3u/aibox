//! Shared Request state plus the socket-free proxy test router. The foreground
//! listener and Control adapters live in [`crate::service`].
//!
//! The proxy is global rather than Tenant-owned and never starts Docker; see
//! `docs/adr/0008-global-trusted-request-service.md`.

mod assessment;
mod inspection;
mod interpretation;
mod model;
mod proxy;
mod reporter;
mod response_observation;
mod sse;
mod store;

use crate::request::store::{RequestStore as Store, RequestWarningSink};
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

pub(crate) use inspection::RequestInspection;
pub(crate) use interpretation::BodyContentCoding;
#[allow(unused_imports)]
pub(crate) use model::{
    AssessmentFinding, AssessmentLevel, AssessmentPrimary, AssessmentSource, DiagnosticMetadata,
    ErrorKind, ErrorMetadata, Outcome, ProtocolDiagnostic, ProtocolFamily, ProtocolSummary,
    RecordedHeader, RequestAssessment, RequestMetadata, RequestOutcome, RequestState,
    RequestedEffective, RequestedObserved, ResponseMetadata, ResponseModeValue, ResponseSource,
    ResultMetadata, SummaryMetadata, SummaryRequestMetadata, SummaryResponseMetadata,
    TimingMetadata, TokenUsage, anchored_at,
};
pub(crate) use proxy::handle as handle_proxy;
pub(crate) use reporter::RequestReporter;
pub(crate) use store::{RequestDetailReadError, StoredRequestSummary};

pub(crate) fn format_version() -> u32 {
    store::FORMAT_VERSION
}

#[cfg(test)]
pub(crate) use store::{ObservedRequest, RequestStore, RuntimeMeasurements};

#[cfg(test)]
use axum::Router;
#[cfg(test)]
use axum::extract::State;
#[cfg(test)]
use std::net::SocketAddr;
#[cfg(test)]
use tokio::net::TcpListener;

#[derive(Clone)]
pub(crate) struct RequestProxyState {
    store: Store,
    shutdown: CancellationToken,
    response_tasks: TaskTracker,
    allow_private_upstream: bool,
    reporter: Option<RequestReporter>,
}

impl RequestProxyState {
    #[cfg(test)]
    pub(crate) fn new(root: &Path, shutdown: CancellationToken) -> Result<Self> {
        Self::new_with_reporter(root, shutdown, None)
    }

    pub(crate) fn new_with_reporter(
        root: &Path,
        shutdown: CancellationToken,
        reporter: Option<RequestReporter>,
    ) -> Result<Self> {
        let warning_sink = reporter.clone().map(|reporter| {
            Arc::new(move |category: &str, id: Option<&str>| reporter.warning(category, id))
                as RequestWarningSink
        });
        Ok(Self {
            store: Store::open_with_warning_sink(root, warning_sink)?,
            shutdown,
            response_tasks: TaskTracker::new(),
            allow_private_upstream: false,
            reporter,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(root: &Path) -> Result<Self> {
        let mut state = Self::new(root, CancellationToken::new())?;
        state.allow_private_upstream = true;
        Ok(state)
    }

    pub(crate) fn inspection(&self) -> RequestInspection {
        RequestInspection::new(self.store.clone())
    }

    pub(crate) fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub(crate) fn spawn_response_task<F>(&self, task: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.response_tasks.spawn(task);
    }

    pub(crate) fn begin_shutdown(&self) {
        self.shutdown.cancel();
        self.response_tasks.close();
    }

    pub(crate) async fn wait_for_response_tasks(&self) {
        self.response_tasks.wait().await;
    }
}

#[cfg(test)]
fn router(state: RequestProxyState) -> Router {
    Router::new().fallback(proxy_fallback).with_state(state)
}

#[cfg(test)]
async fn proxy_fallback(
    State(state): State<RequestProxyState>,
    request: axum::extract::Request,
) -> axum::response::Response {
    proxy::handle(state, request).await
}

#[cfg(test)]
#[path = "request_tests.rs"]
mod tests;
