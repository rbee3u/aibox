//! Config catalog, editing, Application, and Credential Propagation coordination.

use super::run_blocking;
use crate::agent::AgentKind;
use crate::application_error::{ApplicationErrorKind, application_error};
use crate::config;
use crate::service::state::ServiceState;
use crate::tenant::{Tenant, TenantAgent, TenantSelection};
use anyhow::{Context, Result};

#[derive(Clone)]
pub(crate) struct ConfigCoordinator {
    state: ServiceState,
}

pub(crate) struct ConfigCatalog {
    pub(crate) configs: Vec<config::ConfigCatalogEntry>,
    pub(crate) files: &'static [&'static str],
    pub(crate) application: config::ApplicationStatus,
    pub(crate) credential_propagation_available: bool,
}

pub(crate) struct AuthPropagationPreview {
    pub(crate) plan_id: String,
    pub(crate) preview: config::AuthPropagationPreview,
}

pub(crate) struct ConfigFileView {
    pub(crate) snapshot: config::ConfigFileSnapshot,
    pub(crate) visual: ConfigVisualView,
    pub(crate) warnings: Vec<String>,
    pub(crate) auth: Option<config::CodexAuthInspection>,
    pub(crate) linked: Option<config::ConfigFileSnapshot>,
}

#[derive(Default)]
pub(crate) struct ConfigVisualView {
    pub(crate) state: Option<config::VisualConfigState>,
    pub(crate) error: Option<String>,
}

pub(crate) struct DeleteConfigsCommand {
    pub(crate) selection: TenantSelection,
    pub(crate) agent: AgentKind,
    pub(crate) configs: Vec<String>,
    pub(crate) all: bool,
    pub(crate) confirmation: String,
}

pub(crate) struct DeletedConfigs {
    pub(crate) configs: Vec<String>,
    pub(crate) all: bool,
}

impl ConfigCoordinator {
    pub(crate) fn new(state: ServiceState) -> Self {
        Self { state }
    }

    pub(crate) async fn list(
        &self,
        selection: TenantSelection,
        agent: AgentKind,
    ) -> Result<ConfigCatalog> {
        let check_credential_propagation =
            selection == TenantSelection::Host && agent == AgentKind::Codex;
        let selected = self.resolve_agent(&selection, agent)?;
        let root = self.state.root();
        let host_home = self.state.host_home();
        run_blocking(move || {
            let missing_managed_tenant = match &selected.tenant() {
                Tenant::Managed(tenant) => !tenant.exists()?,
                Tenant::Host { .. } => false,
            };
            if missing_managed_tenant {
                return Ok(ConfigCatalog {
                    configs: Vec::new(),
                    files: selected.agent().config_files(),
                    application: config::ApplicationStatus {
                        last_application: None,
                        drift: config::ConfigDrift::Untracked,
                        detail: None,
                    },
                    credential_propagation_available: false,
                });
            }
            let configs = config::inspect_named_configs(&selected)?;
            let credential_propagation_available = check_credential_propagation
                && config::credential_propagation_source_available(&root, &host_home)?;
            Ok(ConfigCatalog {
                configs,
                files: selected.agent().config_files(),
                application: config::application_status(&selected),
                credential_propagation_available,
            })
        })
        .await
    }

    pub(crate) async fn preview_auth_propagation(&self) -> Result<AuthPropagationPreview> {
        let root = self.state.root();
        let host_home = self.state.host_home();
        let plan_store = self.state.clone();
        run_blocking(move || {
            let plan = config::plan_auth_propagation_from(&root, &host_home)?;
            let preview = config::preview_auth_propagation(&plan);
            let plan_id = uuid::Uuid::now_v7().to_string();
            plan_store.auth_propagation_plan(plan_id.clone(), plan);
            Ok(AuthPropagationPreview { plan_id, preview })
        })
        .await
    }

    pub(crate) async fn execute_auth_propagation(
        &self,
        plan_id: String,
    ) -> Result<config::AuthPropagationReport> {
        let guard = self.state.begin_management_mutation()?;
        let plan_store = self.state.clone();
        run_blocking(move || {
            let _guard = guard;
            let plan = plan_store.take_auth_propagation_plan(&plan_id)?;
            Ok(config::execute_auth_propagation(plan))
        })
        .await
    }

    pub(crate) async fn create(
        &self,
        selection: TenantSelection,
        agent: AgentKind,
        name: config::NamedConfigName,
    ) -> Result<String> {
        let selected = self.resolve_agent(&selection, agent)?;
        let guard = self.state.begin_management_mutation()?;
        run_blocking(move || {
            let _guard = guard;
            config::create_named_config(&selected, &name)?;
            Ok(name.to_string())
        })
        .await
    }

    pub(crate) async fn reveal(
        &self,
        selection: TenantSelection,
        agent: AgentKind,
        target: config::ConfigTarget,
        file: config::ConfigFile,
    ) -> Result<ConfigFileView> {
        let selected = self.resolve_agent(&selection, agent)?;
        run_blocking(move || {
            let snapshot = config::read_config_file_target(&selected, &target, file)?;
            file_view(&selected, &target, file, snapshot, None, true)
        })
        .await
    }

    pub(crate) async fn save(
        &self,
        selection: TenantSelection,
        agent: AgentKind,
        target: config::ConfigTarget,
        file: config::ConfigFile,
        revision: String,
        edit: config::ConfigEdit,
    ) -> Result<ConfigFileView> {
        let selected = self.resolve_agent(&selection, agent)?;
        let guard = self.state.begin_management_mutation()?;
        run_blocking(move || {
            let _guard = guard;
            let saved = config::save_config_file_target(&selected, &target, file, &revision, edit)?;
            file_view(
                &selected,
                &target,
                file,
                saved.snapshot,
                saved.linked,
                false,
            )
        })
        .await
    }

    pub(crate) async fn diagnose(
        &self,
        selection: TenantSelection,
        agent: AgentKind,
        target: config::ConfigTarget,
        file: config::ConfigFile,
        content: Vec<u8>,
    ) -> Result<Vec<config::ConfigDiagnostic>> {
        let selected = self.resolve_agent(&selection, agent)?;
        run_blocking(move || config::diagnose_config_file(&selected, &target, file, &content)).await
    }

    pub(crate) async fn apply(
        &self,
        selection: TenantSelection,
        agent: AgentKind,
        name: config::NamedConfigName,
    ) -> Result<config::ApplicationStatus> {
        let selected = self.resolve_agent(&selection, agent)?;
        let guard = self.state.begin_management_mutation()?;
        run_blocking(move || {
            let _guard = guard;
            config::apply_named_config(&selected, &name)?;
            Ok(config::application_status(&selected))
        })
        .await
    }

    pub(crate) async fn delete(&self, command: DeleteConfigsCommand) -> Result<DeletedConfigs> {
        if command.all && command.confirmation != "delete all configs" {
            return Err(application_error(
                ApplicationErrorKind::InvalidInput,
                "confirmation does not match",
            ));
        }
        let selected = self.resolve_agent(&command.selection, command.agent)?;
        let configs = command
            .configs
            .iter()
            .map(|name| config::NamedConfigName::parse(name))
            .collect::<Result<Vec<_>>>()?;
        let guard = self.state.begin_management_mutation()?;
        run_blocking(move || {
            let _guard = guard;
            config::delete_named_configs(&selected, &configs, command.all)?;
            Ok(DeletedConfigs {
                configs: command.configs,
                all: command.all,
            })
        })
        .await
    }

    fn resolve_agent(&self, selection: &TenantSelection, agent: AgentKind) -> Result<TenantAgent> {
        Ok(selection
            .resolve(&self.state.root(), &self.state.host_home())?
            .for_agent(agent))
    }
}

fn file_view(
    selected: &TenantAgent,
    target: &config::ConfigTarget,
    file: config::ConfigFile,
    snapshot: config::ConfigFileSnapshot,
    linked: Option<config::ConfigFileSnapshot>,
    retain_visual_error: bool,
) -> Result<ConfigFileView> {
    let visual = visual_view(selected, target, file, &snapshot, retain_visual_error);
    let warnings = if target.is_current() {
        Vec::new()
    } else {
        config::config_file_warnings(
            selected,
            target.named().expect("Named Config target"),
            file.as_str(selected.agent()),
            &snapshot.content,
        )
        .unwrap_or_default()
    };
    let auth = named_codex_auth(selected, target, file, &snapshot)?;
    Ok(ConfigFileView {
        snapshot,
        visual,
        warnings,
        auth,
        linked,
    })
}

fn visual_view(
    selected: &TenantAgent,
    target: &config::ConfigTarget,
    file: config::ConfigFile,
    snapshot: &config::ConfigFileSnapshot,
    retain_error: bool,
) -> ConfigVisualView {
    if target.is_current() || file != config::ConfigFile::Main {
        return ConfigVisualView::default();
    }
    let result = std::str::from_utf8(&snapshot.content)
        .map_err(|_| anyhow::anyhow!("configuration is not valid UTF-8"))
        .and_then(|text| {
            config::visual_config_state(
                selected,
                target.named().expect("Named Config target"),
                text,
            )
        });
    match result {
        Ok(state) => ConfigVisualView {
            state: Some(state),
            error: None,
        },
        Err(error) if retain_error => ConfigVisualView {
            state: None,
            error: Some(format!("{error:#}")),
        },
        Err(_) => ConfigVisualView::default(),
    }
}

fn named_codex_auth(
    selected: &TenantAgent,
    target: &config::ConfigTarget,
    file: config::ConfigFile,
    snapshot: &config::ConfigFileSnapshot,
) -> Result<Option<config::CodexAuthInspection>> {
    if target.is_current() || selected.agent() != AgentKind::Codex {
        return Ok(None);
    }
    let auth_snapshot = if file == config::ConfigFile::Auth {
        snapshot.clone()
    } else {
        config::read_config_file_target(selected, target, config::ConfigFile::Auth)?
    };
    if !auth_snapshot.exists {
        return Ok(None);
    }
    let text = std::str::from_utf8(&auth_snapshot.content)
        .context("Named Config auth.json is not valid UTF-8")?;
    Ok(config::inspect_named_codex_auth(
        selected,
        target.named().expect("Named Config target"),
        text,
    )
    .ok())
}
