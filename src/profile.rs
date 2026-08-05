//! Tenant-local Agent Profile catalog and command façade.

use crate::cli::{ProfileCommand, ProfileDiffArgs, ReconcileArgs};
use crate::profile_model::{
    self, Change, ChangeClass, ConflictChoice, DiffEntry, ProfileDefinition, PROFILE_METADATA_FILE,
};
use crate::profile_state::{
    agent_file_changes, capture_agent_files, commit_transaction, effective_from_snapshots,
    ensure_profile_exists, profile_definition_changes, profile_exists, profile_file_change,
    profile_files_are_regular, profile_owns_main, read_active_state, read_profile_definition,
    read_regular_bytes, read_regular_string, snapshots_from_effective, temporary_file_prefix,
    validate_private_file, write_temporary_file, ActiveProfileState, AgentFileSnapshots,
    PendingChange, SnapshotOptions,
};
use crate::tenant::{self, FileSnapshot, TenantAgent};
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::process::Command;

pub(crate) use crate::profile_model::Pointer;
pub(crate) use crate::profile_state::recover_pending;

struct Analysis {
    state: ActiveProfileState,
    source: ProfileDefinition,
    base_tree: serde_json::Value,
    working: ProfileDefinition,
    changes: Vec<Change>,
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
            let expected = profile_model::materialize(&base_tree, &state.applied)?;
            ensure_working_config_unchanged(selected, &current, &keys, &expected, &protected)?;
        }
        state.base.clone()
    } else {
        current.clone()
    };

    let base_tree = effective_from_snapshots(selected.agent, &base, &source.auth_keys())?;
    let mut desired_tree = profile_model::materialize(&base_tree, &source)?;
    if !protected.is_empty() {
        let current_tree = effective_from_snapshots(selected.agent, &current, &source.auth_keys())?;
        profile_model::copy_effective_paths(&current_tree, &mut desired_tree, &protected)?;
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
    profile_model::copy_effective_paths(&current_tree, &mut desired_tree, &protected)?;
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
    let working = profile_model::diff(&analysis.state.applied, &analysis.working);
    let source = profile_model::diff(&analysis.state.applied, &analysis.source);
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
            profile_model::display_node(entry.old.as_ref()),
            profile_model::display_node(entry.new.as_ref()),
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
    let result = profile_model::reconcile(
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
    let mut desired_tree = profile_model::materialize(&analysis.base_tree, &result.merged)?;
    let current_tree =
        effective_from_snapshots(selected.agent, &current, &result.merged.auth_keys())?;
    profile_model::copy_effective_paths(&current_tree, &mut desired_tree, &protected)?;
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
    let expected = profile_model::materialize(&base_tree, &state.applied)?;
    let current = capture_agent_files(selected)?;
    let mut working_tree = effective_from_snapshots(selected.agent, &current, &keys)?;
    let protected = crate::component::protected_config_paths(selected)?;
    profile_model::copy_effective_paths(&expected, &mut working_tree, &protected)?;
    let working = profile_model::working_definition(
        selected.agent,
        &state.applied,
        &expected,
        &working_tree,
    )?;
    let result = profile_model::reconcile(&state.applied, &working, &source, &BTreeMap::new())?;
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
    profile_model::materialize(&base, &state.applied)
}

fn ensure_working_config_unchanged(
    selected: &TenantAgent,
    current: &AgentFileSnapshots,
    auth_keys: &BTreeSet<String>,
    expected: &serde_json::Value,
    protected: &[Pointer],
) -> Result<()> {
    let mut working = effective_from_snapshots(selected.agent, current, auth_keys)?;
    profile_model::copy_effective_paths(expected, &mut working, protected)?;
    if working != *expected {
        bail!(
            "Agent Configuration has working changes; reconcile them or use --discard-config-changes"
        );
    }
    Ok(())
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
