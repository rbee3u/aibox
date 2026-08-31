//! Requests module JSON/body API.
//!
//! List handlers read only the materialized Request Summary while detail
//! reads stay strict over raw metadata, following
//! `docs/adr/0009-request-evidence-and-projections.md`. Bodies stream from
//! disk as recorded; the decoded variants only undo a recorded content coding.
//! Nothing here redacts, truncates, or expires a Request.

mod body;
mod projection;
mod timing;

#[cfg(test)]
pub(crate) use body::BodyQuery;
#[cfg(test)]
use body::{body_response, decoded_body_response};
use body::{decoded_request_body, decoded_response_body, request_body, response_body};
#[cfg(test)]
use projection::list_requests_inner;
#[cfg(test)]
pub(crate) use projection::{
    DiagnosticGroups, ListQuery, RequestDetail, RequestList, RequestSummary, ResponseDetail,
};
use projection::{list_requests, request_detail};
use timing::response_event_timings;
#[cfg(test)]
pub(crate) use timing::{
    EventTimingEntry, EventTimingQuery, EventTimingResponse, EventTimingState,
};

use super::routes::{
    REQUEST_BODY, REQUEST_BODY_DECODED, REQUEST_DETAIL, REQUESTS, REQUESTS_DELETE, RESPONSE_BODY,
    RESPONSE_BODY_DECODED, RESPONSE_EVENT_TIMINGS,
};
use super::{ControlResult, json_response};
#[cfg(test)]
pub(crate) use crate::request::RequestState;
#[cfg(test)]
use crate::request::{
    AssessmentLevel, AssessmentSource, RecordedHeader, RequestInspection, ResponseMetadata,
    ResponseSource,
};
use crate::service::coordination::RequestCoordinator;
use crate::service::state::ServiceState;
use axum::Json;
use axum::Router;
use axum::extract::State;
#[cfg(test)]
use axum::extract::{Path, Query};
use axum::http::StatusCode;
#[cfg(test)]
use axum::http::header;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;

/// Register the Requests routes.
///
/// Read paths extract [`RequestProxyState`] through `FromRef` because a
/// diagnostic read needs no mutation gate; deletion takes [`ServiceState`] so it
/// passes through [`RequestCoordinator`] and the shared management gate like
/// every other Console mutation.
pub(crate) fn api_router() -> Router<ServiceState> {
    Router::new()
        .route(REQUESTS, get(list_requests))
        .route(REQUESTS_DELETE, post(delete_requests))
        .route(REQUEST_DETAIL, get(request_detail))
        .route(REQUEST_BODY, get(request_body))
        .route(RESPONSE_BODY, get(response_body))
        .route(REQUEST_BODY_DECODED, get(decoded_request_body))
        .route(RESPONSE_BODY_DECODED, get(decoded_response_body))
        .route(RESPONSE_EVENT_TIMINGS, get(response_event_timings))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DeleteRequest {
    ids: Vec<String>,
}

pub(crate) async fn delete_requests(
    State(state): State<ServiceState>,
    Json(request): Json<DeleteRequest>,
) -> ControlResult {
    let deleted = RequestCoordinator::new(state).delete(request.ids).await?;
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
