//! Tenant-local Agent Profile catalogs, activation state, reconciliation, and
//! roll-forward multi-file transactions.

use crate::agent::AgentKind;
use crate::agent_config::{
    self, Change, ChangeClass, ConflictChoice, DiffEntry, Pointer, ProfileDefinition,
    PROFILE_METADATA_FILE,
};
use crate::cli::{ProfileCommand, ProfileDiffArgs, ReconcileArgs};
use crate::tenant::{self, FileSnapshot, Tenant, TenantAgent};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::Path;
use std::process::Command;

const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;
const MAX_STATE_BYTES: u64 = 256 * 1024 * 1024;
type AgentFileSnapshots = BTreeMap<String, FileSnapshot>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActiveProfileState {
    profile: String,
    base: AgentFileSnapshots,
    applied: ProfileDefinition,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct ScopeMetadata {
    active_profile: Option<ActiveProfileState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending: Option<PendingTransaction>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PendingTransaction {
    changes: Vec<PendingChange>,
    active_profile: Option<ActiveProfileState>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum PendingChange {
    AgentFile {
        file: String,
        snapshot: FileSnapshot,
    },
    ProfileDirectory {
        profile: String,
        present: bool,
    },
    ProfileFile {
        profile: String,
        file: String,
        snapshot: FileSnapshot,
    },
}

struct Analysis {
    state: ActiveProfileState,
    source: ProfileDefinition,
    base_tree: serde_json::Value,
    working: ProfileDefinition,
    changes: Vec<Change>,
}

#[derive(Clone, Copy)]
struct SnapshotOptions {
    preserve_component_config: bool,
    restore_base_main_mode: bool,
}

/// Execute one parsed Agent Profile command.
pub fn dispatch(selected: &TenantAgent, command: &ProfileCommand) -> Result<i32> {
    recover_pending(selected)?;
    match command {
        ProfileCommand::List => {
            for profile in list_profiles(selected)? {
                if !crate::print_line(&profile)? {
                    break;
                }
            }
        }
        ProfileCommand::Get { profile, auth } => {
            crate::print_text(&get_profile(selected, profile, *auth)?)?;
        }
        ProfileCommand::Create { profile } => create_profile(selected, profile)?,
        ProfileCommand::Edit { profile, auth } => edit_profile(selected, profile, *auth)?,
        ProfileCommand::Delete { profiles, all, yes } => {
            delete_profiles(selected, profiles, *all, *yes)?;
        }
        ProfileCommand::Activate {
            profile,
            discard_config_changes,
        } => activate_profile(selected, profile, *discard_config_changes)?,
        ProfileCommand::Deactivate {
            discard_config_changes,
        } => deactivate_profile(selected, *discard_config_changes)?,
        ProfileCommand::Status => print_status(selected)?,
        ProfileCommand::Diff(args) => print_diff(selected, args)?,
        ProfileCommand::Reconcile(args) => reconcile_profile(selected, args)?,
    }
    Ok(0)
}

/// Create a Tenant-local Agent Profile using the selected Coding Agent's
/// default template.
pub fn create_profile(selected: &TenantAgent, profile: &str) -> Result<()> {
    tenant::validate_name("profile", profile)?;
    selected.ensure_for_management()?;
    recover_pending(selected)?;
    if profile_exists(selected, profile)? {
        read_profile_definition(selected, profile)?;
        return Ok(());
    }
    let main = selected.agent.profile_template();
    let changes = vec![
        PendingChange::ProfileDirectory {
            profile: profile.to_string(),
            present: true,
        },
        profile_file_change(profile, selected.agent.main_config_file(), main, 0o600),
        profile_file_change(
            profile,
            selected.agent.profile_auth_file(),
            selected.agent.profile_auth_template(),
            0o600,
        ),
        profile_file_change(
            profile,
            PROFILE_METADATA_FILE,
            "{\n  \"tombstones\": []\n}\n",
            0o600,
        ),
    ];
    commit_transaction(selected, changes, read_active_state(selected)?)
}

/// List Agent Profile names in the selected Tenant-local catalog.
pub fn list_profiles(selected: &TenantAgent) -> Result<Vec<String>> {
    if !selected.metadata_dir_exists()? {
        return Ok(Vec::new());
    }
    let root = selected.metadata_dir();
    let mut profiles = Vec::new();
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
        if tenant::validate_name("profile", &name).is_ok()
            && profile_files_are_regular(selected, &name)
        {
            profiles.push(name);
        }
    }
    profiles.sort();
    Ok(profiles)
}

/// Print an Agent Profile's main configuration or explicit credential file.
pub fn get_profile(selected: &TenantAgent, profile: &str, auth: bool) -> Result<String> {
    ensure_profile_exists(selected, profile)?;
    let file_name = if auth {
        selected.agent.profile_auth_file()
    } else {
        selected.agent.main_config_file()
    };
    let path = selected.profile_file(profile, file_name);
    if auth {
        validate_private_file(&path)?;
    }
    read_regular_string(&path)
}

/// Edit an Agent Profile source file without changing the working Agent
/// Configuration.
pub fn edit_profile(selected: &TenantAgent, profile: &str, auth: bool) -> Result<()> {
    ensure_profile_exists(selected, profile)?;
    let file_name = if auth {
        selected.agent.profile_auth_file()
    } else {
        selected.agent.main_config_file()
    };
    let path = selected.profile_file(profile, file_name);
    if !auth {
        validate_private_file(&selected.profile_file(profile, selected.agent.profile_auth_file()))?;
    }
    let current = read_regular_bytes(&path)?;
    let parent = path.parent().context("Agent Profile path has no parent")?;
    let prefix = temporary_file_prefix(&path, "edit")?;
    let temp = write_temporary_file(parent, &prefix, &current, 0o600)?;
    let editor = configured_editor();
    let status = editor_command(&editor)?
        .arg(temp.path())
        .status()
        .with_context(|| format!("run editor {editor:?}"))?;
    if !status.success() {
        bail!("editor exited with status {status}");
    }

    let content = read_regular_bytes(temp.path())?;
    let edited = std::str::from_utf8(&content)
        .with_context(|| format!("{} is not valid UTF-8", temp.path().display()))?;
    let other = read_regular_string(&selected.profile_file(
        profile,
        if auth {
            selected.agent.main_config_file()
        } else {
            selected.agent.profile_auth_file()
        },
    ))?;
    let (main, auth_content) = if auth {
        (other.as_str(), edited)
    } else {
        (edited, other.as_str())
    };
    let metadata = read_regular_string(&selected.profile_file(profile, PROFILE_METADATA_FILE))?;
    ProfileDefinition::parse(selected.agent, main, auth_content, Some(&metadata))?;
    commit_transaction(
        selected,
        vec![PendingChange::ProfileFile {
            profile: profile.to_string(),
            file: file_name.to_string(),
            snapshot: FileSnapshot {
                present: true,
                content,
                mode: Some(0o600),
            },
        }],
        read_active_state(selected)?,
    )
}

/// Delete selected inactive Agent Profiles, or all inactive Profiles when
/// explicitly requested.
pub fn delete_profiles(
    selected: &TenantAgent,
    profiles: &[String],
    all: bool,
    yes: bool,
) -> Result<()> {
    if all && !profiles.is_empty() {
        bail!("--all cannot be combined with Agent Profile names");
    }
    if !all && profiles.is_empty() {
        bail!("provide at least one Agent Profile name or use --all");
    }
    let active = read_active_state(selected)?;
    let targets = if all {
        list_profiles(selected)?
            .into_iter()
            .filter(|profile| {
                active
                    .as_ref()
                    .is_none_or(|state| state.profile != *profile)
            })
            .collect()
    } else {
        let mut unique = Vec::new();
        for profile in profiles {
            tenant::validate_name("profile", profile)?;
            if active
                .as_ref()
                .is_some_and(|state| state.profile == *profile)
            {
                bail!("Agent Profile '{profile}' is active; deactivate it before deletion");
            }
            if profile_exists(selected, profile)? && !unique.contains(profile) {
                unique.push(profile.clone());
            }
        }
        unique
    };
    if targets.is_empty() {
        eprintln!(">> no inactive Agent Profiles in this Tenant and Coding Agent");
        return Ok(());
    }
    if !yes {
        for profile in &targets {
            if !confirm_delete(profile)? {
                bail!("aborted");
            }
        }
    }
    let changes = targets
        .into_iter()
        .map(|profile| PendingChange::ProfileDirectory {
            profile,
            present: false,
        })
        .collect();
    commit_transaction(selected, changes, active)
}

/// Reject Component installation when the Active Agent Profile already owns
/// one of the Component's logical configuration paths.
pub(crate) fn ensure_component_paths_available(
    selected: &TenantAgent,
    paths: &[&str],
) -> Result<()> {
    let Some(state) = read_active_state(selected)? else {
        return Ok(());
    };
    for path in paths {
        let path = Pointer::parse(path)?;
        if state.applied.overlaps_path(&path) {
            bail!(
                "Active Agent Profile '{}' owns Component path {}; deactivate or change the Agent Profile first",
                state.profile,
                path
            );
        }
    }
    Ok(())
}

fn ensure_definition_avoids_component_paths(
    definition: &ProfileDefinition,
    paths: &[Pointer],
    profile: &str,
) -> Result<()> {
    if let Some(path) = paths.iter().find(|path| definition.overlaps_path(path)) {
        bail!(
            "Agent Profile '{profile}' owns Component path {}; remove the Component or change the Agent Profile first",
            path.display_for_terminal()
        );
    }
    Ok(())
}

/// Activate an Agent Profile by materializing it into Agent Configuration.
pub fn activate_profile(
    selected: &TenantAgent,
    profile: &str,
    discard_config_changes: bool,
) -> Result<()> {
    ensure_profile_exists(selected, profile)?;
    selected.ensure_for_management()?;
    selected.ensure_agent_state_dir()?;
    let source = read_profile_definition(selected, profile)?;
    let protected = crate::component::protected_config_paths(selected)?;
    ensure_definition_avoids_component_paths(&source, &protected, profile)?;
    let current = capture_agent_files(selected)?;
    let previous = read_active_state(selected)?;
    let restore_base_main_mode = previous
        .as_ref()
        .is_some_and(|state| profile_owns_main(selected.agent, &state.applied));
    let base = if let Some(state) = &previous {
        if !discard_config_changes {
            let keys = auth_keys(&state.applied, &source);
            let base_tree = effective_from_snapshots(selected.agent, &state.base, &keys)?;
            let expected = agent_config::materialize(&base_tree, &state.applied)?;
            ensure_working_config_unchanged(selected, &current, &keys, &expected, &protected)?;
        }
        state.base.clone()
    } else {
        current.clone()
    };

    let base_tree = effective_from_snapshots(selected.agent, &base, &source.auth_keys())?;
    let mut desired_tree = agent_config::materialize(&base_tree, &source)?;
    if !protected.is_empty() {
        let current_tree = effective_from_snapshots(selected.agent, &current, &source.auth_keys())?;
        agent_config::copy_effective_paths(&current_tree, &mut desired_tree, &protected)?;
    }
    let desired = snapshots_from_effective(
        selected,
        &desired_tree,
        &current,
        &base,
        &source,
        SnapshotOptions {
            preserve_component_config: !protected.is_empty(),
            restore_base_main_mode,
        },
    )?;
    let state = ActiveProfileState {
        profile: profile.to_string(),
        base,
        applied: source,
    };
    commit_transaction(selected, agent_file_changes(&desired), Some(state))
}

/// Deactivate the current Agent Profile and restore the pre-activation base.
///
/// Component-owned configuration paths retain their current values.
pub fn deactivate_profile(selected: &TenantAgent, discard_config_changes: bool) -> Result<()> {
    let Some(state) = read_active_state(selected)? else {
        return Ok(());
    };
    selected.ensure_agent_state_dir()?;
    let current = capture_agent_files(selected)?;
    let protected = crate::component::protected_config_paths(selected)?;
    if !discard_config_changes {
        let expected = expected_tree(selected, &state, &state.applied)?;
        ensure_working_config_unchanged(
            selected,
            &current,
            &state.applied.auth_keys(),
            &expected,
            &protected,
        )?;
    }
    if protected.is_empty() {
        return commit_transaction(selected, agent_file_changes(&state.base), None);
    }
    let current_tree =
        effective_from_snapshots(selected.agent, &current, &state.applied.auth_keys())?;
    let mut desired_tree =
        effective_from_snapshots(selected.agent, &state.base, &state.applied.auth_keys())?;
    agent_config::copy_effective_paths(&current_tree, &mut desired_tree, &protected)?;
    let desired = snapshots_from_effective(
        selected,
        &desired_tree,
        &current,
        &state.base,
        &ProfileDefinition::empty(),
        SnapshotOptions {
            preserve_component_config: true,
            restore_base_main_mode: profile_owns_main(selected.agent, &state.applied),
        },
    )?;
    commit_transaction(selected, agent_file_changes(&desired), None)
}

/// Return whether the Active Agent Profile has source or working divergence.
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
    crate::print_line(&format!("active {}", analysis.state.profile))?;
    if analysis.changes.is_empty() {
        crate::print_line("clean")?;
    } else {
        for change in analysis.changes {
            if !crate::print_line(&format!(
                "{} {}",
                change.class,
                change.path.display_for_terminal()
            ))? {
                break;
            }
        }
    }
    Ok(())
}

fn print_diff(selected: &TenantAgent, args: &ProfileDiffArgs) -> Result<()> {
    let Some(analysis) = analyze(selected)? else {
        bail!("no Active Agent Profile");
    };
    let working = agent_config::diff(&analysis.state.applied, &analysis.working);
    let source = agent_config::diff(&analysis.state.applied, &analysis.source);
    if working.is_empty() && source.is_empty() {
        crate::print_line("clean")?;
        return Ok(());
    }
    for (side, entries) in [("working", working), ("source", source)] {
        for entry in entries {
            let line = format_diff_entry(side, &entry, args.show_values);
            if !crate::print_line(&line)? {
                return Ok(());
            }
        }
    }
    Ok(())
}

fn format_diff_entry(side: &str, entry: &DiffEntry, show_values: bool) -> String {
    let classification = match (&entry.old, &entry.new) {
        (None, Some(_)) => "added",
        (Some(_), None) => "removed",
        _ => "modified",
    };
    let path = entry.path.display_for_terminal();
    if !show_values {
        return format!("{side} {classification} {path}");
    }
    let (old, new) = if entry.path.is_auth() {
        ("<redacted>".to_string(), "<redacted>".to_string())
    } else {
        (
            agent_config::display_node(entry.old.as_ref()),
            agent_config::display_node(entry.new.as_ref()),
        )
    };
    format!("{side} {classification} {path}: {old} -> {new}")
}

/// Reconcile source and working changes with a three-way merge.
pub fn reconcile_profile(selected: &TenantAgent, args: &ReconcileArgs) -> Result<()> {
    let analysis = analyze(selected)?.context("no Active Agent Profile")?;
    let mut resolutions = explicit_resolutions(args)?;
    if analysis.changes.is_empty() {
        if let Some(path) = resolutions.keys().next() {
            bail!(
                "resolution path is not a current conflict: {}",
                path.display_for_terminal()
            );
        }
        eprintln!(">> Agent Profile and Agent Configuration are clean");
        return Ok(());
    }
    let all_choice = match (args.take_profile_all, args.take_config_all) {
        (true, false) => Some(ConflictChoice::Profile),
        (false, true) => Some(ConflictChoice::Config),
        _ => None,
    };
    for change in &analysis.changes {
        if change.class != ChangeClass::Conflict {
            continue;
        }
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
        .map(|change| change.path.display_for_terminal())
        .collect();
    if !unresolved.is_empty() {
        bail!(
            "unresolved Agent Profile conflicts:\n{}\nuse --take-profile or --take-config for each path",
            unresolved
                .iter()
                .map(|path| format!("  {path}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    let protected = crate::component::protected_config_paths(selected)?;
    ensure_definition_avoids_component_paths(&result.merged, &protected, &analysis.state.profile)?;

    let current = capture_agent_files(selected)?;
    let mut desired_tree = agent_config::materialize(&analysis.base_tree, &result.merged)?;
    let current_tree =
        effective_from_snapshots(selected.agent, &current, &result.merged.auth_keys())?;
    agent_config::copy_effective_paths(&current_tree, &mut desired_tree, &protected)?;
    let desired = snapshots_from_effective(
        selected,
        &desired_tree,
        &current,
        &analysis.state.base,
        &result.merged,
        SnapshotOptions {
            preserve_component_config: !protected.is_empty(),
            restore_base_main_mode: profile_owns_main(selected.agent, &analysis.state.applied),
        },
    )?;
    let mut state = analysis.state;
    state.applied = result.merged.clone();
    let mut changes = profile_definition_changes(selected, &state.profile, &result.merged)?;
    changes.extend(agent_file_changes(&desired));
    commit_transaction(selected, changes, Some(state))
}

fn explicit_resolutions(args: &ReconcileArgs) -> Result<BTreeMap<Pointer, ConflictChoice>> {
    let mut resolutions = BTreeMap::new();
    for (paths, choice) in [
        (&args.take_profile, ConflictChoice::Profile),
        (&args.take_config, ConflictChoice::Config),
    ] {
        for path in paths {
            let path = Pointer::parse(path)?;
            if let Some(previous) = resolutions.insert(path.clone(), choice) {
                if previous != choice {
                    bail!(
                        "conflicting resolutions were supplied for {}",
                        path.display_for_terminal()
                    );
                }
            }
        }
    }
    Ok(resolutions)
}

fn analyze(selected: &TenantAgent) -> Result<Option<Analysis>> {
    let Some(state) = read_active_state(selected)? else {
        return Ok(None);
    };
    let source = read_profile_definition(selected, &state.profile)?;
    let keys = auth_keys(&state.applied, &source);
    let base_tree = effective_from_snapshots(selected.agent, &state.base, &keys)?;
    let expected = agent_config::materialize(&base_tree, &state.applied)?;
    let current = capture_agent_files(selected)?;
    let mut working_tree = effective_from_snapshots(selected.agent, &current, &keys)?;
    let protected = crate::component::protected_config_paths(selected)?;
    agent_config::copy_effective_paths(&expected, &mut working_tree, &protected)?;
    let working =
        agent_config::working_definition(selected.agent, &state.applied, &expected, &working_tree)?;
    let result = agent_config::reconcile(&state.applied, &working, &source, &BTreeMap::new())?;
    Ok(Some(Analysis {
        state,
        source,
        base_tree,
        working,
        changes: result.changes,
    }))
}

fn auth_keys(left: &ProfileDefinition, right: &ProfileDefinition) -> BTreeSet<String> {
    left.auth_keys()
        .union(&right.auth_keys())
        .cloned()
        .collect()
}

fn expected_tree(
    selected: &TenantAgent,
    state: &ActiveProfileState,
    source: &ProfileDefinition,
) -> Result<serde_json::Value> {
    let keys = auth_keys(&state.applied, source);
    let base = effective_from_snapshots(selected.agent, &state.base, &keys)?;
    agent_config::materialize(&base, &state.applied)
}

fn ensure_working_config_unchanged(
    selected: &TenantAgent,
    current: &AgentFileSnapshots,
    auth_keys: &BTreeSet<String>,
    expected: &serde_json::Value,
    protected: &[Pointer],
) -> Result<()> {
    let mut working = effective_from_snapshots(selected.agent, current, auth_keys)?;
    agent_config::copy_effective_paths(expected, &mut working, protected)?;
    if working != *expected {
        bail!(
            "Agent Configuration has working changes; reconcile them or use --discard-config-changes"
        );
    }
    Ok(())
}

fn read_profile_definition(selected: &TenantAgent, profile: &str) -> Result<ProfileDefinition> {
    ensure_profile_exists(selected, profile)?;
    let main =
        read_regular_string(&selected.profile_file(profile, selected.agent.main_config_file()))?;
    let auth_path = selected.profile_file(profile, selected.agent.profile_auth_file());
    validate_private_file(&auth_path)?;
    let auth = read_regular_string(&auth_path)?;
    let metadata = read_regular_string(&selected.profile_file(profile, PROFILE_METADATA_FILE))?;
    ProfileDefinition::parse(selected.agent, &main, &auth, Some(&metadata))
        .with_context(|| format!("parse Agent Profile '{profile}'"))
}

fn profile_definition_changes(
    selected: &TenantAgent,
    profile: &str,
    definition: &ProfileDefinition,
) -> Result<Vec<PendingChange>> {
    let (main, auth, metadata) = definition.render(selected.agent)?;
    Ok(vec![
        profile_file_change(profile, selected.agent.main_config_file(), &main, 0o600),
        profile_file_change(profile, selected.agent.profile_auth_file(), &auth, 0o600),
        profile_file_change(profile, PROFILE_METADATA_FILE, &metadata, 0o600),
    ])
}

fn ensure_profile_exists(selected: &TenantAgent, profile: &str) -> Result<()> {
    tenant::validate_name("profile", profile)?;
    if !selected.metadata_dir_exists()? {
        bail!("Agent Profile '{profile}' does not exist");
    }
    if !profile_exists(selected, profile)? {
        bail!("Agent Profile '{profile}' does not exist");
    }
    for file in selected.agent.profile_files() {
        let path = selected.profile_file(profile, file);
        if !tenant::real_file_exists(&path, "Profile file")? {
            bail!("Agent Profile '{profile}' is incomplete: missing {file}");
        }
        validate_private_file(&path)?;
    }
    Ok(())
}

fn profile_exists(selected: &TenantAgent, profile: &str) -> Result<bool> {
    tenant::validate_name("profile", profile)?;
    if !selected.metadata_dir_exists()? {
        return Ok(false);
    }
    tenant::real_dir_exists(&selected.profile_dir(profile), "Profile directory")
}

fn profile_files_are_regular(selected: &TenantAgent, profile: &str) -> bool {
    selected.agent.profile_files().iter().all(|file| {
        fs::symlink_metadata(selected.profile_file(profile, file)).is_ok_and(|metadata| {
            if !metadata.file_type().is_file() {
                return false;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                metadata.permissions().mode() & 0o777 == 0o600
            }
            #[cfg(not(unix))]
            {
                true
            }
        })
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
            if let Some(state) = &metadata.active_profile {
                tenant::validate_name("profile", &state.profile)?;
            }
            validate_pending(selected, metadata.pending.as_ref())?;
            Ok(metadata)
        }
    }
}

fn read_active_state(selected: &TenantAgent) -> Result<Option<ActiveProfileState>> {
    Ok(read_scope_metadata(selected)?.active_profile)
}

fn write_scope_metadata(selected: &TenantAgent, metadata: &ScopeMetadata) -> Result<()> {
    selected.ensure_for_management()?;
    let content = format!("{}\n", serde_json::to_string_pretty(metadata)?);
    write_atomic(&selected.metadata_file(), content.as_bytes(), Some(0o600))
}

fn capture_agent_files(selected: &TenantAgent) -> Result<AgentFileSnapshots> {
    selected.validate_existing()?;
    selected
        .agent
        .agent_config_files()
        .iter()
        .map(|file_name| {
            let snapshot = FileSnapshot::capture_with_limit(
                &selected.state_file(file_name),
                MAX_CONFIG_BYTES,
            )?;
            Ok(((*file_name).to_string(), snapshot))
        })
        .collect()
}

fn effective_from_snapshots(
    agent: AgentKind,
    snapshots: &AgentFileSnapshots,
    auth_keys: &BTreeSet<String>,
) -> Result<serde_json::Value> {
    let main = snapshot_text(snapshots, agent.main_config_file())?;
    let auth = agent
        .native_auth_file()
        .map(|name| snapshot_text(snapshots, name))
        .transpose()?;
    agent_config::effective_tree(agent, &main, auth.as_deref(), auth_keys)
}

fn snapshot_text(snapshots: &AgentFileSnapshots, file_name: &str) -> Result<String> {
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
    current: &AgentFileSnapshots,
    base: &AgentFileSnapshots,
    profile: &ProfileDefinition,
    options: SnapshotOptions,
) -> Result<AgentFileSnapshots> {
    let (main, auth) = agent_config::render_effective(selected.agent, tree)?;
    let mut snapshots = BTreeMap::new();
    let main_file = selected.agent.main_config_file();
    let base_main = base
        .get(main_file)
        .with_context(|| format!("missing base Agent Configuration snapshot for {main_file}"))?;
    let owns_main = profile_owns_main(selected.agent, profile);
    let config_has_values = tree
        .get("config")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|config| !config.is_empty());
    snapshots.insert(
        main_file.to_string(),
        main_file_snapshot(
            main,
            base_main,
            current.get(main_file),
            owns_main,
            config_has_values,
            options,
        ),
    );
    if let Some(auth_file) = selected.agent.native_auth_file() {
        let base_auth = base.get(auth_file).with_context(|| {
            format!("missing base Agent Configuration snapshot for {auth_file}")
        })?;
        snapshots.insert(
            auth_file.to_string(),
            auth_file_snapshot(auth, base_auth, profile)?,
        );
    }
    Ok(snapshots)
}

fn main_file_snapshot(
    rendered: String,
    base: &FileSnapshot,
    current: Option<&FileSnapshot>,
    owned_by_profile: bool,
    config_has_values: bool,
    options: SnapshotOptions,
) -> FileSnapshot {
    let render = owned_by_profile || (options.preserve_component_config && config_has_values);
    let present = base.present || render;
    let mode = if !present {
        None
    } else if owned_by_profile {
        Some(0o600)
    } else if options.preserve_component_config {
        if options.restore_base_main_mode {
            base.mode.or(Some(0o644))
        } else {
            current
                .and_then(|snapshot| snapshot.mode)
                .or(base.mode)
                .or(Some(0o644))
        }
    } else {
        base.mode
    };
    FileSnapshot {
        present,
        content: if render {
            rendered.into_bytes()
        } else {
            base.content.clone()
        },
        mode,
    }
}

fn auth_file_snapshot(
    rendered: Option<String>,
    base: &FileSnapshot,
    profile: &ProfileDefinition,
) -> Result<FileSnapshot> {
    if profile.deletes_domain("auth") {
        return Ok(FileSnapshot {
            present: false,
            content: Vec::new(),
            mode: None,
        });
    }
    let owned_by_profile = profile.owns_domain("auth");
    Ok(FileSnapshot {
        present: base.present || owned_by_profile,
        content: if owned_by_profile {
            rendered
                .context("Codex normalized configuration has no auth")?
                .into_bytes()
        } else {
            base.content.clone()
        },
        mode: if owned_by_profile {
            Some(0o600)
        } else {
            base.mode
        },
    })
}

fn profile_owns_main(agent: AgentKind, profile: &ProfileDefinition) -> bool {
    profile.owns_domain("config") || (agent == AgentKind::Claude && profile.owns_domain("auth"))
}

/// Finish a durable Agent Profile transaction left by an interrupted command.
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
        "resume pending Agent Profile transaction; its progress remains recorded for the next command"
    })?;
    write_scope_metadata(
        selected,
        &ScopeMetadata {
            active_profile: pending.active_profile,
            pending: None,
        },
    )
    .context("finish recovered Agent Profile transaction")
}

fn commit_transaction(
    selected: &TenantAgent,
    changes: Vec<PendingChange>,
    active_profile: Option<ActiveProfileState>,
) -> Result<()> {
    selected.ensure_for_management()?;
    recover_pending(selected)?;
    let committed = read_scope_metadata(selected)?;
    if committed.pending.is_some() {
        bail!("a pending Agent Profile transaction could not be recovered");
    }
    let pending = PendingTransaction {
        changes,
        active_profile,
    };
    validate_pending(selected, Some(&pending))?;
    write_scope_metadata(
        selected,
        &ScopeMetadata {
            active_profile: committed.active_profile,
            pending: Some(pending.clone()),
        },
    )?;
    apply_pending(selected, &pending).with_context(|| {
        "Agent Profile transaction was interrupted; its progress was saved and will resume on the next command"
    })?;
    write_scope_metadata(
        selected,
        &ScopeMetadata {
            active_profile: pending.active_profile,
            pending: None,
        },
    )
    .context("commit Agent Profile transaction")
}

fn validate_pending(selected: &TenantAgent, pending: Option<&PendingTransaction>) -> Result<()> {
    let Some(pending) = pending else {
        return Ok(());
    };
    if let Some(state) = &pending.active_profile {
        tenant::validate_name("profile", &state.profile)?;
    }
    for change in &pending.changes {
        match change {
            PendingChange::AgentFile { file, snapshot } => {
                if !selected.agent.agent_config_files().contains(&file.as_str()) {
                    bail!("pending transaction names an unsupported Agent file '{file}'");
                }
                validate_snapshot(file, snapshot)?;
            }
            PendingChange::ProfileDirectory { profile, .. } => {
                tenant::validate_name("profile", profile)?;
            }
            PendingChange::ProfileFile {
                profile,
                file,
                snapshot,
            } => {
                tenant::validate_name("profile", profile)?;
                if !selected.agent.profile_files().contains(&file.as_str()) {
                    bail!("pending transaction names an unsupported Agent Profile file '{file}'");
                }
                if snapshot.present && snapshot.mode != Some(0o600) {
                    bail!("pending Agent Profile file must have mode 0600");
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
            write_snapshot(&selected.state_file(file), snapshot)
        }
        PendingChange::ProfileDirectory { profile, present } => {
            let path = selected.profile_dir(profile);
            if *present {
                ensure_profile_dir(selected, &path)
            } else {
                tenant::remove_real_dir_if_exists(&path, "Profile directory")
            }
        }
        PendingChange::ProfileFile {
            profile,
            file,
            snapshot,
        } => {
            let directory = selected.profile_dir(profile);
            ensure_profile_dir(selected, &directory)?;
            write_snapshot(&selected.profile_file(profile, file), snapshot)
        }
    }
}

fn ensure_profile_dir(selected: &TenantAgent, path: &Path) -> Result<()> {
    let existed = tenant::real_dir_exists(path, "Profile directory")?;
    tenant::ensure_real_dir(path, "Profile directory")?;
    if !existed {
        tenant::sync_dir(selected.metadata_dir())?;
    }
    Ok(())
}

fn profile_file_change(profile: &str, file: &str, content: &str, mode: u32) -> PendingChange {
    PendingChange::ProfileFile {
        profile: profile.to_string(),
        file: file.to_string(),
        snapshot: FileSnapshot {
            present: true,
            content: content.as_bytes().to_vec(),
            mode: Some(mode),
        },
    }
}

fn agent_file_changes(snapshots: &AgentFileSnapshots) -> Vec<PendingChange> {
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
        return tenant::remove_real_file_if_exists(path, "configuration path");
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
    let prefix = temporary_file_prefix(path, "write")?;
    let temp = write_temporary_file(parent, &prefix, content, mode.unwrap_or(0o644))?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("replace {}", path.display()))?;
    tenant::sync_dir(parent)
}

fn write_temporary_file(
    parent: &Path,
    prefix: &str,
    content: &[u8],
    mode: u32,
) -> Result<tempfile::NamedTempFile> {
    let mut temp = tempfile::Builder::new()
        .prefix(prefix)
        .tempfile_in(parent)
        .with_context(|| format!("create temporary file in {}", parent.display()))?;
    temp.write_all(content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    temp.as_file().sync_all()?;
    Ok(temp)
}

fn temporary_file_prefix(path: &Path, purpose: &str) -> Result<String> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .context("configuration file name is not valid UTF-8")?;
    Ok(format!(".{name}.aibox-{purpose}-"))
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
    let mut chars = input.chars();
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

fn confirm_delete(profile: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!(
            "refusing to delete Agent Profile '{profile}' without --yes in a non-interactive shell"
        );
    }
    eprint!("Delete Agent Profile '{profile}'? [y/N] ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim(), "y" | "Y" | "yes" | "YES"))
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
