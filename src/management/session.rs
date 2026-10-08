//! Session discovery, evidence access, and deletion coordination.

use super::{ManagementGate, ManagementPaths, run_blocking};
use crate::agent::AgentKind;
use crate::application_error::{ApplicationErrorKind, application_error};
use crate::session;
use crate::tenant::TenantSelection;
use anyhow::Result;
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct SessionCoordinator {
    paths: ManagementPaths,
    gate: ManagementGate,
}

pub(crate) struct SessionAccess {
    home: PathBuf,
    agent: AgentKind,
}

pub(crate) struct DeleteSessionsCommand {
    pub(crate) tenant: TenantSelection,
    pub(crate) agent: AgentKind,
    pub(crate) ids: Vec<String>,
    pub(crate) all: bool,
    pub(crate) confirmation: String,
}

impl SessionCoordinator {
    pub(super) fn new(paths: ManagementPaths, gate: ManagementGate) -> Self {
        Self { paths, gate }
    }

    pub(crate) fn access(
        &self,
        selection: TenantSelection,
        agent: AgentKind,
    ) -> Result<SessionAccess> {
        let tenant = selection.resolve(&self.paths.root, &self.paths.host_home)?;
        tenant.validate_session_home()?;
        Ok(SessionAccess {
            home: tenant.home_dir().to_path_buf(),
            agent,
        })
    }

    pub(crate) async fn list(
        &self,
        selection: TenantSelection,
        agent: AgentKind,
    ) -> Result<session::SessionListData> {
        let access = self.access(selection, agent)?;
        run_blocking(move || session::list_session_data(access.agent, &access.home)).await
    }

    pub(crate) async fn summary(
        &self,
        selection: TenantSelection,
        agent: AgentKind,
    ) -> Result<session::SessionDiscoverySummary> {
        let access = self.access(selection, agent)?;
        run_blocking(move || session::session_discovery_summary(access.agent, &access.home)).await
    }

    pub(crate) async fn evidence(
        &self,
        selection: TenantSelection,
        agent: AgentKind,
        id: String,
        entry: String,
        snapshot: String,
    ) -> Result<session::TranscriptEvidence> {
        let access = self.access(selection, agent)?;
        run_blocking(move || {
            session::read_session_evidence(access.agent, &access.home, &id, &entry, &snapshot)
        })
        .await
    }

    pub(crate) async fn delete(&self, command: DeleteSessionsCommand) -> Result<usize> {
        if command.all && command.confirmation != "delete all sessions" {
            return Err(application_error(
                ApplicationErrorKind::InvalidInput,
                "confirmation does not match",
            ));
        }
        let access = self.access(command.tenant, command.agent)?;
        let mutation = self.gate.begin()?;
        mutation
            .run_blocking(move || {
                session::delete_session_catalog(
                    access.agent,
                    &access.home,
                    &command.ids,
                    command.all,
                )
            })
            .await
    }
}

impl SessionAccess {
    pub(crate) fn stream_detail(
        &self,
        id: &str,
        visit_meta: &mut impl FnMut(&session::SessionDetailMeta) -> Result<bool>,
        visit_record: &mut impl FnMut(session::DetailRecord) -> Result<bool>,
    ) -> Result<(
        session::SessionDetailMeta,
        session::SessionDetailStats,
        Vec<String>,
    )> {
        session::stream_session_detail(self.agent, &self.home, id, visit_meta, visit_record)
    }
}
