//! Request deletion uses the shared mutation gate; diagnostic reads use
//! [`crate::request::RequestInspection`] without acquiring it.

use super::ManagementGate;
use crate::request::RequestInspection;
use anyhow::Result;

#[derive(Clone)]
pub(crate) struct RequestCoordinator {
    inspection: RequestInspection,
    gate: ManagementGate,
}

impl RequestCoordinator {
    pub(super) fn new(inspection: RequestInspection, gate: ManagementGate) -> Self {
        Self { inspection, gate }
    }

    /// Delete the explicitly selected Requests, returning how many were removed.
    ///
    /// The store rejects an Active Request, so an explicit selection containing
    /// one fails as a conflict rather than partially deleting.
    pub(crate) async fn delete(&self, ids: Vec<String>) -> Result<usize> {
        let mutation = self.gate.begin()?;
        let inspection = self.inspection.clone();
        mutation
            .run_blocking(move || inspection.delete_ids(&ids))
            .await
    }
}
