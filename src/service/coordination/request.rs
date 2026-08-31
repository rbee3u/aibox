//! Request deletion coordination.
//!
//! Reads stay on the [`crate::request::RequestInspection`] facade the Control
//! adapter extracts directly, because a diagnostic read needs no mutation gate.
//! Deletion is a mutation, so it passes through the same management gate as
//! Config Application, Tenant lifecycle, Component installation, and Session
//! deletion.

use super::run_blocking;
use crate::service::state::ServiceState;
use anyhow::Result;

#[derive(Clone)]
pub(crate) struct RequestCoordinator {
    state: ServiceState,
}

impl RequestCoordinator {
    pub(crate) fn new(state: ServiceState) -> Self {
        Self { state }
    }

    /// Delete the explicitly selected Requests, returning how many were removed.
    ///
    /// The store rejects an Active Request, so an explicit selection containing
    /// one fails as a conflict rather than partially deleting.
    pub(crate) async fn delete(&self, ids: Vec<String>) -> Result<usize> {
        let guard = self.state.begin_management_mutation()?;
        let inspection = self.state.request().inspection();
        run_blocking(move || {
            let _guard = guard;
            inspection.delete_ids(&ids)
        })
        .await
    }
}
