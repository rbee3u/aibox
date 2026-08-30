//! Test-only export of the Rust-owned Console wire contract.

#[cfg(test)]
mod tests {
    use crate::agent::AgentKind;
    use crate::component::{ComponentKind, LatestEntry, LatestEntryState, LatestSnapshot};
    use crate::config::{
        ApplicationStatus, ConfigCatalogEntry, ConfigCatalogState, ConfigDrift, LastApplication,
    };
    use crate::config::{
        AuthPropagationPreview, AuthPropagationReport, PropagationEntry, PropagationOutcome,
        PropagationPreviewEntry,
    };
    use crate::config::{
        CustomProviderInput, CustomProviderState, VisualAuthInput, VisualConfigOptionInput,
        VisualConfigOptionState, VisualConfigState,
    };
    use crate::request::{
        AssessmentFinding, AssessmentLevel, AssessmentPrimary, AssessmentSource,
        DiagnosticMetadata, ErrorKind, ErrorMetadata, Outcome, ProtocolDiagnostic, ProtocolFamily,
        ProtocolSummary, RecordedHeader, RequestAssessment, RequestMetadata, RequestedEffective,
        RequestedObserved, ResponseMetadata, ResponseModeValue, ResponseSource, ResultMetadata,
        SummaryMetadata, SummaryRequestMetadata, SummaryResponseMetadata, TimingMetadata,
        TokenUsage,
    };
    use crate::service::control::components::{
        ComponentMutation, ComponentQuery, ComponentRow, ComponentStatusWire,
        InstalledComponentResponse, RemovedComponentResponse,
    };
    use crate::service::control::configs::{
        AuthPropagationPreviewResponse, ConfigAuthResponse, ConfigDiagnostic, ConfigFileRequest,
        ConfigFileResponse, ConfigListResponse, ConfigMutationBase, CreatedConfigResponse,
        DeleteConfigsRequest, DeletedConfigsResponse, DiagnoseConfigRequest,
        DiagnoseConfigResponse, ExecuteAuthPropagationRequest, LinkedConfigFileResponse,
        SaveConfigFileRequest,
    };
    use crate::service::control::operations::{
        BuildRequest, CancelledOperationResponse, OperationEnvelope, OperationQuery,
    };
    use crate::service::control::overview::{
        BootstrapResponse, DockerOverview, OverviewResponse, RequestOverview, RuntimeImageOverview,
        ServiceOverview, TopologyAgent, TopologyComponents, TopologyCurrentConfig,
        TopologyNamedConfigs, TopologyResponse, TopologyTenant,
    };
    use crate::service::control::requests::{
        BodyQuery, DeleteRequest, DeletedRequestsResponse, DiagnosticGroups, EventTimingEntry,
        EventTimingQuery, EventTimingResponse, EventTimingState, ListQuery, RequestApiError,
        RequestDetail, RequestList, RequestState, RequestSummary, ResponseDetail,
    };
    use crate::service::control::routes::ENDPOINTS;
    use crate::service::control::sessions::{
        DeleteSessionsRequest, DeletedSessionsResponse, SessionDetailFrame, SessionDetailQuery,
        SessionEvidenceQuery,
    };
    use crate::service::control::tenants::{
        CreateTenantRequest, CreatedTenantResponse, DeleteSelection, DeletedTenantsResponse,
        TenantRow,
    };
    use crate::service::control::{AgentTenantQuery, ControlErrorBody, ControlErrorResponse};
    use crate::service::operation::{OperationLog, OperationSnapshot, OperationState};
    use crate::session::{
        ConversationMessage, ConversationRole, EvidenceEncoding, SessionDetailMeta,
        SessionDetailStats, SessionDiscoverySummary, SessionListData, SessionListRow, ToolActivity,
        ToolActivityStatus, TranscriptEvidence, TranscriptEvidenceSummary,
    };
    use serde::Serialize;
    use std::collections::VecDeque;
    use std::fs;
    use std::path::PathBuf;
    use ts_rs::{Config, TS};

    fn declaration<T: TS>(config: &Config) -> String {
        format!("export {}\n", T::decl(config))
    }

    fn bindings() -> String {
        let config = Config::default().with_large_int("number");
        let mut output = String::from(
            "// Generated from Rust wire DTOs by make console-contract. Do not edit.\n\n\
             export type JsonValue = number | boolean | string | JsonValue[] | { [key: string]: JsonValue };\n",
        );
        macro_rules! export_types {
            ($($type:ty),+ $(,)?) => {
                $(output.push_str(&declaration::<$type>(&config));)+
            };
        }
        export_types!(
            AgentKind,
            AgentTenantQuery,
            ControlErrorBody<'static>,
            ControlErrorResponse<'static>,
            ComponentQuery,
            ComponentMutation,
            ComponentKind,
            ComponentStatusWire,
            BootstrapResponse,
            TenantRow,
            ComponentRow,
            InstalledComponentResponse,
            RemovedComponentResponse,
            ConfigListResponse,
            AuthPropagationPreviewResponse,
            ExecuteAuthPropagationRequest,
            ConfigMutationBase,
            ConfigFileRequest,
            ConfigFileResponse,
            LinkedConfigFileResponse,
            ConfigAuthResponse,
            SaveConfigFileRequest,
            DiagnoseConfigRequest,
            ConfigDiagnostic,
            DiagnoseConfigResponse,
            DeleteConfigsRequest,
            CreatedConfigResponse,
            DeletedConfigsResponse,
            OperationQuery,
            OperationEnvelope,
            BuildRequest,
            CancelledOperationResponse,
            OverviewResponse,
            ServiceOverview,
            DockerOverview,
            RuntimeImageOverview,
            RequestOverview,
            TopologyResponse,
            TopologyTenant,
            TopologyAgent,
            TopologyCurrentConfig,
            TopologyNamedConfigs,
            TopologyComponents,
            SessionDetailQuery,
            SessionEvidenceQuery,
            DeleteSessionsRequest,
            SessionDetailFrame,
            DeletedSessionsResponse,
            CreateTenantRequest,
            DeleteSelection,
            CreatedTenantResponse,
            DeletedTenantsResponse,
            ListQuery,
            RequestSummary,
            RequestList,
            RequestState,
            ResponseDetail,
            RequestDetail,
            DiagnosticGroups,
            BodyQuery,
            EventTimingQuery,
            EventTimingEntry,
            EventTimingResponse,
            EventTimingState,
            DeleteRequest,
            DeletedRequestsResponse,
            RequestApiError<'static>,
            RecordedHeader,
            RequestMetadata,
            ResponseSource,
            ResponseMetadata,
            Outcome,
            ErrorMetadata,
            ErrorKind,
            TimingMetadata,
            DiagnosticMetadata,
            SummaryRequestMetadata,
            SummaryResponseMetadata,
            AssessmentLevel,
            AssessmentSource,
            AssessmentPrimary,
            RequestAssessment,
            AssessmentFinding,
            ProtocolFamily,
            ResponseModeValue,
            RequestedEffective<String>,
            RequestedObserved<String>,
            TokenUsage,
            ProtocolDiagnostic,
            ProtocolSummary,
            SummaryMetadata,
            ResultMetadata,
            VisualConfigOptionInput,
            CustomProviderInput,
            VisualAuthInput,
            VisualConfigOptionState,
            CustomProviderState,
            VisualConfigState,
            LatestEntry,
            LatestEntryState,
            LatestSnapshot,
            LastApplication,
            ConfigDrift,
            ApplicationStatus,
            ConfigCatalogState,
            ConfigCatalogEntry,
            PropagationOutcome,
            PropagationEntry,
            AuthPropagationReport,
            PropagationPreviewEntry,
            AuthPropagationPreview,
            OperationState,
            OperationLog,
            OperationSnapshot,
            SessionDiscoverySummary,
            SessionListRow,
            SessionListData,
            ConversationMessage,
            ConversationRole,
            ToolActivity,
            ToolActivityStatus,
            TranscriptEvidenceSummary,
            SessionDetailStats,
            SessionDetailMeta,
            TranscriptEvidence,
            EvidenceEncoding,
        );
        output
    }

    #[derive(Serialize)]
    struct ContractSamples {
        bootstrap: BootstrapResponse,
        tenants: Vec<TenantRow>,
        component_rows: Vec<ComponentRow>,
        component_statuses: Vec<Option<ComponentStatusWire>>,
        component_latest: LatestSnapshot,
        application: ApplicationStatus,
        config_drifts: Vec<ConfigDrift>,
        propagation_outcomes: Vec<PropagationOutcome>,
        outcomes: Vec<Outcome>,
        assessment_levels: Vec<AssessmentLevel>,
        operation_states: Vec<OperationState>,
        session_frames: Vec<SessionDetailFrame>,
        operation: OperationSnapshot,
    }

    fn samples() -> ContractSamples {
        ContractSamples {
            bootstrap: BootstrapResponse {
                version: "0.1.0",
                csrf_token: "contract-csrf".to_string(),
                listen: "127.0.0.1:4422".to_string(),
            },
            tenants: vec![
                TenantRow::Host {
                    name: None,
                    display_name: "Host Tenant".to_string(),
                    home: "/Users/example".to_string(),
                    exists: true,
                },
                TenantRow::Managed {
                    name: "default".to_string(),
                    display_name: "default".to_string(),
                    home: "/tmp/aibox/tenants/default".to_string(),
                    exists: true,
                },
            ],
            component_rows: ComponentKind::ALL
                .into_iter()
                .zip([
                    ComponentStatusWire::NotInstalled,
                    ComponentStatusWire::Installed,
                    ComponentStatusWire::Incomplete,
                    ComponentStatusWire::Modified,
                    ComponentStatusWire::Unmanaged,
                    ComponentStatusWire::NotInstalled,
                    ComponentStatusWire::Installed,
                    ComponentStatusWire::Incomplete,
                ])
                .map(|(kind, status)| ComponentRow {
                    kind,
                    supports_version: kind.supports_version(),
                    status: Some(status),
                    version: (status == ComponentStatusWire::Installed)
                        .then(|| "1.2.3".to_string()),
                    error: None,
                })
                .collect(),
            component_latest: LatestSnapshot {
                checked_at: "2026-08-27T00:00:00Z".to_string(),
                entries: vec![LatestEntry {
                    kind: ComponentKind::Codex,
                    state: LatestEntryState::Available,
                    version: Some("1.2.3".to_string()),
                    source: "GitHub Releases".to_string(),
                    error: None,
                }],
            },
            component_statuses: vec![
                None,
                Some(ComponentStatusWire::NotInstalled),
                Some(ComponentStatusWire::Installed),
                Some(ComponentStatusWire::Incomplete),
                Some(ComponentStatusWire::Modified),
                Some(ComponentStatusWire::Unmanaged),
            ],
            application: ApplicationStatus {
                last_application: Some(LastApplication {
                    applied: "default".to_string(),
                    applied_at: "2026-08-27T00:00:00Z".to_string(),
                }),
                drift: ConfigDrift::Clean,
                detail: None,
            },
            config_drifts: vec![
                ConfigDrift::Untracked,
                ConfigDrift::Clean,
                ConfigDrift::Dirty,
                ConfigDrift::SourceMissing,
                ConfigDrift::ComparisonError,
            ],
            propagation_outcomes: vec![
                PropagationOutcome::Updated,
                PropagationOutcome::Unchanged,
                PropagationOutcome::Conflict {
                    last_refresh: "2026-08-27T00:00:00Z".to_string(),
                },
                PropagationOutcome::Newer {
                    target_last_refresh: "2026-08-28T00:00:00Z".to_string(),
                    source_last_refresh: "2026-08-27T00:00:00Z".to_string(),
                },
                PropagationOutcome::Invalid {
                    reason: "malformed credential".to_string(),
                },
                PropagationOutcome::Failed {
                    reason: "write failed".to_string(),
                },
            ],
            outcomes: vec![
                Outcome::Completed,
                Outcome::Rejected,
                Outcome::UpstreamError,
                Outcome::ClientDisconnected,
                Outcome::RecordingFailed,
                Outcome::ServerShutdown,
            ],
            assessment_levels: vec![
                AssessmentLevel::Active,
                AssessmentLevel::Ok,
                AssessmentLevel::Warning,
                AssessmentLevel::Error,
            ],
            operation_states: vec![
                OperationState::Running,
                OperationState::Succeeded,
                OperationState::Failed,
                OperationState::Cancelled,
            ],
            session_frames: vec![
                SessionDetailFrame::Message {
                    message: ConversationMessage {
                        entry_ids: vec!["entry-1".to_string()],
                        role: ConversationRole::User,
                        timestamp: "2026-08-27T00:00:00Z".to_string(),
                        text: "hello".to_string(),
                    },
                },
                SessionDetailFrame::ToolActivity {
                    tool_activity: ToolActivity {
                        entry_ids: vec!["entry-2".to_string()],
                        call_id: Some("call-1".to_string()),
                        timestamp: "2026-08-27T00:00:01Z".to_string(),
                        name: "shell".to_string(),
                        status: ToolActivityStatus::Completed,
                        summary: "ok".to_string(),
                    },
                },
                SessionDetailFrame::Evidence {
                    evidence: TranscriptEvidenceSummary {
                        entry_id: "entry-3".to_string(),
                        line: 3,
                        timestamp: "2026-08-27T00:00:02Z".to_string(),
                        native_type: "assistant".to_string(),
                        role: Some("assistant".to_string()),
                        content_types: vec!["text".to_string()],
                        status: "readable".to_string(),
                        preview: "answer".to_string(),
                    },
                },
                SessionDetailFrame::Meta {
                    meta: SessionDetailMeta {
                        id: "session-1".to_string(),
                        title: "Sample".to_string(),
                        start_ts: "2026-08-27T00:00:00Z".to_string(),
                        transcript_path: "/tmp/session.jsonl".to_string(),
                        cwd: Some("/tmp".to_string()),
                        model_provider: Some("openai".to_string()),
                        cli_version: Some("1.0.0".to_string()),
                    },
                },
                SessionDetailFrame::Complete {
                    stats: SessionDetailStats {
                        start_ts: "2026-08-27T00:00:00Z".to_string(),
                        last_event_ts: "2026-08-27T00:00:02Z".to_string(),
                        observed_duration_ms: Some(2000),
                        message_count: 1,
                        tool_count: 1,
                        entry_count: 3,
                        malformed_count: 0,
                        unsupported_count: 0,
                        hidden_internal_count: 0,
                        file_size: 128,
                        snapshot: "snapshot-1".to_string(),
                    },
                    warnings: vec![],
                },
                SessionDetailFrame::Error {
                    agent: AgentKind::Codex,
                    error: "sample error".to_string(),
                },
            ],
            operation: OperationSnapshot {
                id: "0198f000-0000-7000-8000-000000000000".to_string(),
                kind: "install codex@1.2.3".to_string(),
                state: OperationState::Running,
                started_at: "2026-08-27T00:00:00Z".to_string(),
                ended_at: None,
                result: None,
                first_sequence: 4,
                next_sequence: 5,
                logs: VecDeque::from([OperationLog {
                    sequence: 4,
                    message: "Installing codex@1.2.3".to_string(),
                }]),
            },
        }
    }

    #[test]
    #[ignore = "explicitly updates committed Console wire contracts"]
    fn export_console_contract() {
        let directory = PathBuf::from(
            std::env::var_os("AIBOX_CONTRACT_DIR")
                .expect("AIBOX_CONTRACT_DIR is required for explicit contract export"),
        );
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("wire.ts"), bindings()).unwrap();
        let routes = ENDPOINTS
            .iter()
            .map(|endpoint| {
                format!(
                    "  {}: {{ method: \"{}\", path: \"{}\" }},\n",
                    endpoint.key, endpoint.method, endpoint.path
                )
            })
            .collect::<String>();
        fs::write(
            directory.join("routes.ts"),
            format!(
                "// Generated from Rust Control routes by make console-contract. Do not edit.\n\nexport const routes = {{\n{routes}}} as const;\n"
            ),
        )
        .unwrap();
        let samples = serde_json::to_vec_pretty(&samples()).unwrap();
        fs::write(
            directory.join("samples.json"),
            [samples, b"\n".to_vec()].concat(),
        )
        .unwrap();
    }
}
