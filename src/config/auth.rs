//! One-shot Codex Credential Propagation across existing Configs.
//!
//! Planning validates and snapshots the Host source plus the complete
//! structural view of existing candidate Configs before any write. Unsafe
//! filesystem structure aborts the plan, while malformed credential content is
//! retained as a reportable target outcome.
//!
//! Execution consumes those snapshots in stable target order. Each selected
//! `auth.json` is replaced independently and atomically; a failed replacement
//! does not roll back earlier writes or prevent later attempts. Propagation
//! creates no Configs and retains no synchronization state.

use super::{
    MAX_CONFIG_BYTES, capture_optional_agent_file, inspect_named_config_directory,
    replace_existing_atomic, validate_private_directory, validate_private_file,
};
use crate::agent::AgentKind;
use crate::foundation::safe_fs::FileSnapshot;
use crate::tenant::{self, ManagedTenant, TENANTS_DIR, Tenant, TenantAgent};
use anyhow::{Context, Result, bail};
use serde::Serialize;
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "status", rename_all = "kebab-case")]
pub(crate) enum PropagationOutcome {
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
pub(crate) struct AuthPropagationPlan {
    source_content: Vec<u8>,
    targets: Vec<PlannedTarget>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct PropagationEntry {
    pub(crate) label: String,
    pub(crate) outcome: PropagationOutcome,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct AuthPropagationReport {
    pub(crate) entries: Vec<PropagationEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct PropagationPreviewEntry {
    pub(crate) label: String,
    pub(crate) outcome: PropagationOutcome,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct AuthPropagationPreview {
    pub(crate) entries: Vec<PropagationPreviewEntry>,
    pub(crate) updates: usize,
}

pub(crate) fn credential_propagation_source_available(
    root: &Path,
    host_home: &Path,
) -> Result<bool> {
    if !crate::foundation::safe_fs::real_dir_exists(host_home, "Host Home")? {
        return Ok(false);
    }
    let (_, _, source) = capture_host_source(root, host_home)?;
    Ok(source.present
        && matches!(
            classify_auth(&source.content, None),
            AuthContent::ChatGpt(_)
        ))
}

pub(crate) fn plan_auth_propagation_from(
    root: &Path,
    host_home: &Path,
) -> Result<AuthPropagationPlan> {
    let (host, source_file, source) = capture_host_source(root, host_home)?;
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

    plan_auth_propagation(root, &host, source_file, source, source_credentials)
}

fn capture_host_source(
    root: &Path,
    host_home: &Path,
) -> Result<(TenantAgent, &'static str, FileSnapshot)> {
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
    Ok((host, source_file, source))
}

fn plan_auth_propagation(
    root: &Path,
    host: &TenantAgent,
    source_file: &str,
    source: FileSnapshot,
    source_credentials: ChatGptCredentials,
) -> Result<AuthPropagationPlan> {
    let mut candidates = Vec::new();
    discover_named_auth_candidates(host, "host", &mut candidates)?;
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

pub(crate) fn execute_auth_propagation(plan: AuthPropagationPlan) -> AuthPropagationReport {
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

pub(crate) fn preview_auth_propagation(plan: &AuthPropagationPlan) -> AuthPropagationPreview {
    let entries = plan
        .targets
        .iter()
        .map(|target| PropagationPreviewEntry {
            label: target.label.clone(),
            outcome: match &target.action {
                PlannedAction::Write { .. } => PropagationOutcome::Updated,
                PlannedAction::Report(outcome) => outcome.clone(),
            },
        })
        .collect::<Vec<_>>();
    let updates = entries
        .iter()
        .filter(|entry| entry.outcome == PropagationOutcome::Updated)
        .count();
    AuthPropagationPreview { entries, updates }
}

fn discover_managed_tenant_names(root: &Path) -> Result<Vec<String>> {
    let collection = root.join(TENANTS_DIR);
    if !crate::foundation::safe_fs::real_dir_exists(&collection, "Tenant collection")? {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{create_named_config, ensure_named_config_directory};
    use serde_json::json;

    fn credential(account: &str, refreshed: &str, marker: &str) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "auth_mode": "chatgpt",
            "tokens": { "account_id": account },
            "last_refresh": refreshed,
            "marker": marker,
        }))
        .unwrap()
    }

    fn host_agent(root: &Path, host_home: &Path) -> TenantAgent {
        let selected = Tenant::Host {
            home_dir: host_home.to_path_buf(),
            root_dir: root.to_path_buf(),
        }
        .for_agent(AgentKind::Codex);
        selected.ensure_agent_state_dir().unwrap();
        selected
    }

    fn set_named_auth(selected: &TenantAgent, name: &str, content: &[u8]) {
        create_named_config(selected, name).unwrap();
        fs::write(selected.named_config_file(name, "auth.json"), content).unwrap();
    }

    #[test]
    fn planning_classifies_candidates_and_orders_existing_complete_targets() {
        let root = tempfile::tempdir().unwrap();
        let host_home = tempfile::tempdir().unwrap();
        let host = host_agent(root.path(), host_home.path());
        let source = credential("same", "2026-08-08T00:00:00Z", "source");
        fs::write(host.state_file("auth.json"), &source).unwrap();

        set_named_auth(
            &host,
            "older",
            &credential("same", "2026-08-01T00:00:00Z", "old"),
        );
        set_named_auth(&host, "unchanged", &source);
        set_named_auth(
            &host,
            "conflict",
            &credential("same", "2026-08-08T00:00:00Z", "different"),
        );
        set_named_auth(
            &host,
            "newer",
            &credential("same", "2026-08-09T00:00:00Z", "newer"),
        );
        set_named_auth(&host, "invalid", br#"{"auth_mode":"chatgpt"}"#);
        set_named_auth(
            &host,
            "other-account",
            &credential("other", "2026-08-01T00:00:00Z", "other"),
        );
        set_named_auth(&host, "api-key", br#"{"auth_mode":"apikey"}"#);
        ensure_named_config_directory(&host, "incomplete").unwrap();

        let managed = ManagedTenant::resolve(root.path(), "work").unwrap();
        managed.ensure_initialized().unwrap();
        let managed = managed.for_agent(AgentKind::Codex);
        fs::write(
            managed.state_file("auth.json"),
            credential("same", "2026-08-02T00:00:00Z", "managed"),
        )
        .unwrap();

        let preview = preview_auth_propagation(
            &plan_auth_propagation_from(root.path(), host_home.path()).unwrap(),
        );
        let labels = preview
            .entries
            .iter()
            .map(|entry| entry.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            [
                "host/config/conflict",
                "host/config/invalid",
                "host/config/newer",
                "host/config/older",
                "host/config/unchanged",
                "tenant/work/current",
            ]
        );
        assert_eq!(preview.updates, 2);
        assert!(matches!(
            preview.entries[0].outcome,
            PropagationOutcome::Conflict { .. }
        ));
        assert!(matches!(
            preview.entries[1].outcome,
            PropagationOutcome::Invalid { .. }
        ));
        assert!(matches!(
            preview.entries[2].outcome,
            PropagationOutcome::Newer { .. }
        ));
        assert_eq!(preview.entries[3].outcome, PropagationOutcome::Updated);
        assert_eq!(preview.entries[4].outcome, PropagationOutcome::Unchanged);
        assert_eq!(preview.entries[5].outcome, PropagationOutcome::Updated);
        assert!(!host.named_config_file("incomplete", "auth.json").exists());
    }

    #[test]
    fn unsafe_structural_preflight_aborts_before_any_target_write() {
        let root = tempfile::tempdir().unwrap();
        let host_home = tempfile::tempdir().unwrap();
        let host = host_agent(root.path(), host_home.path());
        fs::write(
            host.state_file("auth.json"),
            credential("same", "2026-08-08T00:00:00Z", "source"),
        )
        .unwrap();
        let managed = ManagedTenant::resolve(root.path(), "work").unwrap();
        managed.ensure_initialized().unwrap();
        let target = managed.for_agent(AgentKind::Codex).state_file("auth.json");
        let original = credential("same", "2026-08-01T00:00:00Z", "target");
        fs::write(&target, &original).unwrap();
        fs::write(
            root.path().join("tenants/unsafe"),
            b"not a Tenant directory",
        )
        .unwrap();

        let error = plan_auth_propagation_from(root.path(), host_home.path()).unwrap_err();
        assert!(format!("{error:#}").contains("Managed Tenant entry is not a real directory"));
        assert_eq!(fs::read(target).unwrap(), original);
    }

    #[test]
    fn execution_uses_the_snapshot_continues_after_failure_and_preserves_modes() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("missing/auth.json");
        let target = root.path().join("target.json");
        fs::write(&target, b"old").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        }
        let snapshot = credential("same", "2026-08-08T00:00:00Z", "snapshot");
        let plan = AuthPropagationPlan {
            source_content: snapshot.clone(),
            targets: vec![
                PlannedTarget {
                    label: "first".to_string(),
                    action: PlannedAction::Write {
                        path: missing,
                        mode: 0o600,
                    },
                },
                PlannedTarget {
                    label: "second".to_string(),
                    action: PlannedAction::Write {
                        path: target.clone(),
                        mode: 0o640,
                    },
                },
            ],
        };

        let report = execute_auth_propagation(plan);
        assert!(matches!(
            report.entries[0].outcome,
            PropagationOutcome::Failed { .. }
        ));
        assert_eq!(report.entries[1].outcome, PropagationOutcome::Updated);
        assert_eq!(fs::read(&target).unwrap(), snapshot);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(target).unwrap().permissions().mode() & 0o777,
                0o640
            );
        }
    }
}
