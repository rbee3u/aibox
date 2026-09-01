use super::super::{api_error, json_response};
use crate::request::{
    AssessmentFinding, AssessmentLevel, AssessmentSource, ProtocolSummary, RecordedHeader,
    RequestAssessment, RequestDetailReadError, RequestInspection, RequestMetadata,
    RequestProxyState, RequestState, ResponseMetadata, ResponseSource, ResultMetadata,
    StoredRequestSummary, SummaryMetadata, anchored_at,
};
use anyhow::Context as _;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{Response, StatusCode};
use serde::{Deserialize, Serialize};

const PAGE_SIZE: usize = 50;

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ListQuery {
    pub(super) page: Option<u64>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct RequestSummary {
    pub(super) id: String,
    pub(super) started_at: String,
    pub(super) ended_at: Option<String>,
    pub(super) method: String,
    pub(super) incoming_uri: String,
    pub(super) upstream_url: Option<String>,
    pub(super) status: Option<u16>,
    pub(super) http_version: Option<String>,
    pub(super) outcome: String,
    pub(super) state: RequestState,
    pub(super) total_ms: Option<u64>,
    pub(super) protocol: Option<ProtocolSummary>,
    pub(super) assessment: RequestAssessment,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct RequestList {
    pub(super) requests: Vec<RequestSummary>,
    pub(super) total: usize,
    pub(super) deletable_count: usize,
    pub(super) has_next: bool,
}

pub(crate) async fn list_requests(
    State(state): State<RequestProxyState>,
    Query(query): Query<ListQuery>,
) -> Response<Body> {
    let inspection = state.inspection();
    match tokio::task::spawn_blocking(move || list_requests_inner(&inspection, query.page)).await {
        Ok(Ok(value)) => json_response(StatusCode::OK, &value),
        Ok(Err(error)) => api_error(StatusCode::BAD_REQUEST, &error.to_string()),
        Err(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("scan Requests: {error}"),
        ),
    }
}

pub(super) fn list_requests_inner(
    inspection: &RequestInspection,
    page: Option<u64>,
) -> anyhow::Result<RequestList> {
    let page = page.unwrap_or(1);
    if page == 0 {
        anyhow::bail!("Request page must be a positive integer");
    }
    let start = usize::try_from(page - 1)
        .ok()
        .and_then(|page| page.checked_mul(PAGE_SIZE))
        .context("Request page is too large")?;
    let requests = inspection.list_page(start, PAGE_SIZE)?;
    let has_next = start
        .checked_add(PAGE_SIZE)
        .is_some_and(|next| next < requests.total);
    Ok(RequestList {
        requests: requests
            .requests
            .iter()
            .map(|request| summary(inspection, request))
            .collect(),
        total: requests.total,
        deletable_count: requests.deletable_count,
        has_next,
    })
}

fn state_name(active: bool, terminal: bool) -> RequestState {
    RequestState::from_snapshot(active, terminal)
}

fn summary(inspection: &RequestInspection, request: &StoredRequestSummary) -> RequestSummary {
    let value = &request.summary;
    let state = state_name(request.active, value.outcome.is_some());
    let outcome = match value.outcome {
        Some(outcome) if !request.active => outcome.as_str(),
        _ => state.as_str(),
    };
    let ended_at = value
        .terminal
        .then(|| {
            value
                .timing
                .finished_at_ns
                .as_deref()
                .and_then(|offset| anchored_at(&value.observed_at, offset))
        })
        .flatten();
    RequestSummary {
        id: value.request_id.clone(),
        started_at: value.observed_at.clone(),
        ended_at,
        method: value.request.method.clone(),
        incoming_uri: value.request.incoming_uri.clone(),
        upstream_url: value.request.upstream_url.clone(),
        status: value.response.as_ref().map(|response| response.status),
        http_version: value
            .response
            .as_ref()
            .map(|response| response.http_version.clone()),
        outcome: outcome.to_string(),
        state,
        total_ms: if request.active {
            request.live_elapsed_ns.as_deref().and_then(elapsed_ns_ms)
        } else {
            value
                .timing
                .finished_at_ns
                .as_deref()
                .and_then(elapsed_ns_ms)
        },
        protocol: value.protocol.clone(),
        assessment: inspection.assessment(value, request.active),
    }
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ResponseDetail {
    pub(super) format_version: u32,
    pub(super) source: ResponseSource,
    pub(super) headers_at: String,
    pub(super) status: u16,
    pub(super) http_version: String,
    pub(super) reason_phrase: Option<String>,
    pub(super) headers: Vec<RecordedHeader>,
}

impl From<ResponseMetadata> for ResponseDetail {
    fn from(metadata: ResponseMetadata) -> Self {
        let reason_phrase = StatusCode::from_u16(metadata.status)
            .ok()
            .and_then(|status| status.canonical_reason())
            .map(str::to_string);
        Self {
            format_version: crate::request::format_version(),
            source: metadata.source,
            headers_at: metadata.headers_at,
            status: metadata.status,
            http_version: metadata.http_version,
            reason_phrase,
            headers: metadata.headers,
        }
    }
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct RequestDetail {
    pub(super) request: RequestMetadata,
    pub(super) response: Option<ResponseDetail>,
    pub(super) result: Option<ResultMetadata>,
    pub(super) summary: SummaryMetadata,
    pub(super) assessment: RequestAssessment,
    pub(super) diagnostics: DiagnosticGroups,
    pub(super) state: RequestState,
    pub(super) request_body_bytes: u64,
    pub(super) response_body_bytes: u64,
    pub(super) live_total_ms: Option<u64>,
    pub(super) timeline_end_at_ns: Option<String>,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DiagnosticGroups {
    pub(super) request: Vec<AssessmentFinding>,
    pub(super) http: Vec<AssessmentFinding>,
    pub(super) provider: Vec<AssessmentFinding>,
    pub(super) warnings: Vec<AssessmentFinding>,
}

fn diagnostic_groups(
    inspection: &RequestInspection,
    summary: &SummaryMetadata,
    interrupted: bool,
) -> DiagnosticGroups {
    let mut groups = DiagnosticGroups {
        request: Vec::new(),
        http: Vec::new(),
        provider: Vec::new(),
        warnings: Vec::new(),
    };
    for finding in inspection.diagnostics(summary, interrupted) {
        if finding.level == AssessmentLevel::Warning {
            groups.warnings.push(finding);
        } else {
            match finding.source {
                AssessmentSource::Request => groups.request.push(finding),
                AssessmentSource::Http => groups.http.push(finding),
                AssessmentSource::Provider => groups.provider.push(finding),
                AssessmentSource::Diagnostic => groups.warnings.push(finding),
            }
        }
    }
    groups
}

pub(crate) async fn request_detail(
    State(state): State<RequestProxyState>,
    Path(id): Path<String>,
) -> Response<Body> {
    let inspection = state.inspection();
    let lookup_id = id.clone();
    let lookup = tokio::task::spawn_blocking(move || inspection.find_detail(&lookup_id)).await;
    match lookup {
        Ok(Ok(request)) => {
            let terminal = request.result.is_some();
            let display_state = state_name(request.active, terminal);
            let live_total_ms = request.live_elapsed_ns.as_deref().and_then(elapsed_ns_ms);
            let interrupted = !request.active && !terminal;
            let inspection = state.inspection();
            let assessment = inspection.assessment(&request.summary, request.active);
            let diagnostics = diagnostic_groups(&inspection, &request.summary, interrupted);
            let timeline_end_at_ns =
                inspection.timeline_end_at_ns(&request, request.live_elapsed_ns.clone());
            let response_headers_at = request
                .summary
                .timing
                .upstream_response_headers_at_ns
                .as_deref()
                .and_then(|offset| anchored_at(&request.summary.observed_at, offset));
            let response = request.response.map(|metadata| {
                let mut detail = ResponseDetail::from(metadata);
                if let Some(headers_at) = &response_headers_at {
                    detail.headers_at = headers_at.clone();
                }
                detail
            });
            json_response(
                StatusCode::OK,
                &RequestDetail {
                    request: request.request,
                    response,
                    result: request.result,
                    summary: request.summary,
                    assessment,
                    diagnostics,
                    state: display_state,
                    request_body_bytes: request.request_body_bytes,
                    response_body_bytes: request.response_body_bytes,
                    live_total_ms,
                    timeline_end_at_ns,
                },
            )
        }
        Ok(Err(RequestDetailReadError::Lookup(error))) => {
            api_error(StatusCode::NOT_FOUND, &error.to_string())
        }
        Ok(Err(RequestDetailReadError::EventIndex(error))) => {
            api_error(StatusCode::INTERNAL_SERVER_ERROR, &error.to_string())
        }
        Err(error) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("read Request detail: {error}"),
        ),
    }
}

fn elapsed_ns_ms(elapsed_ns: &str) -> Option<u64> {
    elapsed_ns
        .parse::<u128>()
        .ok()
        .and_then(|value| u64::try_from(value / 1_000_000).ok())
}
