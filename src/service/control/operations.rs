//! Management Operation Control API handlers and wire commands.

use super::{ControlResult, json_response};
use crate::service::coordination::OperationCoordinator;
use crate::service::state::ServiceState;
use async_stream::stream;
use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{Response, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::Infallible;
use std::time::Duration;

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct OperationQuery {
    after_sequence: Option<u64>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct OperationEnvelope {
    operation: Option<crate::service::operation::OperationSnapshot>,
    gap: bool,
}

pub(super) async fn current_operation(
    State(state): State<ServiceState>,
    Query(query): Query<OperationQuery>,
) -> Response<Body> {
    let view = OperationCoordinator::new(state).current(query.after_sequence);
    json_response(
        StatusCode::OK,
        &OperationEnvelope {
            operation: view.operation,
            gap: view.gap,
        },
    )
}

pub(super) async fn operation_events(
    State(state): State<ServiceState>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let coordinator = OperationCoordinator::new(state.clone());
    let shutdown = state.request().shutdown_token();
    let mut changes = coordinator.subscribe();
    let events = stream! {
        let mut cursor = coordinator.event_cursor();
        loop {
            let view = cursor.next(&coordinator);
            let payload = serde_json::to_string(&OperationEnvelope {
                operation: view.operation,
                gap: view.gap,
            })
                .unwrap_or_else(|_| "{\"operation\":null,\"gap\":false}".to_string());
            yield Ok(Event::default().event("operation").data(payload));
            tokio::select! {
                change = changes.recv() => match change {
                    Ok(()) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                () = shutdown.cancelled() => break,
            }
        }
    };
    Sse::new(events).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct BuildRequest {
    #[serde(default)]
    force: bool,
}

pub(super) async fn start_build(
    State(state): State<ServiceState>,
    Json(request): Json<BuildRequest>,
) -> ControlResult {
    let operation = OperationCoordinator::new(state).start_build(request.force)?;
    Ok(json_response(StatusCode::ACCEPTED, &operation))
}

pub(super) async fn cancel_operation(
    State(state): State<ServiceState>,
    Path(id): Path<String>,
    Json(_request): Json<Value>,
) -> ControlResult {
    OperationCoordinator::new(state).cancel(&id)?;
    Ok(json_response(
        StatusCode::ACCEPTED,
        &CancelledOperationResponse { cancelled: id },
    ))
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct CancelledOperationResponse {
    cancelled: String,
}
