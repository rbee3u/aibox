//! Read-only and destructive inspection facade for recorded Requests.
//!
//! Control adapters use this facade instead of depending on persistence,
//! interpretation, or assessment implementation modules.

use super::assessment::{diagnostic_findings, effective_assessment};
use super::interpretation::{BodyContentCoding, body_content_coding};
use super::model::{AssessmentFinding, AssessmentLevel, RequestAssessment, SummaryMetadata};
use super::store::{
    RequestDetailReadError, RequestStore, StoredEventTimings, StoredRequest, StoredRequestSummary,
    timeline_end_at_ns,
};
use anyhow::{Context as _, Result};
use std::fs;

#[derive(Clone)]
pub(crate) struct RequestInspection {
    store: RequestStore,
}

pub(crate) struct RequestOverview {
    pub(crate) total: usize,
    pub(crate) active: usize,
    pub(crate) warning: usize,
    pub(crate) error: usize,
    pub(crate) bytes: u64,
}

impl RequestInspection {
    pub(crate) fn new(store: RequestStore) -> Self {
        Self { store }
    }

    pub(crate) fn scan_summaries(&self) -> Result<Vec<StoredRequestSummary>> {
        self.store.scan_summaries()
    }

    pub(crate) fn find(&self, id: &str) -> Result<StoredRequest> {
        self.store.find(id)
    }

    pub(crate) fn find_detail(&self, id: &str) -> Result<StoredRequest, RequestDetailReadError> {
        self.store.find_with_event_index_warnings(id)
    }

    pub(crate) fn open_body(
        &self,
        id: &str,
        response: bool,
        offset: u64,
    ) -> Result<(fs::File, u64)> {
        self.store.open_body(id, response, offset)
    }

    pub(crate) fn open_request_body(
        &self,
        request: &StoredRequest,
        response: bool,
        offset: u64,
    ) -> Result<(fs::File, u64)> {
        self.store.open_request_body(request, response, offset)
    }

    pub(crate) fn read_event_timings(
        &self,
        id: &str,
        after_sequence: u64,
    ) -> Result<StoredEventTimings> {
        self.store.read_event_timings(id, after_sequence)
    }

    pub(crate) fn delete_ids(&self, ids: &[String]) -> Result<usize> {
        self.store.delete_ids(ids)
    }

    pub(crate) fn assessment(&self, summary: &SummaryMetadata, active: bool) -> RequestAssessment {
        effective_assessment(summary, active)
    }

    pub(crate) fn diagnostics(
        &self,
        summary: &SummaryMetadata,
        interrupted: bool,
    ) -> Vec<AssessmentFinding> {
        diagnostic_findings(summary, interrupted)
    }

    pub(crate) fn body_content_coding(
        &self,
        headers: &[super::model::RecordedHeader],
    ) -> Result<BodyContentCoding> {
        body_content_coding(headers)
    }

    pub(crate) fn timeline_end_at_ns(
        &self,
        request: &StoredRequest,
        live: Option<String>,
    ) -> Option<String> {
        timeline_end_at_ns(request, live)
    }

    pub(crate) fn overview(&self) -> Result<RequestOverview> {
        let captured_requests = self.store.scan_summaries()?;
        let mut overview = RequestOverview {
            total: captured_requests.len(),
            active: 0,
            warning: 0,
            error: 0,
            bytes: directory_size(self.store.root())?,
        };
        for request in captured_requests {
            match effective_assessment(&request.summary, request.active).level {
                AssessmentLevel::Active => overview.active += 1,
                AssessmentLevel::Warning => overview.warning += 1,
                AssessmentLevel::Error => overview.error += 1,
                AssessmentLevel::Ok => {}
            }
        }
        Ok(overview)
    }

    #[cfg(test)]
    pub(crate) fn store(&self) -> RequestStore {
        self.store.clone()
    }
}

fn directory_size(root: &std::path::Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        for child in fs::read_dir(entry.path())? {
            let child = child?;
            let kind = child.file_type()?;
            if kind.is_file() && !kind.is_symlink() {
                total = total.saturating_add(child.metadata()?.len());
            }
        }
    }
    Ok(total)
}
