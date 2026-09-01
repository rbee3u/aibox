//! Read-only and destructive inspection facade for recorded Requests.
//!
//! Control adapters use this facade instead of depending on persistence,
//! interpretation, or assessment implementation modules.

use super::assessment::{diagnostic_findings, effective_assessment};
use super::interpretation::{BodyContentCoding, body_content_coding};
use super::model::{AssessmentFinding, RequestAssessment, SummaryMetadata};
use super::store::{
    RequestDetailReadError, RequestListPage, RequestStore, StoredEventTimings, StoredRequest,
    timeline_end_at_ns,
};
use anyhow::Result;
use std::fs;

/// Read-only and destructive inspection handle for recorded Requests.
#[derive(Clone)]
pub(crate) struct RequestInspection {
    store: RequestStore,
}

impl RequestInspection {
    /// Inspect recorded Requests through `store`.
    pub(crate) fn new(store: RequestStore) -> Self {
        Self { store }
    }

    /// Count the collection from directory names and read only this page of summaries.
    pub(crate) fn list_page(&self, start: usize, limit: usize) -> Result<RequestListPage> {
        self.store.list_page(start, limit)
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
}
