//! Tenant-local Provider catalogs, activation state, reconciliation, and
//! replayable multi-file transactions.

use crate::agent::AgentKind;
use crate::agent_config::{
    self, Change, ChangeClass, ConflictChoice, Pointer, ProviderDefinition, PROVIDER_METADATA_FILE,
};
use crate::cli::{ProviderCommand, ReconcileArgs};
use crate::tenant::{self, FileSnapshot, Tenant, TenantAgent};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;
const MAX_STATE_BYTES: u64 = 256 * 1024 * 1024;

const DEFAULT_CODEX_CONFIG: &str = r#"approval_policy = "never"
sandbox_mode = "danger-full-access"
model_reasoning_effort = "xhigh"
plan_mode_reasoning_effort = "xhigh"
model = "gpt-5.6-sol"
model_provider = "custom"

[model_providers.custom]
base_url = "https://example.com/v1"
requires_openai_auth = true
"#;

const DEFAULT_CLAUDE_SETTINGS: &str = r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://example.com",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4-5",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-5[1m]",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-5[1m]",
    "ANTHROPIC_DEFAULT_FABLE_MODEL": "claude-fable-5[1m]"
  },
  "permissions": {
    "defaultMode": "bypassPermissions"
  },
  "skipDangerousModePermissionPrompt": true
}
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActiveProviderState {
    provider: String,
    base: BTreeMap<String, FileSnapshot>,
    applied: ProviderDefinition,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ScopeMetadata {
    active_provider: Option<ActiveProviderState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending: Option<PendingTransaction>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PendingTransaction {
    changes: Vec<PendingChange>,
    active_provider: Option<ActiveProviderState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum PendingChange {
    AgentFile {
        file: String,
        snapshot: FileSnapshot,
    },
    ProviderDirectory {
        provider: String,
        present: bool,
    },
    ProviderFile {
        provider: String,
        file: String,
        snapshot: FileSnapshot,
    },
}

struct Analysis {
    state: ActiveProviderState,
    source: ProviderDefinition,
    base_tree: serde_json::Value,
    working: ProviderDefinition,
    changes: Vec<Change>,
}

/// Execute one parsed Provider command.
pub fn dispatch(selected: &TenantAgent, command: &ProviderCommand) -> Result<i32> {
    recover_pending(selected)?;
    match command {
        ProviderCommand::List => {
            for provider in list_providers(selected)? {
                if !crate::print_line(&provider)? {
                    break;
                }
            }
        }
        ProviderCommand::Get { provider, auth } => {
            crate::print_text(&get_provider(selected, provider, *auth)?)?;
        }
        ProviderCommand::Create { provider } => create_provider(selected, provider)?,
        ProviderCommand::Edit { provider, auth } => edit_provider(selected, provider, *auth)?,
        ProviderCommand::Delete {
            providers,
            all,
            yes,
        } => delete_providers(selected, providers, *all, *yes)?,
        ProviderCommand::Activate {
            provider,
            discard_config_changes,
        } => activate_provider(selected, provider, *discard_config_changes)?,
        ProviderCommand::Deactivate {
            discard_config_changes,
        } => deactivate_provider(selected, *discard_config_changes)?,
        ProviderCommand::Status => print_status(selected)?,
        ProviderCommand::Diff => print_diff(selected)?,
        ProviderCommand::Reconcile(args) => reconcile_provider(selected, args)?,
    }
    Ok(0)
}

/// Create a Tenant-local Provider using the selected Agent's default template.
pub fn create_provider(selected: &TenantAgent, provider: &str) -> Result<()> {
    tenant::validate_name("provider", provider)?;
    selected.ensure_for_management()?;
    recover_pending(selected)?;
    if provider_exists(selected, provider)? {
        read_provider_definition(selected, provider)?;
        return Ok(());
    }
    let main = match selected.agent {
        AgentKind::Codex => DEFAULT_CODEX_CONFIG,
        AgentKind::Claude => DEFAULT_CLAUDE_SETTINGS,
    };
    let changes = vec![
        PendingChange::ProviderDirectory {
            provider: provider.to_string(),
            present: true,
        },
        provider_file_change(provider, selected.agent.main_config_file(), main, 0o644),
        provider_file_change(provider, selected.agent.provider_auth_file(), "{}\n", 0o600),
        provider_file_change(
            provider,
            PROVIDER_METADATA_FILE,
            "{\n  \"tombstones\": []\n}\n",
            0o644,
        ),
    ];
    commit_transaction(selected, changes, read_active_state(selected)?)
}

/// List Provider names in the selected Tenant-local catalog.
pub fn list_providers(selected: &TenantAgent) -> Result<Vec<String>> {
    if !selected.metadata_dir_exists()? {
        return Ok(Vec::new());
    }
    let root = selected.metadata_dir();
    let mut providers = Vec::new();
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if tenant::validate_name("provider", &name).is_ok()
            && provider_files_are_regular(selected, &name)
        {
            providers.push(name);
        }
    }
    providers.sort();
    Ok(providers)
}

/// Print a Provider's main configuration or explicit credential file.
pub fn get_provider(selected: &TenantAgent, provider: &str, auth: bool) -> Result<String> {
    ensure_provider_exists(selected, provider)?;
    let file_name = if auth {
        selected.agent.provider_auth_file()
    } else {
        selected.agent.main_config_file()
    };
    let path = selected.provider_file(provider, file_name);
    if auth {
        validate_private_file(&path)?;
    }
    read_regular_string(&path)
}

/// Edit a Provider source file without changing the working Agent
/// Configuration.
pub fn edit_provider(selected: &TenantAgent, provider: &str, auth: bool) -> Result<()> {
    ensure_provider_exists(selected, provider)?;
    let file_name = if auth {
        selected.agent.provider_auth_file()
    } else {
        selected.agent.main_config_file()
    };
    let path = selected.provider_file(provider, file_name);
    if !auth {
        validate_private_file(
            &selected.provider_file(provider, selected.agent.provider_auth_file()),
        )?;
    }
    let current = read_regular_bytes(&path)?;
    let temp = sibling_temp_path(&path, "edit")?;
    write_new_file(&temp, &current, if auth { 0o600 } else { 0o644 })?;
    let editor = configured_editor();
    let status = editor_command(&editor)?
        .arg(&temp)
        .status()
        .with_context(|| format!("run editor {editor:?}"));
    let result = match status {
        Ok(status) if status.success() => {
            let edited = read_regular_string(&temp)?;
            let main = if auth {
                read_regular_string(
                    &selected.provider_file(provider, selected.agent.main_config_file()),
                )?
            } else {
                edited.clone()
            };
            let auth_content = if auth {
                edited
            } else {
                read_regular_string(
                    &selected.provider_file(provider, selected.agent.provider_auth_file()),
                )?
            };
            let metadata =
                read_regular_string(&selected.provider_file(provider, PROVIDER_METADATA_FILE))?;
            ProviderDefinition::parse(selected.agent, &main, &auth_content, Some(&metadata))?;
            let content = read_regular_bytes(&temp)?;
            let mode = if auth {
                0o600
            } else {
                existing_mode(&path)?.unwrap_or(0o644)
            };
            commit_transaction(
                selected,
                vec![PendingChange::ProviderFile {
                    provider: provider.to_string(),
                    file: file_name.to_string(),
                    snapshot: FileSnapshot {
                        present: true,
                        content,
                        mode: Some(mode),
                    },
                }],
                read_active_state(selected)?,
            )
        }
        Ok(status) => bail!("editor exited with status {status}"),
        Err(error) => Err(error),
    };
    let _ = fs::remove_file(&temp);
    result
}

/// Delete selected inactive Providers, or all when explicitly requested.
pub fn delete_providers(
    selected: &TenantAgent,
    providers: &[String],
    all: bool,
    yes: bool,
) -> Result<()> {
    if all && !providers.is_empty() {
        bail!("--all cannot be combined with Provider names");
    }
    if !all && providers.is_empty() {
        bail!("provide at least one Provider name or use --all");
    }
    let active = read_active_state(selected)?;
    let targets = if all {
        list_providers(selected)?
            .into_iter()
            .filter(|provider| {
                active
                    .as_ref()
                    .is_none_or(|state| state.provider != *provider)
            })
            .collect()
    } else {
        let mut unique = Vec::new();
        for provider in providers {
            tenant::validate_name("provider", provider)?;
            if active
                .as_ref()
                .is_some_and(|state| state.provider == *provider)
            {
                bail!(
                    "Provider '{}' is active; deactivate it before deletion",
                    provider
                );
            }
            if provider_exists(selected, provider)? && !unique.contains(provider) {
                unique.push(provider.clone());
            }
        }
        unique
    };
    if targets.is_empty() {
        eprintln!(">> no inactive Providers in this Tenant and Coding Agent");
        return Ok(());
    }
    if !yes {
        for provider in &targets {
            if !confirm_delete(provider)? {
                bail!("aborted");
            }
        }
    }
    let changes = targets
        .into_iter()
        .map(|provider| PendingChange::ProviderDirectory {
            provider,
            present: false,
        })
        .collect();
    commit_transaction(selected, changes, active)
}

/// Activate a Provider by materializing it into native Agent Configuration.
pub fn activate_provider(
    selected: &TenantAgent,
    provider: &str,
    discard_config_changes: bool,
) -> Result<()> {
    ensure_provider_exists(selected, provider)?;
    selected.ensure_for_management()?;
    selected.ensure_agent_dir()?;
    let source = read_provider_definition(selected, provider)?;
    let current = capture_agent_files(selected)?;
    let previous = read_active_state(selected)?;
    let base = if let Some(state) = &previous {
        let keys = auth_keys(&state.applied, &source);
        let base_tree = effective_from_snapshots(selected.agent, &state.base, &keys)?;
        let expected = agent_config::materialize(&base_tree, &state.applied)?;
        let working = effective_from_snapshots(selected.agent, &current, &keys)?;
        if working != expected && !discard_config_changes {
            bail!(
                "Agent Configuration has working changes; reconcile them or use --discard-config-changes"
            );
        }
        state.base.clone()
    } else {
        current.clone()
    };

    let base_tree = effective_from_snapshots(selected.agent, &base, &source.auth_keys())?;
    let desired_tree = agent_config::materialize(&base_tree, &source)?;
    let desired = snapshots_from_effective(selected, &desired_tree, &current, &base, &source)?;
    let state = ActiveProviderState {
        provider: provider.to_string(),
        base,
        applied: source,
    };
    commit_transaction(selected, agent_file_changes(&desired), Some(state))
}

/// Deactivate the current Provider and restore the exact pre-activation base.
pub fn deactivate_provider(selected: &TenantAgent, discard_config_changes: bool) -> Result<()> {
    let Some(state) = read_active_state(selected)? else {
        return Ok(());
    };
    selected.ensure_agent_dir()?;
    let current = capture_agent_files(selected)?;
    let expected = expected_tree(selected, &state, &state.applied)?;
    let working = effective_from_snapshots(selected.agent, &current, &state.applied.auth_keys())?;
    if working != expected && !discard_config_changes {
        bail!(
            "Agent Configuration has working changes; reconcile them or use --discard-config-changes"
        );
    }
    commit_transaction(selected, agent_file_changes(&state.base), None)
}

/// Return whether the Active Provider has source or working divergence.
pub(crate) fn has_divergence(selected: &TenantAgent) -> Result<bool> {
    let Some(analysis) = analyze(selected)? else {
        return Ok(false);
    };
    Ok(!analysis.changes.is_empty())
}

fn print_status(selected: &TenantAgent) -> Result<()> {
    let Some(analysis) = analyze(selected)? else {
        crate::print_line("inactive")?;
        return Ok(());
    };
    crate::print_line(&format!("active {}", analysis.state.provider))?;
    if analysis.changes.is_empty() {
        crate::print_line("clean")?;
    } else {
        for change in analysis.changes {
            if !crate::print_line(&format!("{} {}", change.class, change.path))? {
                break;
            }
        }
    }
    Ok(())
}

fn print_diff(selected: &TenantAgent) -> Result<()> {
    let Some(analysis) = analyze(selected)? else {
        bail!("no Active Provider");
    };
    let working = agent_config::diff(&analysis.state.applied, &analysis.working);
    let source = agent_config::diff(&analysis.state.applied, &analysis.source);
    if working.is_empty() && source.is_empty() {
        crate::print_line("clean")?;
        return Ok(());
    }
    for (side, entries) in [("working", working), ("source", source)] {
        for entry in entries {
            let (old, new) = if entry.path.is_auth() {
                ("<redacted>".to_string(), "<redacted>".to_string())
            } else {
                (
                    agent_config::display_node(entry.old.as_ref()),
                    agent_config::display_node(entry.new.as_ref()),
                )
            };
            if !crate::print_line(&format!("{side} {}: {old} -> {new}", entry.path))? {
                return Ok(());
            }
        }
    }
    Ok(())
}

/// Reconcile source and working changes with a three-way merge.
pub fn reconcile_provider(selected: &TenantAgent, args: &ReconcileArgs) -> Result<()> {
    let analysis = analyze_unlocked(selected)?.context("no Active Provider")?;
    if analysis.changes.is_empty() {
        eprintln!(">> Provider and Agent Configuration are clean");
        return Ok(());
    }
    let mut resolutions = explicit_resolutions(args)?;
    for change in &analysis.changes {
        if change.class != ChangeClass::Conflict {
            continue;
        }
        let all_choice = match (args.take_provider_all, args.take_config_all) {
            (true, false) => Some(ConflictChoice::Provider),
            (false, true) => Some(ConflictChoice::Config),
            _ => None,
        };
        if let Some(choice) = all_choice {
            resolutions.entry(change.path.clone()).or_insert(choice);
        }
    }
    let result = agent_config::reconcile(
        &analysis.state.applied,
        &analysis.working,
        &analysis.source,
        &resolutions,
    )?;
    let unresolved: Vec<_> = result
        .changes
        .iter()
        .filter(|change| {
            change.class == ChangeClass::Conflict && !resolutions.contains_key(&change.path)
        })
        .map(|change| change.path.to_string())
        .collect();
    if !unresolved.is_empty() {
        bail!(
            "unresolved Provider conflicts:\n{}\nuse --take-provider or --take-config for each path",
            unresolved
                .iter()
                .map(|path| format!("  {path}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    let current = capture_agent_files(selected)?;
    let desired_tree = agent_config::materialize(&analysis.base_tree, &result.merged)?;
    let desired = snapshots_from_effective(
        selected,
        &desired_tree,
        &current,
        &analysis.state.base,
        &result.merged,
    )?;
    let mut state = analysis.state;
    state.applied = result.merged.clone();
    let mut changes = provider_definition_changes(selected, &state.provider, &result.merged)?;
    changes.extend(agent_file_changes(&desired));
    commit_transaction(selected, changes, Some(state))
}

fn explicit_resolutions(args: &ReconcileArgs) -> Result<BTreeMap<Pointer, ConflictChoice>> {
    let mut resolutions = BTreeMap::new();
    for (paths, choice) in [
        (&args.take_provider, ConflictChoice::Provider),
        (&args.take_config, ConflictChoice::Config),
    ] {
        for path in paths {
            let path = Pointer::parse(path)?;
            if let Some(previous) = resolutions.insert(path.clone(), choice) {
                if previous != choice {
                    bail!("conflicting resolutions were supplied for {path}");
                }
            }
        }
    }
    Ok(resolutions)
}

fn analyze(selected: &TenantAgent) -> Result<Option<Analysis>> {
    analyze_unlocked(selected)
}

fn analyze_unlocked(selected: &TenantAgent) -> Result<Option<Analysis>> {
    let Some(state) = read_active_state(selected)? else {
        return Ok(None);
    };
    let source = read_provider_definition(selected, &state.provider)?;
    let keys = auth_keys(&state.applied, &source);
    let base_tree = effective_from_snapshots(selected.agent, &state.base, &keys)?;
    let expected = agent_config::materialize(&base_tree, &state.applied)?;
    let current = capture_agent_files(selected)?;
    let working_tree = effective_from_snapshots(selected.agent, &current, &keys)?;
    let working = agent_config::working_definition(&state.applied, &expected, &working_tree)?;
    let result = agent_config::reconcile(&state.applied, &working, &source, &BTreeMap::new())?;
    Ok(Some(Analysis {
        state,
        source,
        base_tree,
        working,
        changes: result.changes,
    }))
}

fn auth_keys(left: &ProviderDefinition, right: &ProviderDefinition) -> BTreeSet<String> {
    left.auth_keys()
        .union(&right.auth_keys())
        .cloned()
        .collect()
}

fn expected_tree(
    selected: &TenantAgent,
    state: &ActiveProviderState,
    source: &ProviderDefinition,
) -> Result<serde_json::Value> {
    let keys = auth_keys(&state.applied, source);
    let base = effective_from_snapshots(selected.agent, &state.base, &keys)?;
    agent_config::materialize(&base, &state.applied)
}

fn read_provider_definition(selected: &TenantAgent, provider: &str) -> Result<ProviderDefinition> {
    ensure_provider_exists(selected, provider)?;
    let main =
        read_regular_string(&selected.provider_file(provider, selected.agent.main_config_file()))?;
    let auth_path = selected.provider_file(provider, selected.agent.provider_auth_file());
    validate_private_file(&auth_path)?;
    let auth = read_regular_string(&auth_path)?;
    let metadata = read_regular_string(&selected.provider_file(provider, PROVIDER_METADATA_FILE))?;
    ProviderDefinition::parse(selected.agent, &main, &auth, Some(&metadata))
        .with_context(|| format!("parse Provider '{provider}'"))
}

fn provider_definition_changes(
    selected: &TenantAgent,
    provider: &str,
    definition: &ProviderDefinition,
) -> Result<Vec<PendingChange>> {
    let (main, auth, metadata) = definition.render(selected.agent)?;
    let main_mode =
        existing_mode(&selected.provider_file(provider, selected.agent.main_config_file()))?
            .unwrap_or(0o644);
    Ok(vec![
        provider_file_change(
            provider,
            selected.agent.main_config_file(),
            &main,
            main_mode,
        ),
        provider_file_change(provider, selected.agent.provider_auth_file(), &auth, 0o600),
        provider_file_change(provider, PROVIDER_METADATA_FILE, &metadata, 0o644),
    ])
}

fn ensure_provider_exists(selected: &TenantAgent, provider: &str) -> Result<()> {
    tenant::validate_name("provider", provider)?;
    if !selected.metadata_dir_exists()? {
        bail!("Provider '{provider}' does not exist");
    }
    if !provider_exists(selected, provider)? {
        bail!("Provider '{provider}' does not exist");
    }
    for file in selected.agent.provider_files() {
        if !tenant::real_file_exists(&selected.provider_file(provider, file), "Provider file")? {
            bail!("Provider '{provider}' is incomplete: missing {file}");
        }
    }
    Ok(())
}

fn provider_exists(selected: &TenantAgent, provider: &str) -> Result<bool> {
    tenant::validate_name("provider", provider)?;
    if !selected.metadata_dir_exists()? {
        return Ok(false);
    }
    tenant::real_dir_exists(&selected.provider_dir(provider), "Provider directory")
}

fn provider_files_are_regular(selected: &TenantAgent, provider: &str) -> bool {
    selected.agent.provider_files().iter().all(|file| {
        fs::symlink_metadata(selected.provider_file(provider, file))
            .is_ok_and(|metadata| metadata.file_type().is_file())
    })
}

fn read_scope_metadata(selected: &TenantAgent) -> Result<ScopeMetadata> {
    if !selected.metadata_dir_exists()? {
        return Ok(ScopeMetadata::default());
    }
    let path = selected.metadata_file();
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(ScopeMetadata::default()),
        Err(error) => Err(error.into()),
        Ok(meta) if !meta.file_type().is_file() => {
            bail!("Tenant metadata is not a regular file: {}", path.display())
        }
        Ok(_) => {
            validate_private_file(&path)?;
            let metadata: ScopeMetadata =
                serde_json::from_str(&read_regular_string_with_limit(&path, MAX_STATE_BYTES)?)
                    .context("parse Agent/Tenant metadata")?;
            if let Some(state) = &metadata.active_provider {
                tenant::validate_name("provider", &state.provider)?;
            }
            validate_pending(selected, metadata.pending.as_ref())?;
            Ok(metadata)
        }
    }
}

fn read_active_state(selected: &TenantAgent) -> Result<Option<ActiveProviderState>> {
    Ok(read_scope_metadata(selected)?.active_provider)
}

fn write_scope_metadata(selected: &TenantAgent, metadata: &ScopeMetadata) -> Result<()> {
    selected.ensure_for_management()?;
    let content = format!("{}\n", serde_json::to_string_pretty(metadata)?);
    write_atomic(&selected.metadata_file(), content.as_bytes(), Some(0o600))
}

fn capture_agent_files(selected: &TenantAgent) -> Result<BTreeMap<String, FileSnapshot>> {
    selected.validate_existing()?;
    selected
        .agent
        .agent_config_files()
        .iter()
        .map(|file_name| {
            let snapshot = FileSnapshot::capture(&selected.active_file(file_name))?;
            if snapshot.content.len() as u64 > MAX_CONFIG_BYTES {
                bail!("Agent Configuration file exceeds {MAX_CONFIG_BYTES} bytes: {file_name}");
            }
            Ok(((*file_name).to_string(), snapshot))
        })
        .collect()
}

fn effective_from_snapshots(
    agent: AgentKind,
    snapshots: &BTreeMap<String, FileSnapshot>,
    auth_keys: &BTreeSet<String>,
) -> Result<serde_json::Value> {
    let main = snapshot_text(snapshots, agent.main_config_file())?;
    let auth = agent
        .active_auth_file()
        .map(|name| snapshot_text(snapshots, name))
        .transpose()?;
    agent_config::effective_tree(agent, &main, auth.as_deref(), auth_keys)
}

fn snapshot_text(snapshots: &BTreeMap<String, FileSnapshot>, file_name: &str) -> Result<String> {
    let snapshot = snapshots
        .get(file_name)
        .with_context(|| format!("missing snapshot for {file_name}"))?;
    if !snapshot.present {
        return Ok(String::new());
    }
    String::from_utf8(snapshot.content.clone())
        .with_context(|| format!("Agent Configuration {file_name} is not valid UTF-8"))
}

fn snapshots_from_effective(
    selected: &TenantAgent,
    tree: &serde_json::Value,
    current: &BTreeMap<String, FileSnapshot>,
    base: &BTreeMap<String, FileSnapshot>,
    provider: &ProviderDefinition,
) -> Result<BTreeMap<String, FileSnapshot>> {
    let (main, auth) = agent_config::render_effective(selected.agent, tree)?;
    let mut snapshots = BTreeMap::new();
    let main_file = selected.agent.main_config_file();
    let base_main = base
        .get(main_file)
        .with_context(|| format!("missing base Agent Configuration snapshot for {main_file}"))?;
    let owns_main = provider.owns_domain("config")
        || (selected.agent == AgentKind::Claude && provider.owns_domain("auth"));
    snapshots.insert(
        main_file.to_string(),
        FileSnapshot {
            present: base_main.present || owns_main,
            content: if owns_main {
                main.into_bytes()
            } else {
                base_main.content.clone()
            },
            mode: if owns_main {
                current
                    .get(main_file)
                    .and_then(|snapshot| snapshot.mode)
                    .or(base_main.mode)
                    .or(Some(0o644))
            } else {
                base_main.mode
            },
        },
    );
    if let Some(auth_file) = selected.agent.active_auth_file() {
        let owns_auth = provider.owns_domain("auth");
        let base_auth = base.get(auth_file).with_context(|| {
            format!("missing base Agent Configuration snapshot for {auth_file}")
        })?;
        snapshots.insert(
            auth_file.to_string(),
            FileSnapshot {
                present: base_auth.present || owns_auth,
                content: if owns_auth {
                    auth.context("Codex normalized configuration has no auth")?
                        .into_bytes()
                } else {
                    base_auth.content.clone()
                },
                mode: if owns_auth {
                    Some(0o600)
                } else {
                    base_auth.mode
                },
            },
        );
    }
    Ok(snapshots)
}

/// Finish a durable Provider transaction left by an interrupted command.
pub(crate) fn recover_pending(selected: &TenantAgent) -> Result<()> {
    if matches!(&selected.tenant, Tenant::Managed(tenant) if !tenant.exists()?) {
        return Ok(());
    }
    let metadata = read_scope_metadata(selected)?;
    let Some(pending) = metadata.pending else {
        return Ok(());
    };
    selected.ensure_for_management()?;
    apply_pending(selected, &pending).with_context(|| {
        "resume pending Provider transaction; its progress remains recorded for the next command"
    })?;
    write_scope_metadata(
        selected,
        &ScopeMetadata {
            active_provider: pending.active_provider,
            pending: None,
        },
    )
    .context("finish recovered Provider transaction")
}

fn commit_transaction(
    selected: &TenantAgent,
    changes: Vec<PendingChange>,
    active_provider: Option<ActiveProviderState>,
) -> Result<()> {
    selected.ensure_for_management()?;
    recover_pending(selected)?;
    let committed = read_scope_metadata(selected)?;
    if committed.pending.is_some() {
        bail!("a pending Provider transaction could not be recovered");
    }
    let pending = PendingTransaction {
        changes,
        active_provider,
    };
    validate_pending(selected, Some(&pending))?;
    write_scope_metadata(
        selected,
        &ScopeMetadata {
            active_provider: committed.active_provider,
            pending: Some(pending.clone()),
        },
    )?;
    apply_pending(selected, &pending).with_context(|| {
        "Provider transaction was interrupted; its progress was saved and will resume on the next command"
    })?;
    write_scope_metadata(
        selected,
        &ScopeMetadata {
            active_provider: pending.active_provider,
            pending: None,
        },
    )
    .context("commit Provider transaction")
}

fn validate_pending(selected: &TenantAgent, pending: Option<&PendingTransaction>) -> Result<()> {
    let Some(pending) = pending else {
        return Ok(());
    };
    if let Some(state) = &pending.active_provider {
        tenant::validate_name("provider", &state.provider)?;
    }
    for change in &pending.changes {
        match change {
            PendingChange::AgentFile { file, snapshot } => {
                if !selected.agent.agent_config_files().contains(&file.as_str()) {
                    bail!("pending transaction names an unsupported Agent file '{file}'");
                }
                validate_snapshot(file, snapshot)?;
            }
            PendingChange::ProviderDirectory { provider, .. } => {
                tenant::validate_name("provider", provider)?;
            }
            PendingChange::ProviderFile {
                provider,
                file,
                snapshot,
            } => {
                tenant::validate_name("provider", provider)?;
                if !selected.agent.provider_files().contains(&file.as_str()) {
                    bail!("pending transaction names an unsupported Provider file '{file}'");
                }
                if file == selected.agent.provider_auth_file()
                    && snapshot.present
                    && snapshot.mode != Some(0o600)
                {
                    bail!("pending Provider auth file must have mode 0600");
                }
                validate_snapshot(file, snapshot)?;
            }
        }
    }
    Ok(())
}

fn validate_snapshot(file: &str, snapshot: &FileSnapshot) -> Result<()> {
    if snapshot.content.len() as u64 > MAX_CONFIG_BYTES {
        bail!("pending snapshot exceeds {MAX_CONFIG_BYTES} bytes: {file}");
    }
    if !snapshot.present && (!snapshot.content.is_empty() || snapshot.mode.is_some()) {
        bail!("absent pending snapshot carries file data: {file}");
    }
    Ok(())
}

fn apply_pending(selected: &TenantAgent, pending: &PendingTransaction) -> Result<()> {
    validate_pending(selected, Some(pending))?;
    for change in &pending.changes {
        apply_change(selected, change)?;
    }
    Ok(())
}

fn apply_change(selected: &TenantAgent, change: &PendingChange) -> Result<()> {
    match change {
        PendingChange::AgentFile { file, snapshot } => {
            write_snapshot(&selected.active_file(file), snapshot)
        }
        PendingChange::ProviderDirectory { provider, present } => {
            let path = selected.provider_dir(provider);
            if *present {
                ensure_provider_dir(selected, &path)
            } else {
                remove_provider_dir_if_exists(&path)
            }
        }
        PendingChange::ProviderFile {
            provider,
            file,
            snapshot,
        } => {
            let directory = selected.provider_dir(provider);
            ensure_provider_dir(selected, &directory)?;
            write_snapshot(&selected.provider_file(provider, file), snapshot)
        }
    }
}

fn ensure_provider_dir(selected: &TenantAgent, path: &Path) -> Result<()> {
    let existed = tenant::real_dir_exists(path, "Provider directory")?;
    tenant::ensure_real_dir(path, "Provider directory")?;
    if !existed {
        tenant::sync_dir(selected.metadata_dir())?;
    }
    Ok(())
}

fn remove_provider_dir_if_exists(path: &Path) -> Result<()> {
    let parent = path.parent().context("Provider directory has no parent")?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            bail!("Provider path is not a real directory: {}", path.display())
        }
        Ok(_) => {
            fs::remove_dir_all(path)
                .with_context(|| format!("delete Provider directory {}", path.display()))?;
            tenant::sync_dir(parent)
        }
    }
}

fn provider_file_change(provider: &str, file: &str, content: &str, mode: u32) -> PendingChange {
    PendingChange::ProviderFile {
        provider: provider.to_string(),
        file: file.to_string(),
        snapshot: FileSnapshot {
            present: true,
            content: content.as_bytes().to_vec(),
            mode: Some(mode),
        },
    }
}

fn agent_file_changes(snapshots: &BTreeMap<String, FileSnapshot>) -> Vec<PendingChange> {
    snapshots
        .iter()
        .map(|(file, snapshot)| PendingChange::AgentFile {
            file: file.clone(),
            snapshot: snapshot.clone(),
        })
        .collect()
}

fn write_snapshot(path: &Path, snapshot: &FileSnapshot) -> Result<()> {
    if !snapshot.present {
        return remove_regular_if_exists(path);
    }
    let mode = snapshot.mode.unwrap_or(0o644);
    write_atomic(path, &snapshot.content, Some(mode))
}

fn read_regular_string(path: &Path) -> Result<String> {
    String::from_utf8(read_regular_bytes_with_limit(path, MAX_CONFIG_BYTES)?)
        .with_context(|| format!("{} is not valid UTF-8", path.display()))
}

fn read_regular_bytes(path: &Path) -> Result<Vec<u8>> {
    read_regular_bytes_with_limit(path, MAX_CONFIG_BYTES)
}

fn read_regular_string_with_limit(path: &Path, limit: u64) -> Result<String> {
    String::from_utf8(read_regular_bytes_with_limit(path, limit)?)
        .with_context(|| format!("{} is not valid UTF-8", path.display()))
}

fn read_regular_bytes_with_limit(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = tenant::open_real_file(path, "configuration file")?;
    let size = file.metadata()?.len();
    if size > limit {
        bail!(
            "configuration file exceeds {limit} bytes: {}",
            path.display()
        );
    }
    let mut content = Vec::new();
    file.take(limit + 1).read_to_end(&mut content)?;
    if content.len() as u64 > limit {
        bail!(
            "configuration file exceeds {limit} bytes: {}",
            path.display()
        );
    }
    Ok(content)
}

fn validate_private_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::symlink_metadata(path)?.permissions().mode() & 0o777;
        if mode != 0o600 {
            bail!("private file must have mode 0600: {}", path.display());
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn existing_mode(path: &Path) -> Result<Option<u32>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
        Ok(meta) if !meta.file_type().is_file() => {
            bail!(
                "configuration path is not a regular file: {}",
                path.display()
            )
        }
        Ok(meta) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                Ok(Some(meta.permissions().mode() & 0o7777))
            }
            #[cfg(not(unix))]
            {
                let _ = meta;
                Ok(None)
            }
        }
    }
}

fn write_atomic(path: &Path, content: &[u8], mode: Option<u32>) -> Result<()> {
    if content.len() as u64 > MAX_STATE_BYTES {
        bail!("refusing oversized state write: {}", path.display());
    }
    let parent = path.parent().context("configuration path has no parent")?;
    tenant::ensure_real_dir(parent, "configuration parent directory")?;
    match fs::symlink_metadata(path) {
        Ok(meta) if !meta.file_type().is_file() => {
            bail!(
                "configuration path is not a regular file: {}",
                path.display()
            )
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let temp = sibling_temp_path(path, "write")?;
    let result = (|| {
        write_new_file(&temp, content, mode.unwrap_or(0o644))?;
        fs::rename(&temp, path).with_context(|| format!("replace {}", path.display()))?;
        tenant::sync_dir(parent)
    })();
    let _ = fs::remove_file(&temp);
    result
}

fn write_new_file(path: &Path, content: &[u8], mode: u32) -> Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let mut file = options.open(path)?;
    file.write_all(content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    file.sync_all()?;
    Ok(())
}

fn sibling_temp_path(path: &Path, purpose: &str) -> Result<PathBuf> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .context("configuration file name is not valid UTF-8")?;
    Ok(path.with_file_name(format!(
        ".{name}.aibox-{purpose}-{}-{}",
        std::process::id(),
        now_nanos()?
    )))
}

fn remove_regular_if_exists(path: &Path) -> Result<()> {
    let parent = path.parent().context("configuration path has no parent")?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(meta) if !meta.file_type().is_file() => {
            bail!(
                "configuration path is not a regular file: {}",
                path.display()
            )
        }
        Ok(_) => {
            fs::remove_file(path).with_context(|| format!("delete {}", path.display()))?;
            tenant::sync_dir(parent)
        }
    }
}

fn configured_editor() -> OsString {
    non_empty_env("VISUAL")
        .or_else(|| non_empty_env("EDITOR"))
        .unwrap_or_else(|| "vim".into())
}

fn non_empty_env(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.to_string_lossy().trim().is_empty())
}

fn editor_command(editor: &OsStr) -> Result<Command> {
    let mut parts = split_editor_command(editor)?;
    let program = parts.remove(0);
    let mut command = Command::new(program);
    command.args(parts);
    Ok(command)
}

fn split_editor_command(editor: &OsStr) -> Result<Vec<OsString>> {
    let Some(editor) = editor.to_str() else {
        return Ok(vec![editor.to_os_string()]);
    };
    let words = split_shell_words(editor)?;
    if words.is_empty() {
        bail!("editor command is empty");
    }
    Ok(words.into_iter().map(OsString::from).collect())
}

fn split_shell_words(input: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut quote = None;
    let mut in_word = false;
    while let Some(character) = chars.next() {
        match quote {
            Some('\'') => {
                if character == '\'' {
                    quote = None;
                } else {
                    current.push(character);
                }
            }
            Some('"') => match character {
                '"' => quote = None,
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                _ => current.push(character),
            },
            Some(_) => unreachable!(),
            None => match character {
                '\'' | '"' => {
                    quote = Some(character);
                    in_word = true;
                }
                '\\' => {
                    current.push(chars.next().context("trailing escape in editor command")?);
                    in_word = true;
                }
                character if character.is_whitespace() => {
                    if in_word {
                        words.push(std::mem::take(&mut current));
                        in_word = false;
                    }
                }
                character => {
                    current.push(character);
                    in_word = true;
                }
            },
        }
    }
    if quote.is_some() {
        bail!("unterminated quote in editor command");
    }
    if in_word {
        words.push(current);
    }
    Ok(words)
}

fn confirm_delete(provider: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!("refusing to delete Provider '{provider}' without --yes in a non-interactive shell");
    }
    eprint!("Delete Provider '{provider}'? [y/N] ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim(), "y" | "Y" | "yes" | "YES"))
}

fn now_nanos() -> Result<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before unix epoch")?
        .as_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tenant::{ManagedTenant, Tenant};
    use crate::testutil::EnvGuard;

    fn selected(root: &Path, agent: AgentKind) -> TenantAgent {
        let tenant = ManagedTenant::resolve(root, "work").unwrap();
        tenant.ensure_initialized().unwrap();
        tenant.for_agent(agent)
    }

    #[test]
    fn providers_are_tenant_and_agent_local() {
        let root = tempfile::tempdir().unwrap();
        let codex = selected(root.path(), AgentKind::Codex);
        let claude = selected(root.path(), AgentKind::Claude);
        create_provider(&codex, "custom").unwrap();
        assert_eq!(list_providers(&codex).unwrap(), ["custom"]);
        assert!(list_providers(&claude).unwrap().is_empty());
    }

    #[test]
    fn codex_provider_uses_default_native_configuration() {
        let root = tempfile::tempdir().unwrap();
        let codex = selected(root.path(), AgentKind::Codex);

        create_provider(&codex, "custom").unwrap();

        assert_eq!(
            fs::read_to_string(codex.provider_file("custom", "config.toml")).unwrap(),
            r#"approval_policy = "never"
sandbox_mode = "danger-full-access"
model_reasoning_effort = "xhigh"
plan_mode_reasoning_effort = "xhigh"
model = "gpt-5.6-sol"
model_provider = "custom"

[model_providers.custom]
base_url = "https://example.com/v1"
requires_openai_auth = true
"#
        );
    }

    #[test]
    fn claude_provider_uses_default_native_configuration() {
        let root = tempfile::tempdir().unwrap();
        let claude = selected(root.path(), AgentKind::Claude);

        create_provider(&claude, "custom").unwrap();

        assert_eq!(
            fs::read_to_string(claude.provider_file("custom", "settings.json")).unwrap(),
            r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://example.com",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4-5",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-5[1m]",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-5[1m]",
    "ANTHROPIC_DEFAULT_FABLE_MODEL": "claude-fable-5[1m]"
  },
  "permissions": {
    "defaultMode": "bypassPermissions"
  },
  "skipDangerousModePermissionPrompt": true
}
"#
        );
    }

    #[test]
    fn host_provider_creation_does_not_install_managed_tenant_baseline_files() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("HOME", home.path());
        let selected = Tenant::resolve(root.path(), true, "default")
            .unwrap()
            .for_agent(AgentKind::Claude);

        create_provider(&selected, "custom").unwrap();

        assert!(selected.provider_dir("custom").is_dir());
        assert!(!home.path().join(".gitconfig").exists());
        assert!(!home.path().join(".claude/statusline.sh").exists());
    }

    #[test]
    fn host_provider_activation_does_not_install_managed_tenant_statusline() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("HOME", home.path());
        let selected = Tenant::resolve(root.path(), true, "default")
            .unwrap()
            .for_agent(AgentKind::Claude);
        create_provider(&selected, "custom").unwrap();

        activate_provider(&selected, "custom", false).unwrap();

        assert!(home.path().join(".claude/settings.json").is_file());
        assert!(!home.path().join(".claude/statusline.sh").exists());
    }

    #[test]
    fn missing_managed_tenant_ignores_orphaned_provider_metadata() {
        let root = tempfile::tempdir().unwrap();
        let managed = ManagedTenant::resolve(root.path(), "work").unwrap();
        let selected = managed.for_agent(AgentKind::Codex);
        let orphan = root.path().join("codex/work/custom");
        fs::create_dir_all(&orphan).unwrap();
        fs::write(orphan.join("config.toml"), DEFAULT_CODEX_CONFIG).unwrap();
        fs::write(orphan.join("auth.json"), "{}\n").unwrap();
        tenant::set_600(&orphan.join("auth.json")).unwrap();
        fs::write(orphan.join(PROVIDER_METADATA_FILE), "{\"tombstones\":[]}\n").unwrap();

        assert!(list_providers(&selected).unwrap().is_empty());
        assert!(read_active_state(&selected).unwrap().is_none());
        delete_providers(&selected, &["custom".to_string()], false, true).unwrap();
        assert!(!managed.home_dir.exists());
        assert!(orphan.exists());
    }

    #[test]
    fn activation_materializes_and_deactivation_restores_exact_base() {
        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Claude);
        fs::write(
            selected.active_file("settings.json"),
            b"{\"theme\":\"dark\"}\n",
        )
        .unwrap();
        let base = fs::read(selected.active_file("settings.json")).unwrap();
        create_provider(&selected, "custom").unwrap();
        activate_provider(&selected, "custom", false).unwrap();
        let active = fs::read_to_string(selected.active_file("settings.json")).unwrap();
        assert!(active.contains("ANTHROPIC_BASE_URL"));
        assert!(active.contains("theme"));
        deactivate_provider(&selected, false).unwrap();
        assert_eq!(
            fs::read(selected.active_file("settings.json")).unwrap(),
            base
        );
        assert!(read_active_state(&selected).unwrap().is_none());
    }

    #[test]
    fn empty_codex_provider_auth_does_not_create_native_auth_file() {
        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Codex);
        create_provider(&selected, "custom").unwrap();
        assert_eq!(
            fs::read_to_string(selected.provider_file("custom", "auth.json")).unwrap(),
            "{}\n"
        );

        activate_provider(&selected, "custom", false).unwrap();

        assert!(selected.active_file("config.toml").is_file());
        assert!(!selected.active_file("auth.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn empty_codex_provider_auth_preserves_existing_native_auth() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Codex);
        let auth = selected.active_file("auth.json");
        let original = b"{\"token\":\"native\"}\n";
        fs::write(&auth, original).unwrap();
        fs::set_permissions(&auth, fs::Permissions::from_mode(0o400)).unwrap();
        create_provider(&selected, "custom").unwrap();

        activate_provider(&selected, "custom", false).unwrap();

        assert_eq!(fs::read(&auth).unwrap(), original);
        assert_eq!(
            fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
            0o400
        );
    }

    #[test]
    fn provider_that_owns_no_config_preserves_native_file_bytes() {
        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Codex);
        let config = selected.active_file("config.toml");
        let original = b"# keep this formatting\nmodel='native'\n";
        fs::write(&config, original).unwrap();
        create_provider(&selected, "empty").unwrap();
        fs::write(selected.provider_file("empty", "config.toml"), b"\n").unwrap();

        activate_provider(&selected, "empty", false).unwrap();

        assert_eq!(fs::read(&config).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn provider_auth_requires_exact_owner_read_write_mode() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Codex);
        create_provider(&selected, "custom").unwrap();
        let auth = selected.provider_file("custom", "auth.json");
        fs::set_permissions(&auth, fs::Permissions::from_mode(0o400)).unwrap();

        let error = activate_provider(&selected, "custom", false)
            .unwrap_err()
            .to_string();

        assert!(error.contains("mode 0600"), "{error}");
        assert!(!selected.active_file("config.toml").exists());
    }

    #[cfg(unix)]
    #[test]
    fn codex_auth_permissions_round_trip_through_deactivation() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Codex);
        let auth = selected.active_file("auth.json");
        fs::write(&auth, b"{\"token\":\"base\"}\n").unwrap();
        fs::set_permissions(&auth, fs::Permissions::from_mode(0o400)).unwrap();
        create_provider(&selected, "custom").unwrap();
        fs::write(
            selected.provider_file("custom", "auth.json"),
            b"{\"token\":\"provider\"}\n",
        )
        .unwrap();

        activate_provider(&selected, "custom", false).unwrap();
        assert_eq!(
            fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
            0o600
        );
        deactivate_provider(&selected, false).unwrap();
        assert_eq!(
            fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
            0o400
        );
    }

    #[test]
    fn working_drift_blocks_switch_without_explicit_discard() {
        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Claude);
        create_provider(&selected, "one").unwrap();
        create_provider(&selected, "two").unwrap();
        activate_provider(&selected, "one", false).unwrap();
        fs::write(
            selected.active_file("settings.json"),
            b"{\"changed\":true}\n",
        )
        .unwrap();
        let error = activate_provider(&selected, "two", false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("working changes"));
        activate_provider(&selected, "two", true).unwrap();
    }

    #[test]
    fn reconcile_moves_non_overlapping_changes_both_directions() {
        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Claude);
        create_provider(&selected, "custom").unwrap();
        fs::write(
            selected.provider_file("custom", "settings.json"),
            b"{\"model\":\"a\",\"source\":1}\n",
        )
        .unwrap();
        activate_provider(&selected, "custom", false).unwrap();
        fs::write(
            selected.provider_file("custom", "settings.json"),
            b"{\"model\":\"a\",\"source\":2}\n",
        )
        .unwrap();
        fs::write(
            selected.active_file("settings.json"),
            b"{\"model\":\"working\",\"source\":1}\n",
        )
        .unwrap();
        reconcile_provider(
            &selected,
            &ReconcileArgs {
                take_provider: Vec::new(),
                take_config: Vec::new(),
                take_provider_all: false,
                take_config_all: false,
            },
        )
        .unwrap();
        let source = fs::read_to_string(selected.provider_file("custom", "settings.json")).unwrap();
        let working = fs::read_to_string(selected.active_file("settings.json")).unwrap();
        assert!(source.contains("working"));
        assert!(source.contains("2"));
        assert!(working.contains("working"));
        assert!(working.contains("2"));
    }

    #[test]
    fn pending_provider_creation_resumes_after_partial_application() {
        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Codex);
        selected.ensure_for_management().unwrap();
        let pending = PendingTransaction {
            changes: vec![
                PendingChange::ProviderDirectory {
                    provider: "custom".to_string(),
                    present: true,
                },
                provider_file_change(
                    "custom",
                    selected.agent.main_config_file(),
                    DEFAULT_CODEX_CONFIG,
                    0o644,
                ),
                provider_file_change("custom", "auth.json", "{}\n", 0o600),
                provider_file_change(
                    "custom",
                    PROVIDER_METADATA_FILE,
                    "{\n  \"tombstones\": []\n}\n",
                    0o644,
                ),
            ],
            active_provider: None,
        };
        write_scope_metadata(
            &selected,
            &ScopeMetadata {
                active_provider: None,
                pending: Some(pending.clone()),
            },
        )
        .unwrap();
        apply_change(&selected, &pending.changes[0]).unwrap();
        apply_change(&selected, &pending.changes[1]).unwrap();

        recover_pending(&selected).unwrap();

        assert_eq!(list_providers(&selected).unwrap(), ["custom"]);
        assert!(read_scope_metadata(&selected).unwrap().pending.is_none());
    }

    #[test]
    fn pending_agent_file_removal_is_idempotently_replayed() {
        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Codex);
        let config = selected.active_file("config.toml");
        fs::write(&config, b"model = \"native\"\n").unwrap();
        let pending = PendingTransaction {
            changes: vec![PendingChange::AgentFile {
                file: "config.toml".to_string(),
                snapshot: FileSnapshot {
                    present: false,
                    content: Vec::new(),
                    mode: None,
                },
            }],
            active_provider: None,
        };
        write_scope_metadata(
            &selected,
            &ScopeMetadata {
                active_provider: None,
                pending: Some(pending.clone()),
            },
        )
        .unwrap();
        apply_pending(&selected, &pending).unwrap();

        recover_pending(&selected).unwrap();

        assert!(!config.exists());
        assert!(read_scope_metadata(&selected).unwrap().pending.is_none());
    }

    #[test]
    fn pending_transaction_rejects_untyped_paths() {
        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Codex);
        selected.ensure_for_management().unwrap();
        fs::write(
            selected.metadata_file(),
            r#"{
  "active_provider": null,
  "pending": {
    "changes": [{
      "kind": "agent-file",
      "file": "../outside",
      "snapshot": {"present": false, "content": "", "mode": null}
    }],
    "active_provider": null
  }
}
"#,
        )
        .unwrap();
        tenant::set_600(&selected.metadata_file()).unwrap();

        let error = recover_pending(&selected).unwrap_err().to_string();

        assert!(error.contains("unsupported Agent file"), "{error}");
        assert!(!root.path().join("outside").exists());
    }

    #[test]
    fn empty_delete_selection_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Codex);
        let error = delete_providers(&selected, &[], false, true)
            .unwrap_err()
            .to_string();
        assert!(error.contains("at least one"));
    }

    #[test]
    fn delete_all_keeps_the_active_provider() {
        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Codex);
        create_provider(&selected, "active").unwrap();
        create_provider(&selected, "inactive").unwrap();
        activate_provider(&selected, "active", false).unwrap();

        delete_providers(&selected, &[], true, true).unwrap();

        assert_eq!(list_providers(&selected).unwrap(), ["active"]);
        assert_eq!(
            read_active_state(&selected).unwrap().unwrap().provider,
            "active"
        );
    }

    #[cfg(unix)]
    #[test]
    fn scope_metadata_is_private_and_omits_an_empty_pending_field() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let selected = selected(root.path(), AgentKind::Codex);
        create_provider(&selected, "custom").unwrap();

        let metadata = fs::read_to_string(selected.metadata_file()).unwrap();
        let mode = fs::metadata(selected.metadata_file())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        assert!(!metadata.contains("\"pending\""), "{metadata}");
    }
}
