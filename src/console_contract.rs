//! Explicit test-only export of the Rust-owned Console wire contract.

#[cfg(test)]
mod tests {
    use crate::agent::AgentKind;
    use crate::component::updates::{LatestEntry, LatestEntryState, LatestSnapshot};
    use crate::config::model::{
        CustomProviderInput, CustomProviderState, VisualAuthInput, VisualConfigOptionInput,
        VisualConfigOptionState, VisualConfigState,
    };
    use crate::config::{
        ApplicationStatus, ConfigCatalogEntry, ConfigCatalogState, ConfigDrift, LastApplication,
    };
    use crate::config::{
        AuthPropagationPreview, AuthPropagationReport, PropagationEntry, PropagationOutcome,
        PropagationPreviewEntry,
    };
    use crate::request::model::{
        AssessmentFinding, AssessmentLevel, AssessmentPrimary, AssessmentSource,
        DiagnosticMetadata, ErrorKind, ErrorMetadata, Outcome, ProtocolDiagnostic, ProtocolFamily,
        ProtocolSummary, RecordedHeader, RequestAssessment, RequestMetadata, RequestedEffective,
        RequestedObserved, ResponseMetadata, ResponseModeValue, ResponseSource, ResultMetadata,
        SummaryMetadata, SummaryRequestMetadata, SummaryResponseMetadata, TimingMetadata,
        TokenUsage,
    };
    use crate::service::control::{
        AgentTenantQuery, AuthPropagationPreviewResponse, BodyQuery, BootstrapResponse,
        BuildRequest, CancelledOperationResponse, ComponentMutation, ComponentQuery, ComponentRow,
        ConfigAuthResponse, ConfigDiagnostic, ConfigFileRequest, ConfigFileResponse,
        ConfigListResponse, ConfigMutationBase, ControlErrorBody, ControlErrorResponse,
        CreateTenantRequest, CreatedConfigResponse, CreatedTenantResponse, DeleteConfigsRequest,
        DeleteRequest, DeleteSelection, DeleteSessionsRequest, DeletedConfigsResponse,
        DeletedRequestsResponse, DeletedSessionsResponse, DeletedTenantsResponse,
        DiagnoseConfigRequest, DiagnoseConfigResponse, DiagnosticGroups, DockerOverview,
        EventTimingEntry, EventTimingQuery, EventTimingResponse, EventTimingState,
        ExecuteAuthPropagationRequest, InstalledComponentResponse, LinkedConfigFileResponse,
        ListQuery, OperationEnvelope, OperationQuery, OverviewResponse, RemovedComponentResponse,
        RequestApiError, RequestDetail, RequestList, RequestOverview, RequestState, RequestSummary,
        ResponseDetail, RuntimeImageOverview, SaveConfigFileRequest, ServiceOverview,
        SessionDetailFrame, SessionDetailQuery, SessionEvidenceQuery, TenantRow, TopologyAgent,
        TopologyComponents, TopologyCurrentConfig, TopologyNamedConfigs, TopologyResponse,
        TopologyTenant,
    };
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
        component_latest: LatestSnapshot,
        application: ApplicationStatus,
        propagation_outcomes: Vec<PropagationOutcome>,
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
            component_latest: LatestSnapshot {
                checked_at: "2026-08-27T00:00:00Z".to_string(),
                entries: vec![LatestEntry {
                    kind: "codex".to_string(),
                    state: LatestEntryState::Available,
                    version: Some("1.2.3".to_string()),
                    source: "GitHub Releases".to_string(),
                    error: None,
                }],
            },
            application: ApplicationStatus {
                last_application: Some(LastApplication {
                    applied: "default".to_string(),
                    applied_at: "2026-08-27T00:00:00Z".to_string(),
                }),
                drift: ConfigDrift::Clean,
                detail: None,
            },
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
        let samples = serde_json::to_vec_pretty(&samples()).unwrap();
        fs::write(
            directory.join("samples.json"),
            [samples, b"\n".to_vec()].concat(),
        )
        .unwrap();
    }
}
