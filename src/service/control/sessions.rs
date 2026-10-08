//! Session Control API handlers, wire queries, and NDJSON presentation.

use super::{
    AgentTenantQuery, ControlResult, content, default_agent, default_tenant_selection,
    json_response,
};
use crate::agent::AgentKind;
use crate::management::DeleteSessionsCommand;
use crate::service::state::ServiceState;
use crate::session;
use crate::tenant::TenantSelection;
use anyhow::{Context, Result};
use axum::Json;
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use bytes::Bytes;
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio_stream::wrappers::ReceiverStream;

pub(super) async fn list_sessions(
    State(state): State<ServiceState>,
    Query(query): Query<AgentTenantQuery>,
) -> ControlResult {
    let selection = TenantSelection::parse(&query.tenant)?;
    let data = state
        .management
        .sessions
        .list(selection, query.agent)
        .await?;
    Ok(json_response(StatusCode::OK, &data))
}

pub(super) async fn session_summary(
    State(state): State<ServiceState>,
    Query(query): Query<AgentTenantQuery>,
) -> ControlResult {
    let selection = TenantSelection::parse(&query.tenant)?;
    let summary = state
        .management
        .sessions
        .summary(selection, query.agent)
        .await?;
    Ok(json_response(StatusCode::OK, &summary))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionDetailQuery {
    #[serde(default = "default_tenant_selection")]
    tenant: String,
    #[serde(default = "default_agent")]
    agent: AgentKind,
    id: String,
}

pub(super) async fn session_detail(
    State(state): State<ServiceState>,
    Query(query): Query<SessionDetailQuery>,
) -> ControlResult {
    let selection = TenantSelection::parse(&query.tenant)?;
    let access = state.management.sessions.access(selection, query.agent)?;
    let agent = query.agent;
    let id = query.id;
    let (sender, receiver) = tokio::sync::mpsc::channel::<Bytes>(8);
    tokio::task::spawn_blocking(move || {
        let result = access.stream_detail(
            &id,
            &mut |meta| send_ndjson(&sender, &SessionDetailFrame::Meta { meta: meta.clone() }),
            &mut |record| {
                let frame = match record {
                    session::DetailRecord::Message(message) => {
                        SessionDetailFrame::Message { message }
                    }
                    session::DetailRecord::Tool(tool_activity) => {
                        SessionDetailFrame::ToolActivity { tool_activity }
                    }
                    session::DetailRecord::Evidence(evidence) => {
                        SessionDetailFrame::Evidence { evidence }
                    }
                };
                send_ndjson(&sender, &frame)
            },
        );
        let frame = match result {
            Ok((_meta, stats, warnings)) => SessionDetailFrame::Complete { stats, warnings },
            Err(error) => SessionDetailFrame::Error {
                agent,
                error: format!("{error:#}"),
            },
        };
        let _ = send_ndjson(&sender, &frame);
    });
    let stream = ReceiverStream::new(receiver).map(Ok::<Bytes, Infallible>);
    Ok(content(
        StatusCode::OK,
        "application/x-ndjson; charset=utf-8",
        Body::from_stream(stream),
    ))
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionEvidenceQuery {
    #[serde(default = "default_tenant_selection")]
    tenant: String,
    #[serde(default = "default_agent")]
    agent: AgentKind,
    id: String,
    entry: String,
    snapshot: String,
}

pub(super) async fn session_evidence(
    State(state): State<ServiceState>,
    Query(query): Query<SessionEvidenceQuery>,
) -> ControlResult {
    let selection = TenantSelection::parse(&query.tenant)?;
    let evidence = state
        .management
        .sessions
        .evidence(
            selection,
            query.agent,
            query.id,
            query.entry,
            query.snapshot,
        )
        .await?;
    Ok(json_response(StatusCode::OK, &evidence))
}

fn send_ndjson(sender: &tokio::sync::mpsc::Sender<Bytes>, value: &impl Serialize) -> Result<bool> {
    let mut line = serde_json::to_vec(value).context("serialize Session stream record")?;
    line.push(b'\n');
    Ok(sender.blocking_send(Bytes::from(line)).is_ok())
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct DeleteSessionsRequest {
    #[serde(default = "default_tenant_selection")]
    tenant: String,
    #[serde(default = "default_agent")]
    agent: AgentKind,
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default)]
    all: bool,
    confirmation: String,
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum SessionDetailFrame {
    Message {
        message: session::ConversationMessage,
    },
    ToolActivity {
        tool_activity: session::ToolActivity,
    },
    Evidence {
        evidence: session::TranscriptEvidenceSummary,
    },
    Meta {
        meta: session::SessionDetailMeta,
    },
    Complete {
        stats: session::SessionDetailStats,
        warnings: Vec<String>,
    },
    Error {
        agent: AgentKind,
        error: String,
    },
}

pub(super) async fn delete_sessions(
    State(state): State<ServiceState>,
    Json(request): Json<DeleteSessionsRequest>,
) -> ControlResult {
    let command = DeleteSessionsCommand {
        tenant: TenantSelection::parse(&request.tenant)?,
        agent: request.agent,
        ids: request.ids,
        all: request.all,
        confirmation: request.confirmation,
    };
    let deleted = state.management.sessions.delete(command).await?;
    Ok(json_response(
        StatusCode::OK,
        &DeletedSessionsResponse { deleted },
    ))
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DeletedSessionsResponse {
    deleted: usize,
}
