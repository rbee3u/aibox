//! Requests module JSON/body API.
//!
//! List handlers read only the materialized Request Summary while detail
//! reads stay strict over raw metadata; see
//! `docs/adr/0007-request-evidence-and-materialized-projections.md`.
//! Bodies stream from disk as recorded; the decoded variants only undo a
//! recorded content coding. Nothing here redacts, truncates, or expires a
//! Request.

mod body;
mod projection;
mod timing;

#[cfg(test)]
pub(crate) use body::BodyQuery;
#[cfg(test)]
use body::{body_response, decoded_body_response};
pub(super) use body::{decoded_request_body, decoded_response_body, request_body, response_body};
#[cfg(test)]
use projection::list_requests_inner;
#[cfg(test)]
pub(crate) use projection::{ListQuery, RequestDetail, RequestList, ResponseDetail};
pub(super) use projection::{list_requests, request_detail};
pub(super) use timing::response_event_timings;
#[cfg(test)]
pub(crate) use timing::{EventTimingQuery, EventTimingResponse};

use super::{ControlResult, json_response};
#[cfg(test)]
use crate::request::{
    AssessmentLevel, AssessmentSource, RecordedHeader, RequestInspection, ResponseMetadata,
    ResponseSource,
};
use crate::service::state::ServiceState;
use axum::Json;
use axum::extract::State;
#[cfg(test)]
use axum::extract::{Path, Query};
use axum::http::StatusCode;
#[cfg(test)]
use axum::http::header;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DeleteRequest {
    ids: Vec<String>,
}

/// Delete through Management's mutation gate; diagnostic reads remain ungated.
pub(super) async fn delete_requests(
    State(state): State<ServiceState>,
    Json(request): Json<DeleteRequest>,
) -> ControlResult {
    let deleted = state.management.requests.delete(request.ids).await?;
    Ok(json_response(
        StatusCode::OK,
        &DeletedRequestsResponse { deleted },
    ))
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DeletedRequestsResponse {
    deleted: usize,
}

#[cfg(test)]
#[path = "requests_tests.rs"]
mod tests;
