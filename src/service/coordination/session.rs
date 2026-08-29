//! Session discovery, evidence access, and deletion coordination.

use super::run_blocking;
use crate::agent::AgentKind;
use crate::application_error::{ApplicationErrorKind, application_error};
use crate::service::state::ServiceState;
use crate::session;
use crate::tenant::TenantSelection;
use anyhow::Result;
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) struct SessionCoordinator {
    state: ServiceState,
}

pub(crate) struct SessionAccess {
    home: PathBuf,
    agent: AgentKind,
}

pub(crate) struct DeleteSessionsCommand {
    pub(crate) tenant: String,
    pub(crate) agent: AgentKind,
    pub(crate) ids: Vec<String>,
    pub(crate) all: bool,
    pub(crate) confirmation: String,
}

impl SessionCoordinator {
    pub(crate) fn new(state: ServiceState) -> Self {
        Self { state }
    }

    pub(crate) fn access(
        &self,
        selection: TenantSelection,
        agent: AgentKind,
    ) -> Result<SessionAccess> {
        let tenant = selection.resolve(&self.state.root(), &self.state.host_home())?;
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
        run_blocking(move || access.list()).await
    }

    pub(crate) async fn summary(
        &self,
        selection: TenantSelection,
        agent: AgentKind,
    ) -> Result<session::SessionDiscoverySummary> {
        let access = self.access(selection, agent)?;
        run_blocking(move || access.summary()).await
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
        run_blocking(move || access.evidence(&id, &entry, &snapshot)).await
    }

    pub(crate) async fn delete(&self, command: DeleteSessionsCommand) -> Result<usize> {
        if command.all && command.confirmation != "delete all sessions" {
            return Err(application_error(
                ApplicationErrorKind::InvalidInput,
                "confirmation does not match",
            ));
        }
        let selection = TenantSelection::parse(&command.tenant)?;
        let access = self.access(selection, command.agent)?;
        let guard = self.state.begin_management_mutation()?;
        run_blocking(move || {
            let _guard = guard;
            access.delete(&command.ids, command.all)
        })
        .await
    }
}

impl SessionAccess {
    fn list(&self) -> Result<session::SessionListData> {
        let backend = session::backend_for(self.agent);
        session::list_data(backend.as_ref(), &self.home)
    }

    fn summary(&self) -> Result<session::SessionDiscoverySummary> {
        let backend = session::backend_for(self.agent);
        session::discovery_summary(backend.as_ref(), &self.home)
    }

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
        let backend = session::backend_for(self.agent);
        session::stream_detail_data(backend.as_ref(), &self.home, id, visit_meta, visit_record)
    }

    fn evidence(
        &self,
        id: &str,
        entry: &str,
        snapshot: &str,
    ) -> Result<session::TranscriptEvidence> {
        let backend = session::backend_for(self.agent);
        session::read_evidence(backend.as_ref(), &self.home, id, entry, snapshot)
    }

    fn delete(&self, ids: &[String], all: bool) -> Result<usize> {
        let backend = session::backend_for(self.agent);
        session::delete_sessions(backend.as_ref(), &self.home, ids, all)
    }
}
