//! Session Control API handlers, wire queries, and NDJSON presentation.

use super::*;
use crate::service::coordination::session::{DeleteSessionsCommand, SessionCoordinator};

pub(super) async fn list_sessions(
    State(state): State<ServiceState>,
    Query(query): Query<AgentTenantQuery>,
) -> Response<Body> {
    let selection = match TenantSelection::parse(&query.tenant) {
        Ok(selection) => selection,
        Err(error) => return result_error(error),
    };
    match SessionCoordinator::new(state)
        .list(selection, query.agent)
        .await
    {
        Ok(data) => json_response(StatusCode::OK, &data),
        Err(error) => result_error(error),
    }
}

pub(super) async fn session_summary(
    State(state): State<ServiceState>,
    Query(query): Query<AgentTenantQuery>,
) -> Response<Body> {
    let selection = match TenantSelection::parse(&query.tenant) {
        Ok(selection) => selection,
        Err(error) => return result_error(error),
    };
    match SessionCoordinator::new(state)
        .summary(selection, query.agent)
        .await
    {
        Ok(summary) => json_response(StatusCode::OK, &summary),
        Err(error) => result_error(error),
    }
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
) -> Response<Body> {
    let selection = match TenantSelection::parse(&query.tenant) {
        Ok(selection) => selection,
        Err(error) => return result_error(error),
    };
    let access = match SessionCoordinator::new(state).access(selection, query.agent) {
        Ok(access) => access,
        Err(error) => return result_error(error),
    };
    let agent = query.agent;
    let id = query.id;
    let (sender, receiver) = tokio::sync::mpsc::channel::<Bytes>(8);
    tokio::task::spawn_blocking(move || {
        let result = access.stream_detail(
            &id,
            &mut |meta| send_ndjson(&sender, &SessionDetailFrame::Meta { meta: meta.clone() }),
            &mut |record| match record {
                session::DetailRecord::Message(message) => {
                    send_ndjson(&sender, &SessionDetailFrame::Message { message })
                }
                session::DetailRecord::Tool(tool_activity) => {
                    send_ndjson(&sender, &SessionDetailFrame::ToolActivity { tool_activity })
                }
                session::DetailRecord::Evidence(evidence) => {
                    send_ndjson(&sender, &SessionDetailFrame::Evidence { evidence })
                }
            },
        );
        match result {
            Ok((_meta, stats, warnings)) => {
                let _ = send_ndjson(&sender, &SessionDetailFrame::Complete { stats, warnings });
            }
            Err(error) => {
                let _ = send_ndjson(
                    &sender,
                    &SessionDetailFrame::Error {
                        agent,
                        error: format!("{error:#}"),
                    },
                );
            }
        }
    });
    let stream = ReceiverStream::new(receiver).map(Ok::<Bytes, Infallible>);
    let mut response = Response::new(Body::from_stream(stream));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    response
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
) -> Response<Body> {
    let selection = match TenantSelection::parse(&query.tenant) {
        Ok(selection) => selection,
        Err(error) => return result_error(error),
    };
    match SessionCoordinator::new(state)
        .evidence(
            selection,
            query.agent,
            query.id,
            query.entry,
            query.snapshot,
        )
        .await
    {
        Ok(evidence) => json_response(StatusCode::OK, &evidence),
        Err(error) => result_error(error),
    }
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
) -> Response<Body> {
    let command = DeleteSessionsCommand {
        tenant: request.tenant,
        agent: request.agent,
        ids: request.ids,
        all: request.all,
        confirmation: request.confirmation,
    };
    match SessionCoordinator::new(state).delete(command).await {
        Ok(deleted) => json_response(StatusCode::OK, &DeletedSessionsResponse { deleted }),
        Err(error) => result_error(error),
    }
}

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct DeletedSessionsResponse {
    deleted: usize,
}
