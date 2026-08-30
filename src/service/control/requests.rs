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

use super::response::content;
use super::routes::{
    REQUEST_BODY, REQUEST_BODY_DECODED, REQUEST_DETAIL, REQUESTS, REQUESTS_DELETE, RESPONSE_BODY,
    RESPONSE_BODY_DECODED, RESPONSE_EVENT_TIMINGS,
};
use crate::request::RequestProxyState;
#[cfg(test)]
pub(crate) use crate::request::RequestState;
#[cfg(test)]
use crate::request::{
    AssessmentLevel, AssessmentSource, RecordedHeader, RequestInspection, ResponseMetadata,
    ResponseSource,
};
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{FromRef, State};
#[cfg(test)]
use axum::extract::{Path, Query};
#[cfg(test)]
use axum::http::header;
use axum::http::{Response, StatusCode};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::json;

pub(crate) fn api_router<S>() -> Router<S>
where
    RequestProxyState: FromRef<S>,
    S: Clone + Send + Sync + 'static,
{
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
    State(state): State<RequestProxyState>,
    Json(request): Json<DeleteRequest>,
) -> Response<Body> {
    let inspection = state.inspection();
    let deleted = tokio::task::spawn_blocking(move || inspection.delete_ids(&request.ids)).await;
    match deleted {
        Ok(Ok(deleted)) => json_response(StatusCode::OK, &DeletedRequestsResponse { deleted }),
        Ok(Err(error)) => {
            let status = match crate::application_error::ApplicationError::kind(&error) {
                Some(crate::application_error::ApplicationErrorKind::NotFound) => {
                    StatusCode::NOT_FOUND
                }
                Some(crate::application_error::ApplicationErrorKind::Conflict) => {
                    StatusCode::CONFLICT
                }
                Some(crate::application_error::ApplicationErrorKind::InputTooLarge) => {
                    StatusCode::PAYLOAD_TOO_LARGE
                }
                Some(crate::application_error::ApplicationErrorKind::Internal) => {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
                _ => StatusCode::BAD_REQUEST,
            };
            json_error(status, &format!("{error:#}"))
        }
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("delete Requests: {error}"),
        ),
    }
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DeletedRequestsResponse {
    deleted: usize,
}

fn json_response<T: Serialize>(status: StatusCode, value: &T) -> Response<Body> {
    match serde_json::to_vec(value) {
        Ok(bytes) => content(status, "application/json; charset=utf-8", bytes),
        Err(error) => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("serialize Request API response: {error}"),
        ),
    }
}

fn json_error(status: StatusCode, message: &str) -> Response<Body> {
    let body = serde_json::to_vec(&RequestApiError { error: message })
        .unwrap_or_else(|_| b"{\"error\":\"Request API error\"}".to_vec());
    content(status, "application/json; charset=utf-8", body)
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct RequestApiError<'a> {
    error: &'a str,
}

#[cfg(test)]
#[path = "requests_tests.rs"]
mod tests;
