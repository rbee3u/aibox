//! One-shot Codex Credential Propagation across existing Configs.

use super::{
    MAX_CONFIG_BYTES, capture_optional_agent_file, inspect_named_config_directory,
    replace_existing_atomic, validate_private_directory, validate_private_file,
};
use crate::agent::AgentKind;
use crate::tenant::{self, FileSnapshot, ManagedTenant, TENANTS_DIR, Tenant, TenantAgent};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[derive(Clone, Debug, Eq, PartialEq)]
struct ChatGptCredentials {
    account_id: String,
    last_refresh: OffsetDateTime,
    last_refresh_text: String,
    value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AuthContent {
    Other,
    Invalid(String),
    ChatGpt(ChatGptCredentials),
}

#[derive(Clone, Debug)]
struct AuthCandidate {
    label: String,
    path: PathBuf,
    content: Vec<u8>,
    mode: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PropagationOutcome {
    Updated,
    Unchanged,
    Conflict {
        last_refresh: String,
    },
    Newer {
        target_last_refresh: String,
        source_last_refresh: String,
    },
    Invalid {
        reason: String,
    },
    Failed {
        reason: String,
    },
}

#[derive(Clone, Debug)]
enum PlannedAction {
    Write { path: PathBuf, mode: u32 },
    Report(PropagationOutcome),
}

#[derive(Clone, Debug)]
struct PlannedTarget {
    label: String,
    action: PlannedAction,
}

#[derive(Clone, Debug)]
pub(super) struct AuthPropagationPlan {
    source_content: Vec<u8>,
    targets: Vec<PlannedTarget>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PropagationEntry {
    pub(super) label: String,
    pub(super) outcome: PropagationOutcome,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct PropagationCounts {
    pub(super) updated: usize,
    pub(super) unchanged: usize,
    pub(super) conflicts: usize,
    pub(super) newer: usize,
    pub(super) invalid: usize,
    pub(super) failed: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct AuthPropagationReport {
    pub(super) entries: Vec<PropagationEntry>,
}

pub(super) fn propagate_auth_from(root: &Path, host_home: &Path) -> Result<i32> {
    let plan = plan_auth_propagation_from(root, host_home)?;
    let report = execute_auth_propagation(plan);
    report.print()?;
    Ok(i32::from(report.counts().failed > 0))
}

pub(super) fn plan_auth_propagation_from(
    root: &Path,
    host_home: &Path,
) -> Result<AuthPropagationPlan> {
    let host = Tenant::Host {
        home_dir: host_home.to_path_buf(),
        root_dir: root.to_path_buf(),
    }
    .for_agent(AgentKind::Codex);
    let source_file = host
        .agent
        .native_auth_file()
        .expect("Codex has a native auth file");
    let source = capture_optional_agent_file(&host, source_file)?;
    if !source.present {
        bail!(
            "Host Codex Current Config {source_file} does not exist: {}",
            host.state_file(source_file).display()
        );
    }
    let source_credentials = match classify_auth(&source.content, None) {
        AuthContent::ChatGpt(credentials) => credentials,
        AuthContent::Other => {
            bail!("Host Codex Current Config auth.json is not ChatGPT Credentials")
        }
        AuthContent::Invalid(reason) => {
            bail!("Host Codex Current Config auth.json is invalid: {reason}")
        }
    };

    let mut candidates = Vec::new();
    discover_named_auth_candidates(&host, "host", &mut candidates)?;
    for tenant_name in discover_managed_tenant_names(root)? {
        let managed = ManagedTenant::resolve(root, &tenant_name)?;
        let selected = managed.for_agent(AgentKind::Codex);
        let current = capture_optional_agent_file(&selected, source_file)?;
        if current.present {
            candidates.push(AuthCandidate {
                label: format!("tenant/{tenant_name}/current"),
                path: selected.state_file(source_file),
                content: current.content,
                mode: current.mode.unwrap_or(0o600),
            });
        }
        discover_named_auth_candidates(
            &selected,
            &format!("tenant/{tenant_name}"),
            &mut candidates,
        )?;
    }
    candidates.sort_by(|left, right| left.label.cmp(&right.label));

    let mut targets = Vec::new();
    for candidate in candidates {
        let action = match classify_auth(
            &candidate.content,
            Some(source_credentials.account_id.as_str()),
        ) {
            AuthContent::Other => continue,
            AuthContent::Invalid(reason) => {
                PlannedAction::Report(PropagationOutcome::Invalid { reason })
            }
            AuthContent::ChatGpt(target) => {
                debug_assert_eq!(target.account_id, source_credentials.account_id);
                match target.last_refresh.cmp(&source_credentials.last_refresh) {
                    Ordering::Less => PlannedAction::Write {
                        path: candidate.path,
                        mode: candidate.mode,
                    },
                    Ordering::Equal if target.value == source_credentials.value => {
                        PlannedAction::Report(PropagationOutcome::Unchanged)
                    }
                    Ordering::Equal => PlannedAction::Report(PropagationOutcome::Conflict {
                        last_refresh: target.last_refresh_text,
                    }),
                    Ordering::Greater => PlannedAction::Report(PropagationOutcome::Newer {
                        target_last_refresh: target.last_refresh_text,
                        source_last_refresh: source_credentials.last_refresh_text.clone(),
                    }),
                }
            }
        };
        targets.push(PlannedTarget {
            label: candidate.label,
            action,
        });
    }

    Ok(AuthPropagationPlan {
        source_content: source.content,
        targets,
    })
}

pub(super) fn execute_auth_propagation(plan: AuthPropagationPlan) -> AuthPropagationReport {
    let mut entries = Vec::with_capacity(plan.targets.len());
    for target in plan.targets {
        let outcome = match target.action {
            PlannedAction::Report(outcome) => outcome,
            PlannedAction::Write { path, mode } => {
                match replace_existing_atomic(&path, &plan.source_content, mode) {
                    Ok(()) => PropagationOutcome::Updated,
                    Err(error) => PropagationOutcome::Failed {
                        reason: format!("{error:#}"),
                    },
                }
            }
        };
        entries.push(PropagationEntry {
            label: target.label,
            outcome,
        });
    }
    AuthPropagationReport { entries }
}

fn discover_managed_tenant_names(root: &Path) -> Result<Vec<String>> {
    let collection = root.join(TENANTS_DIR);
    if !tenant::real_dir_exists(&collection, "Tenant collection")? {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in
        fs::read_dir(&collection).with_context(|| format!("read {}", collection.display()))?
    {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !tenant::is_safe_name(&name) {
            continue;
        }
        let kind = entry.file_type()?;
        if !kind.is_dir() || kind.is_symlink() {
            bail!(
                "Managed Tenant entry is not a real directory: {}",
                entry.path().display()
            );
        }
        names.push(name);
    }
    names.sort();
    Ok(names)
}

fn discover_named_auth_candidates(
    selected: &TenantAgent,
    scope_label: &str,
    candidates: &mut Vec<AuthCandidate>,
) -> Result<()> {
    if !selected.named_config_catalog_exists()? {
        return Ok(());
    }
    let catalog = selected.named_config_catalog_dir();
    if let Some(collection) = catalog.parent() {
        validate_private_directory(collection)?;
    }
    validate_private_directory(catalog)?;

    for entry in fs::read_dir(catalog).with_context(|| format!("read {}", catalog.display()))? {
        let entry = entry?;
        let Some(config) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if tenant::validate_name("config", &config).is_err() {
            continue;
        }
        let kind = entry.file_type()?;
        if !kind.is_dir() || kind.is_symlink() {
            bail!(
                "Named Config directory is not a real directory: {}",
                entry.path().display()
            );
        }
        let layout = inspect_named_config_directory(selected, &config)?
            .expect("discovered Named Config directory exists");
        validate_private_directory(&selected.named_config_dir(&config))?;
        if layout.main {
            validate_private_file(
                &selected.named_config_file(&config, selected.agent.main_config_file()),
            )?;
        }
        if let Some(auth_file) = selected.agent.native_auth_file()
            && layout.auth
        {
            validate_private_file(&selected.named_config_file(&config, auth_file))?;
        }
        if !layout.complete(selected) {
            continue;
        }
        let auth_file = selected
            .agent
            .native_auth_file()
            .expect("Codex Named Config has a native auth file");
        let path = selected.named_config_file(&config, auth_file);
        let snapshot = FileSnapshot::capture_with_limit(&path, MAX_CONFIG_BYTES)?;
        debug_assert!(snapshot.present);
        candidates.push(AuthCandidate {
            label: format!("{scope_label}/config/{config}"),
            path,
            content: snapshot.content,
            mode: 0o600,
        });
    }
    Ok(())
}

fn classify_auth(content: &[u8], expected_account_id: Option<&str>) -> AuthContent {
    let value = match serde_json::from_slice::<Value>(content) {
        Ok(value) => value,
        Err(error) => return AuthContent::Invalid(format!("invalid JSON: {error}")),
    };
    let Some(object) = value.as_object() else {
        return AuthContent::Invalid("JSON value is not an object".to_string());
    };
    match object.get("auth_mode") {
        None => return AuthContent::Other,
        Some(Value::String(mode)) if mode != "chatgpt" => return AuthContent::Other,
        Some(Value::String(_)) => {}
        Some(_) => return AuthContent::Invalid("auth_mode is not a string".to_string()),
    }
    let Some(account_id) = object
        .get("tokens")
        .and_then(Value::as_object)
        .and_then(|tokens| tokens.get("account_id"))
        .and_then(Value::as_str)
        .filter(|account_id| !account_id.trim().is_empty())
    else {
        return AuthContent::Invalid(
            "chatgpt credentials require a non-empty tokens.account_id".to_string(),
        );
    };
    if expected_account_id.is_some_and(|expected| account_id != expected) {
        return AuthContent::Other;
    }
    let Some(last_refresh_text) = object.get("last_refresh").and_then(Value::as_str) else {
        return AuthContent::Invalid(
            "chatgpt credentials require a string last_refresh".to_string(),
        );
    };
    let last_refresh = match OffsetDateTime::parse(last_refresh_text, &Rfc3339) {
        Ok(last_refresh) => last_refresh,
        Err(error) => {
            return AuthContent::Invalid(format!(
                "chatgpt credentials have invalid last_refresh: {error}"
            ));
        }
    };
    AuthContent::ChatGpt(ChatGptCredentials {
        account_id: account_id.to_string(),
        last_refresh,
        last_refresh_text: last_refresh_text.to_string(),
        value,
    })
}

impl AuthPropagationReport {
    pub(super) fn counts(&self) -> PropagationCounts {
        let mut counts = PropagationCounts::default();
        for entry in &self.entries {
            match entry.outcome {
                PropagationOutcome::Updated => counts.updated += 1,
                PropagationOutcome::Unchanged => counts.unchanged += 1,
                PropagationOutcome::Conflict { .. } => counts.conflicts += 1,
                PropagationOutcome::Newer { .. } => counts.newer += 1,
                PropagationOutcome::Invalid { .. } => counts.invalid += 1,
                PropagationOutcome::Failed { .. } => counts.failed += 1,
            }
        }
        counts
    }

    fn print(&self) -> Result<()> {
        let mut stdout_open = true;
        for entry in &self.entries {
            match &entry.outcome {
                PropagationOutcome::Updated => {
                    if stdout_open {
                        stdout_open = crate::print_line(&format!("updated {}", entry.label))?;
                    }
                }
                PropagationOutcome::Unchanged => {
                    if stdout_open {
                        stdout_open = crate::print_line(&format!("unchanged {}", entry.label))?;
                    }
                }
                PropagationOutcome::Conflict { last_refresh } => {
                    if stdout_open {
                        stdout_open = crate::print_line(&format!(
                            "conflict {}: same last_refresh {last_refresh} but JSON values differ",
                            entry.label
                        ))?;
                    }
                }
                PropagationOutcome::Newer {
                    target_last_refresh,
                    source_last_refresh,
                } => {
                    if stdout_open {
                        stdout_open = crate::print_line(&format!(
                            "skipped {}: target last_refresh {target_last_refresh} is newer than source {source_last_refresh}",
                            entry.label
                        ))?;
                    }
                }
                PropagationOutcome::Invalid { reason } => {
                    eprintln!("warning {}: {reason}", entry.label);
                }
                PropagationOutcome::Failed { reason } => {
                    eprintln!("failed {}: {reason}", entry.label);
                }
            }
        }
        let counts = self.counts();
        if stdout_open {
            crate::print_line(&format!(
                "summary: updated={} unchanged={} conflicts={} newer={} invalid={} failed={}",
                counts.updated,
                counts.unchanged,
                counts.conflicts,
                counts.newer,
                counts.invalid,
                counts.failed
            ))?;
        }
        Ok(())
    }
}
