//! Shared Request state. The foreground listener and Control adapters live in
//! [`crate::service`].
//!
//! The proxy is global rather than Tenant-owned and never starts Docker; see
//! `docs/adr/0006-global-request-proxy-and-shared-listener.md`.

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
use proxy::RetryRules;
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

pub(crate) use inspection::RequestInspection;
pub(crate) use interpretation::{BodyContentCoding, body_reader};
pub(crate) use model::{
    AssessmentFinding, AssessmentLevel, AssessmentSource, ProtocolSummary, RecordedHeader,
    RequestAssessment, RequestMetadata, RequestState, ResponseMetadata, ResponseSource,
    ResultMetadata, SummaryMetadata, anchored_at,
};
/// Types explicitly constructed by Request and Control test fixtures.
#[cfg(test)]
pub(crate) use model::{ProtocolDiagnostic, RequestOutcome};
pub(crate) use proxy::handle as handle_proxy;
pub(crate) use reporter::RequestReporter;
pub(crate) use store::{
    REQUEST_GROUP_COMPACT_INTERVAL, RequestDetailReadError, StoredRequestSummary,
};

pub(crate) fn format_version() -> u32 {
    store::FORMAT_VERSION
}

#[cfg(test)]
pub(crate) use store::{ObservedRequest, RequestStore, RuntimeMeasurements};

#[derive(Clone)]
pub(crate) struct RequestProxyState {
    store: Store,
    shutdown: CancellationToken,
    response_tasks: TaskTracker,
    reporter: Option<RequestReporter>,
    retry_rules: RetryRules,
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
        let retry_rules = RetryRules::load(root)?;
        let warning_sink = reporter.clone().map(|reporter| {
            Arc::new(move |category: &str, id: Option<&str>| reporter.warning(category, id))
                as RequestWarningSink
        });
        Ok(Self {
            store: Store::open_with_warning_sink(root, warning_sink)?,
            shutdown,
            response_tasks: TaskTracker::new(),
            reporter,
            retry_rules,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(root: &Path) -> Result<Self> {
        Self::new(root, CancellationToken::new())
    }

    pub(crate) fn inspection(&self) -> RequestInspection {
        RequestInspection::new(self.store.clone())
    }

    /// Compact at most one Request Group of the oldest ungrouped Requests.
    pub(crate) fn compact_once(&self) -> Result<()> {
        self.store.compact_once()
    }

    /// Fixture writes must share this state's active-Request map; reopening
    /// the store would lose visibility of in-flight Requests.
    #[cfg(test)]
    pub(crate) fn store(&self) -> Store {
        self.store.clone()
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
#[path = "request_tests.rs"]
mod tests;
