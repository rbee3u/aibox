use super::super::{api_error, json_response};
use crate::request::RequestProxyState;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{Response, StatusCode};
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct EventTimingQuery {
    #[serde(default)]
    pub(super) after_sequence: u64,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct EventTimingEntry {
    pub(super) sequence: u64,
    pub(super) completed_at_ns: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct EventTimingResponse {
    pub(super) state: EventTimingState,
    pub(super) events: Vec<EventTimingEntry>,
    pub(super) next_sequence: u64,
    pub(super) warning: Option<String>,
}

#[derive(Clone, Copy, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub(crate) enum EventTimingState {
    Available,
    Unavailable,
    Partial,
}

pub(crate) async fn response_event_timings(
    State(state): State<RequestProxyState>,
    Path(id): Path<String>,
    Query(query): Query<EventTimingQuery>,
) -> Response<Body> {
    let inspection = state.inspection();
    let timings = tokio::task::spawn_blocking(move || {
        inspection.read_event_timings(&id, query.after_sequence)
    })
    .await;
    match timings {
        Ok(Ok(timings)) => json_response(
            StatusCode::OK,
            &EventTimingResponse {
                state: if !timings.available {
                    EventTimingState::Unavailable
                } else if timings.partial {
                    EventTimingState::Partial
                } else {
                    EventTimingState::Available
                },
                events: timings
                    .events
                    .into_iter()
                    .map(|entry| EventTimingEntry {
                        sequence: entry.sequence,
                        completed_at_ns: entry.completed_at_ns,
                    })
                    .collect(),
                next_sequence: timings.next_sequence,
                warning: timings.warning,
            },
        ),
        Ok(Err(error)) => api_error(StatusCode::NOT_FOUND, &error.to_string()),
        Err(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("read Request SSE event timings: {error}"),
        ),
    }
}
