//! Persistent provider overlays for `aibox config`.
//!
//! Providers are host-only snapshots. Applying one merges its TOML or JSON into
//! the selected profile's active agent configuration, replaces Codex
//! `auth.json` as a whole, and backs up existing managed files. The top-level
//! `aibox` table/object is reserved for apply metadata and stripped from active
//! output.

use crate::agent::AgentKind;
use crate::cli::ConfigCommand;
use crate::merge::{
    merge_json_with_apply_metadata, merge_toml_strings, parse_json_or_empty_object,
};
use crate::profile::{self, Profile};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const BACKUP_RETENTION: usize = 20;
const DEFAULT_CODEX_CONFIG_TEMPLATE: &[u8] = br#"approval_policy = "never"
sandbox_mode = "danger-full-access"
model_reasoning_effort = "xhigh"
plan_mode_reasoning_effort = "xhigh"
model = "gpt-5.6-sol"
# model_instructions_file = "~/prompts/gpt-5.5-base-instructions.md"
model_provider = "custom"

[model_providers.custom]
name = "example"
wire_api = "responses"
requires_openai_auth = true
base_url = "https://example.ai/v1"

# To remove fields from the active config when applying this provider:
# [aibox.config.apply]
# remove = ["model_provider", "model_providers.custom"]
"#;

const DEFAULT_CODEX_AUTH_TEMPLATE: &[u8] = br#"{
  "OPENAI_API_KEY": "sk-example"
}
"#;

const DEFAULT_CLAUDE_SETTINGS_TEMPLATE: &[u8] = br#"{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "sk-example",
    "ANTHROPIC_BASE_URL": "https://example.ai",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-opus-5",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-opus-5[1m]",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-5[1m]",
    "ANTHROPIC_DEFAULT_FABLE_MODEL": "claude-fable-5[1m]"
  },
  "statusLine": {
    "type": "command",
    "command": "bash ~/.claude/statusline.sh"
  },
  "skipDangerousModePermissionPrompt": true,
  "permissions": {
    "defaultMode": "bypassPermissions"
  }
}
"#;

/// One row returned by provider listing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderListEntry {
    /// Valid provider directory name.
    pub name: String,
    /// Whether this provider was the last one applied successfully.
    pub last_applied: bool,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct State {
    last_applied: Option<String>,
    last_applied_at: Option<u64>,
}

/// Execute one parsed provider command for a resolved profile.
///
/// `agent` must match [`Profile::agent`]; it is accepted separately because
/// the parsed command dispatcher validates agent-specific CLI flags.
pub fn dispatch(agent: AgentKind, profile: &Profile, command: &ConfigCommand) -> Result<i32> {
    match command {
        ConfigCommand::List => {
            for provider in list_providers(profile)? {
                let marker = if provider.last_applied { "*" } else { " " };
                if !crate::print_line(&format!("{marker} {}", provider.name))? {
                    break;
                }
            }
        }
        ConfigCommand::Get { provider, .. } => {
            crate::print_text(&get_provider(profile, provider)?)?;
        }
        ConfigCommand::Create { provider, .. } => {
            create_provider(profile, provider)?;
        }
        ConfigCommand::Apply { provider, .. } => {
            apply_provider(profile, provider)?;
        }
        ConfigCommand::Edit { provider, auth, .. } => {
            if *auth && agent.auth_file().is_none() {
                bail!("{} does not have an auth file", agent.tag());
            }
            edit_provider(profile, provider, *auth)?;
        }
        ConfigCommand::Delete {
            providers,
            all,
            yes,
            ..
        } => {
            delete_providers(profile, providers, *all, *yes)?;
        }
    }
    Ok(0)
}

/// Create a provider directory containing the selected agent's templates.
///
/// Existing providers are never overwritten. For an ordinary profile this
/// also finishes initializing the shared home and both agents' management
/// directories; for the host profile it creates only host-side provider
/// metadata.
pub fn create_provider(profile: &Profile, provider: &str) -> Result<()> {
    create_provider_with_templates(profile, provider, write_provider_templates)
}

fn create_provider_with_templates(
    profile: &Profile,
    provider: &str,
    write_templates: impl FnOnce(AgentKind, &Path) -> Result<()>,
) -> Result<()> {
    profile::validate_name("provider", provider)?;
    let provider_dir = profile.provider_dir(provider);
    if profile.management_dir_exists()? {
        match fs::symlink_metadata(&provider_dir) {
            Ok(_) => bail!("provider '{provider}' already exists"),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("inspect {}", provider_dir.display()));
            }
        }
    }
    profile.ensure_management_dir()?;
    fs::create_dir(&provider_dir)
        .with_context(|| format!("create provider directory {}", provider_dir.display()))?;

    if let Err(error) = write_templates(profile.agent, &provider_dir) {
        let _ = fs::remove_dir_all(&provider_dir);
        return Err(error);
    }
    Ok(())
}

fn write_provider_templates(agent: AgentKind, provider_dir: &Path) -> Result<()> {
    match agent {
        AgentKind::Codex => {
            atomic_write(
                &provider_dir.join("config.toml"),
                DEFAULT_CODEX_CONFIG_TEMPLATE,
                false,
            )?;
            atomic_write(
                &provider_dir.join("auth.json"),
                DEFAULT_CODEX_AUTH_TEMPLATE,
                true,
            )?;
        }
        AgentKind::Claude => {
            atomic_write(
                &provider_dir.join("settings.json"),
                DEFAULT_CLAUDE_SETTINGS_TEMPLATE,
                false,
            )?;
        }
    }
    Ok(())
}

/// List valid provider directories in name order.
pub fn list_providers(profile: &Profile) -> Result<Vec<ProviderListEntry>> {
    let state = read_state(profile)?;
    let providers = list_provider_names(profile)?
        .into_iter()
        .map(|name| ProviderListEntry {
            last_applied: state.last_applied.as_deref() == Some(&name),
            name,
        })
        .collect();
    Ok(providers)
}

fn list_provider_names(profile: &Profile) -> Result<Vec<String>> {
    let provider_root = profile.provider_root_dir();
    if !profile.management_dir_exists()? {
        return Ok(Vec::new());
    }

    let mut providers = Vec::new();
    for entry in fs::read_dir(&provider_root)
        .with_context(|| format!("read provider root {}", provider_root.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if profile::validate_name("provider", name).is_err() {
                continue;
            }
            providers.push(name.to_string());
        }
    }
    providers.sort();
    Ok(providers)
}

/// Render all managed files for one provider, with file-name headings.
///
/// Codex output includes `auth.json` and must be treated as secret.
pub fn get_provider(profile: &Profile, provider: &str) -> Result<String> {
    profile::validate_name("provider", provider)?;
    ensure_provider_exists(profile, provider)?;

    let mut output = String::new();
    for (index, file_name) in profile.agent.managed_config_files().iter().enumerate() {
        let path = profile.provider_file(provider, file_name);
        let content = read_required_string(&path)?;
        if index > 0 {
            output.push('\n');
        }
        output.push_str("# ");
        output.push_str(file_name);
        output.push('\n');
        output.push_str(&content);
        if !content.ends_with('\n') {
            output.push('\n');
        }
    }
    Ok(output)
}

/// Apply a provider to the active agent configuration.
///
/// Main configuration objects are deep-merged, while Codex `auth.json` is
/// validated and replaced as a whole. Existing managed files are backed up
/// before prepared replacements are committed. The main-config merge starts
/// from the current active file; applying a different provider does not reset
/// keys left by earlier applies.
pub fn apply_provider(profile: &Profile, provider: &str) -> Result<()> {
    profile::validate_name("provider", provider)?;
    ensure_provider_exists(profile, provider)?;
    profile.validate_existing_active_agent_dir()?;

    let mut writes = Vec::new();
    match profile.agent {
        AgentKind::Codex => {
            let provider_config =
                read_required_string(&profile.provider_file(provider, "config.toml"))?;
            let active_config = read_optional_regular_string(&profile.active_file("config.toml"))?;
            let merged_config = merge_toml_strings(&active_config, &provider_config)
                .with_context(|| format!("merge codex config for provider '{provider}'"))?;
            writes.push(PlannedWrite {
                path: profile.active_file("config.toml"),
                content: merged_config.into_bytes(),
                private: false,
            });

            let auth_path = profile.provider_file(provider, "auth.json");
            let provider_auth = read_required_string(&auth_path)?;
            validate_codex_auth(&provider_auth)
                .with_context(|| format!("validate {}", auth_path.display()))?;
            writes.push(PlannedWrite {
                path: profile.active_file("auth.json"),
                content: provider_auth.into_bytes(),
                private: true,
            });
        }
        AgentKind::Claude => {
            let settings_path = profile.provider_file(provider, "settings.json");
            let provider_settings = read_required_string(&settings_path)?;
            validate_claude_settings(&provider_settings)
                .with_context(|| format!("validate {}", settings_path.display()))?;
            let active_settings =
                read_optional_regular_string(&profile.active_file("settings.json"))?;
            let merged_settings =
                merge_json_object_strings(&active_settings, &provider_settings)
                    .with_context(|| format!("merge claude settings for provider '{provider}'"))?;
            writes.push(PlannedWrite {
                path: profile.active_file("settings.json"),
                content: merged_settings.into_bytes(),
                private: false,
            });
        }
    }

    let state = State {
        last_applied: Some(provider.to_string()),
        last_applied_at: Some(now_secs()?),
    };
    writes.push(planned_state_write(profile, &state)?);

    profile.ensure_active_agent_dir()?;
    let prepared_writes = prepare_writes(&writes)?;
    if let Err(error) = create_backup(profile) {
        cleanup_prepared_writes(&prepared_writes);
        return Err(error);
    }
    commit_prepared_writes(prepared_writes)?;
    Ok(())
}

/// Open a provider's main config, or its Codex auth file, in the configured
/// editor.
pub fn edit_provider(profile: &Profile, provider: &str, edit_auth: bool) -> Result<()> {
    profile::validate_name("provider", provider)?;
    ensure_provider_exists(profile, provider)?;

    if edit_auth && profile.agent.auth_file().is_none() {
        bail!("{} does not have an auth file", profile.agent.tag());
    }

    let file_name = if edit_auth {
        profile.agent.auth_file().expect("auth file checked above")
    } else {
        profile.agent.main_config_file()
    };
    let path = profile.provider_file(provider, file_name);
    if !path.exists() {
        let initial_content = if edit_auth || profile.agent == AgentKind::Claude {
            "{}\n"
        } else {
            ""
        };
        atomic_write(&path, initial_content.as_bytes(), edit_auth)?;
    }
    ensure_regular_file(&path)?;
    if edit_auth {
        profile::set_600(&path)?;
    }

    let editor = configured_editor();
    let status = editor_command(&editor)?
        .arg(&path)
        .status()
        .with_context(|| format!("run editor {editor:?}"))?;
    if !status.success() {
        bail!("editor exited with status {status}");
    }
    if edit_auth {
        profile::set_600(&path)?;
    }
    Ok(())
}

/// Delete one provider, optionally skipping confirmation.
///
/// This does not revert configuration already applied to the active agent
/// directory.
pub fn delete_provider(profile: &Profile, provider: &str, yes: bool) -> Result<()> {
    delete_providers(profile, &[provider.to_string()], false, yes)
}

/// Delete selected providers, or every provider when `all` or an empty slice
/// selects all.
///
/// Provider names and `all` are mutually exclusive. Unless `yes` is set, each
/// target requires interactive confirmation.
pub fn delete_providers(
    profile: &Profile,
    providers: &[String],
    all: bool,
    yes: bool,
) -> Result<()> {
    let targets = delete_provider_targets(profile, providers, all)?;
    if targets.is_empty() {
        eprintln!(">> no providers in this profile");
        return Ok(());
    }
    let deleting_every_provider = list_provider_names(profile)?.len() == targets.len();

    if !yes {
        for provider in &targets {
            if !confirm_delete(provider)? {
                bail!("aborted");
            }
        }
    }

    let state = read_state_for_delete(profile)?;
    let last_applied = state
        .as_ref()
        .and_then(|state| state.last_applied.as_deref());
    let clear_state = state.is_none()
        || deleting_every_provider
        || last_applied.is_some_and(|last| targets.iter().any(|provider| provider == last));
    for provider in &targets {
        let provider_dir = profile.provider_dir(provider);
        fs::remove_dir_all(&provider_dir)
            .with_context(|| format!("delete provider directory {}", provider_dir.display()))?;
    }
    if clear_state {
        remove_state_file(profile)?;
    }
    Ok(())
}

fn delete_provider_targets(
    profile: &Profile,
    providers: &[String],
    all: bool,
) -> Result<Vec<String>> {
    if all && !providers.is_empty() {
        bail!("--all cannot be combined with provider names");
    }

    if all || providers.is_empty() {
        return list_provider_names(profile);
    }

    let mut targets = Vec::new();
    for provider in providers {
        profile::validate_name("provider", provider)?;
        ensure_provider_exists(profile, provider)?;
        if !targets.contains(provider) {
            targets.push(provider.to_string());
        }
    }
    Ok(targets)
}

struct PlannedWrite {
    path: PathBuf,
    content: Vec<u8>,
    private: bool,
}

#[derive(Clone)]
struct PreparedWrite {
    path: PathBuf,
    temp_path: PathBuf,
}

fn ensure_provider_exists(profile: &Profile, provider: &str) -> Result<()> {
    if !profile.management_dir_exists()? {
        bail!("provider '{provider}' does not exist");
    }
    let provider_dir = profile.provider_dir(provider);
    match fs::symlink_metadata(&provider_dir) {
        Ok(meta) if meta.file_type().is_dir() => Ok(()),
        Ok(_) => bail!("provider '{provider}' is not a real directory"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            bail!("provider '{provider}' does not exist")
        }
        Err(error) => Err(error).with_context(|| format!("inspect {}", provider_dir.display())),
    }
}

fn read_required_string(path: &Path) -> Result<String> {
    read_existing_regular_string(path)
}

fn read_optional_regular_string(path: &Path) -> Result<String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => read_existing_regular_string(path),
        Ok(_) => bail!("{} is not a regular file", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
    }
}

fn read_existing_regular_string(path: &Path) -> Result<String> {
    let mut file = profile::open_real_file(path, "config file")?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .with_context(|| format!("read {}", path.display()))?;
    Ok(content)
}

fn ensure_regular_file(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => Ok(()),
        Ok(_) => bail!("{} is not a regular file", path.display()),
        Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
    }
}

fn merge_json_object_strings(base: &str, overlay: &str) -> Result<String> {
    let mut base_value = parse_json_or_empty_object(base)?;
    ensure_json_object(&base_value, "active settings")?;
    let overlay_value = parse_json_or_empty_object(overlay)?;
    ensure_json_object(&overlay_value, "provider settings")?;
    merge_json_with_apply_metadata(&mut base_value, overlay_value)?;
    Ok(format!("{}\n", serde_json::to_string_pretty(&base_value)?))
}

fn ensure_json_object(value: &JsonValue, label: &str) -> Result<()> {
    if value.is_object() {
        Ok(())
    } else {
        bail!("{label} must be a JSON object")
    }
}

fn validate_codex_auth(content: &str) -> Result<()> {
    let value: JsonValue = serde_json::from_str(content)?;
    match value {
        JsonValue::Object(map) if !map.is_empty() => {
            reject_placeholder_credentials(map.values(), "codex auth.json")?;
            Ok(())
        }
        JsonValue::Object(_) => bail!("codex auth.json must not be an empty object"),
        _ => bail!("codex auth.json must be a JSON object"),
    }
}

fn validate_claude_settings(content: &str) -> Result<()> {
    let value = parse_json_or_empty_object(content)?;
    ensure_json_object(&value, "provider settings")?;
    let values = value
        .as_object()
        .expect("provider settings object checked above")
        .values();
    reject_placeholder_credentials(values, "claude settings.json")
}

fn reject_placeholder_credentials<'a>(
    values: impl IntoIterator<Item = &'a JsonValue>,
    label: &str,
) -> Result<()> {
    if values.into_iter().any(contains_placeholder_secret) {
        bail!("{label} still contains placeholder credentials; edit it before applying")
    }
    Ok(())
}

fn contains_placeholder_secret(value: &JsonValue) -> bool {
    match value {
        JsonValue::String(value) => value.trim() == "sk-example",
        JsonValue::Array(values) => values.iter().any(contains_placeholder_secret),
        JsonValue::Object(values) => values.values().any(contains_placeholder_secret),
        _ => false,
    }
}

fn create_backup(profile: &Profile) -> Result<()> {
    profile::ensure_real_dir(&profile.backups_dir(), "backups directory")?;
    let backup_dir = create_unique_backup_dir(&profile.backups_dir())?;
    let copied = match copy_active_files_to_backup(profile, &backup_dir) {
        Ok(copied) => copied,
        Err(error) => {
            let _ = fs::remove_dir_all(&backup_dir);
            return Err(error);
        }
    };
    if copied == 0 {
        let _ = fs::remove_dir_all(&backup_dir);
        return Ok(());
    }
    prune_backups(&profile.backups_dir(), BACKUP_RETENTION)?;
    Ok(())
}

fn copy_active_files_to_backup(profile: &Profile, backup_dir: &Path) -> Result<usize> {
    let mut copied = 0;
    for file_name in profile.agent.managed_config_files() {
        let source = profile.active_file(file_name);
        match fs::symlink_metadata(&source) {
            Ok(meta) if meta.file_type().is_file() => {}
            Ok(_) => bail!("{} is not a regular file", source.display()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("inspect {}", source.display()))
            }
        }
        let destination = backup_dir.join(file_name);
        let private = Some(*file_name) == profile.agent.auth_file();
        copy_regular_file(&source, &destination, private)
            .with_context(|| format!("backup {} to {}", source.display(), destination.display()))?;
        if private {
            profile::set_600(&destination)?;
        }
        copied += 1;
    }
    Ok(copied)
}

fn create_unique_backup_dir(backups_dir: &Path) -> Result<PathBuf> {
    for attempt in 0..100 {
        let candidate = backups_dir.join(format!("{:020}-{attempt:03}", now_nanos()?));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("create backup directory {}", candidate.display()));
            }
        }
    }
    bail!("could not allocate a unique backup directory")
}

fn prune_backups(backups_dir: &Path, keep: usize) -> Result<()> {
    let mut backup_dirs = Vec::new();
    for entry in fs::read_dir(backups_dir)
        .with_context(|| format!("read backups directory {}", backups_dir.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_dir() && is_managed_backup_dir_name(&entry.file_name()) {
            backup_dirs.push(entry.path());
        }
    }

    backup_dirs.sort();
    let remove_count = backup_dirs.len().saturating_sub(keep);
    for path in backup_dirs.into_iter().take(remove_count) {
        fs::remove_dir_all(&path)
            .with_context(|| format!("prune backup directory {}", path.display()))?;
    }
    Ok(())
}

fn is_managed_backup_dir_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    let Some((timestamp, attempt)) = name.split_once('-') else {
        return false;
    };
    !timestamp.is_empty()
        && !attempt.is_empty()
        && timestamp.bytes().all(|byte| byte.is_ascii_digit())
        && attempt.bytes().all(|byte| byte.is_ascii_digit())
}

fn read_state(profile: &Profile) -> Result<State> {
    let path = profile.state_path();
    match read_optional_regular_string(&path) {
        Ok(content) if content.trim().is_empty() => Ok(State::default()),
        Ok(content) => serde_json::from_str(&content)
            .with_context(|| format!("parse state file {}", path.display())),
        Err(error) => Err(error),
    }
}

fn read_state_for_delete(profile: &Profile) -> Result<Option<State>> {
    let path = profile.state_path();
    match read_optional_regular_string(&path) {
        Ok(content) if content.trim().is_empty() => Ok(Some(State::default())),
        Ok(content) => match serde_json::from_str(&content) {
            Ok(state) => Ok(Some(state)),
            Err(_) => Ok(None),
        },
        Err(error) => Err(error),
    }
}

fn planned_state_write(profile: &Profile, state: &State) -> Result<PlannedWrite> {
    let content = format!("{}\n", serde_json::to_string_pretty(state)?);
    Ok(PlannedWrite {
        path: profile.state_path(),
        content: content.into_bytes(),
        private: false,
    })
}

fn remove_state_file(profile: &Profile) -> Result<()> {
    let path = profile.state_path();
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("delete {}", path.display())),
    }
}

fn atomic_write(path: &Path, content: &[u8], private: bool) -> Result<()> {
    let write = PlannedWrite {
        path: path.to_path_buf(),
        content: content.to_vec(),
        private,
    };
    let prepared_writes = prepare_writes(std::slice::from_ref(&write))?;
    commit_prepared_writes(prepared_writes)
}

fn prepare_writes(writes: &[PlannedWrite]) -> Result<Vec<PreparedWrite>> {
    let mut prepared_writes = Vec::with_capacity(writes.len());
    for write in writes {
        match prepare_write(write) {
            Ok(prepared_write) => prepared_writes.push(prepared_write),
            Err(error) => {
                cleanup_prepared_writes(&prepared_writes);
                return Err(error);
            }
        }
    }
    Ok(prepared_writes)
}

fn prepare_write(write: &PlannedWrite) -> Result<PreparedWrite> {
    let parent = write
        .path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .with_context(|| format!("{} has no parent directory", write.path.display()))?;
    profile::ensure_real_dir(parent, "config parent directory")?;

    let replacement_permissions = replacement_permissions(&write.path, write.private)?;
    let file_name = path_file_name(&write.path)?;
    let temp_path = write.path.with_file_name(format!(
        ".{file_name}.aibox-tmp-{}-{}",
        std::process::id(),
        now_nanos()?
    ));

    if let Err(error) = write_new_file(
        &temp_path,
        &write.content,
        write.private,
        replacement_permissions,
    ) {
        let _ = fs::remove_file(&temp_path);
        return Err(error).with_context(|| format!("write {}", temp_path.display()));
    }

    Ok(PreparedWrite {
        path: write.path.clone(),
        temp_path,
    })
}

fn commit_prepared_writes(prepared_writes: Vec<PreparedWrite>) -> Result<()> {
    for (index, write) in prepared_writes.iter().enumerate() {
        if let Err(error) = fs::rename(&write.temp_path, &write.path) {
            cleanup_prepared_writes(&prepared_writes[index..]);
            return Err(error).with_context(|| format!("replace {}", write.path.display()));
        }
    }
    Ok(())
}

fn cleanup_prepared_writes(writes: &[PreparedWrite]) {
    for write in writes {
        let _ = fs::remove_file(&write.temp_path);
    }
}

fn replacement_permissions(path: &Path, private: bool) -> Result<Option<fs::Permissions>> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => {
            if private {
                Ok(None)
            } else {
                Ok(Some(meta.permissions()))
            }
        }
        Ok(_) => bail!("{} is not a regular file", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(error).with_context(|| format!("read permissions for {}", path.display()))
        }
    }
}

fn write_new_file(
    path: &Path,
    content: &[u8],
    private: bool,
    permissions: Option<fs::Permissions>,
) -> Result<()> {
    let mut file = create_new_file(path, private)?;
    file.write_all(content)?;
    if let Some(permissions) = permissions {
        file.set_permissions(permissions)?;
    }
    Ok(())
}

fn create_new_file(path: &Path, private: bool) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if private { 0o600 } else { 0o644 });
    }
    let file = options.open(path)?;
    #[cfg(not(unix))]
    let _ = private;
    Ok(file)
}

fn copy_regular_file(source: &Path, destination: &Path, private: bool) -> Result<()> {
    let mut source_file = profile::open_real_file(source, "active config file")?;
    let permissions = if private {
        None
    } else {
        Some(
            source_file
                .metadata()
                .with_context(|| format!("inspect {}", source.display()))?
                .permissions(),
        )
    };
    let mut destination_file = create_new_file(destination, private)?;
    if let Some(permissions) = permissions {
        if let Err(error) = destination_file.set_permissions(permissions) {
            let _ = fs::remove_file(destination);
            return Err(error)
                .with_context(|| format!("set permissions on {}", destination.display()));
        }
    }
    if let Err(error) = io::copy(&mut source_file, &mut destination_file) {
        let _ = fs::remove_file(destination);
        return Err(error.into());
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
            Some(_) => unreachable!("only single and double quotes are used"),
            None => match character {
                '\'' | '"' => {
                    quote = Some(character);
                    in_word = true;
                }
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
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
        bail!("refusing to delete provider '{provider}' without --yes in a non-interactive shell");
    }

    eprint!("Delete provider '{provider}'? [y/N] ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim(), "y" | "Y" | "yes" | "YES"))
}

fn now_secs() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before unix epoch")?
        .as_secs())
}

fn now_nanos() -> Result<u128> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time is before unix epoch")?
        .as_nanos())
}

fn path_file_name(path: &Path) -> Result<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .context("path has no valid UTF-8 file name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn profile(root: &Path, agent: AgentKind) -> Profile {
        Profile::resolve(agent, root, "default").unwrap()
    }

    #[test]
    fn create_list_and_get_codex_provider() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);

        create_provider(&p, "openai").unwrap();
        let provider = p.provider_dir("openai");

        let codex_config = fs::read_to_string(provider.join("config.toml")).unwrap();
        assert!(codex_config.starts_with(
            r#"approval_policy = "never"
sandbox_mode = "danger-full-access"
model_reasoning_effort = "xhigh"
plan_mode_reasoning_effort = "xhigh"
model = "gpt-5.6-sol"
"#
        ));
        assert!(codex_config.contains("requires_openai_auth = true"));
        assert!(!codex_config.contains("[model_providers]\n"));
        assert_eq!(
            fs::read_to_string(provider.join("auth.json")).unwrap(),
            "{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n"
        );
        assert!(root.path().join("default/home/.codex").is_dir());
        assert!(root
            .path()
            .join("default/home/.claude/statusline.sh")
            .is_file());
        assert!(root.path().join("default/home/.gitconfig").is_file());
        assert!(root.path().join("default/config/claude").is_dir());
        profile::ensure_real_dir(&p.backups_dir(), "backup directory").unwrap();
        fs::write(p.state_path(), "{}\n").unwrap();
        assert_eq!(
            list_providers(&p).unwrap(),
            vec![ProviderListEntry {
                name: "openai".to_string(),
                last_applied: false
            }]
        );
        let details = get_provider(&p, "openai").unwrap();
        assert!(details.contains("# config.toml\n"));
        assert!(details.contains("# auth.json\n{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n"));
    }

    #[test]
    fn get_provider_requires_every_managed_file() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::remove_file(p.provider_file("openai", "auth.json")).unwrap();

        let err = get_provider(&p, "openai").unwrap_err().to_string();

        assert!(err.contains("auth.json"), "{err}");
    }

    #[test]
    fn create_claude_provider_includes_env_and_statusline_template() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Claude);

        create_provider(&p, "anthropic").unwrap();

        let settings: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(p.provider_file("anthropic", "settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-example");
        assert_eq!(
            settings["env"]["ANTHROPIC_DEFAULT_SONNET_MODEL"],
            "claude-opus-5[1m]"
        );
        assert_eq!(
            settings["statusLine"]["command"],
            "bash ~/.claude/statusline.sh"
        );
        assert_eq!(settings["skipDangerousModePermissionPrompt"], true);
        assert_eq!(settings["permissions"]["defaultMode"], "bypassPermissions");
        assert_eq!(
            settings["permissions"]
                .as_object()
                .unwrap()
                .get("skipDangerousModePermissionPrompt"),
            None
        );
        assert!(get_provider(&p, "anthropic")
            .unwrap()
            .contains(r#""ANTHROPIC_BASE_URL": "https://example.ai""#));
        assert!(root.path().join("default/home/.codex").is_dir());
        assert!(root
            .path()
            .join("default/home/.claude/statusline.sh")
            .is_file());
        assert!(root.path().join("default/home/.gitconfig").is_file());
        assert!(root.path().join("default/config/codex").is_dir());
    }

    #[test]
    fn create_provider_removes_partial_directory_when_template_write_fails() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);

        let err = create_provider_with_templates(&p, "broken", |_, provider_dir| {
            fs::write(provider_dir.join("config.toml"), "partial\n").unwrap();
            anyhow::bail!("template failed")
        })
        .unwrap_err()
        .to_string();

        assert!(err.contains("template failed"), "{err}");
        assert!(
            !p.provider_dir("broken").exists(),
            "a failed create must not leave a half-created provider"
        );

        create_provider(&p, "broken").unwrap();
        assert!(p.provider_file("broken", "config.toml").is_file());
        assert!(p.provider_file("broken", "auth.json").is_file());
    }

    #[test]
    fn corrupt_state_file_is_reported_instead_of_ignored() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::write(p.state_path(), "{not json").unwrap();

        let err = list_providers(&p).unwrap_err().to_string();

        assert!(err.contains("parse state file"), "{err}");
        assert!(err.contains(".state.json"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn provider_commands_reject_symlinked_profile_config() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("default")).unwrap();
        symlink(outside.path(), root.path().join("default/config")).unwrap();
        let p = profile(root.path(), AgentKind::Codex);

        let err = create_provider(&p, "openai").unwrap_err().to_string();
        assert!(
            err.contains("profile layout entry is not a real directory"),
            "{err}"
        );
        assert!(
            !outside.path().join("codex").exists(),
            "provider create must not write through a symlinked profile config"
        );

        fs::create_dir_all(outside.path().join("codex/openai")).unwrap();
        fs::write(
            outside.path().join("codex/openai/config.toml"),
            "model = \"outside\"\n",
        )
        .unwrap();
        fs::write(
            outside.path().join("codex/openai/auth.json"),
            "{\"token\":\"outside\"}\n",
        )
        .unwrap();

        let err = list_providers(&p).unwrap_err().to_string();
        assert!(
            err.contains("profile layout entry is not a real directory"),
            "{err}"
        );
        let err = get_provider(&p, "openai").unwrap_err().to_string();
        assert!(
            err.contains("profile layout entry is not a real directory"),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn provider_config_file_symlink_is_rejected() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), "model = \"outside\"\n").unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::remove_file(p.provider_file("openai", "config.toml")).unwrap();
        symlink(outside.path(), p.provider_file("openai", "config.toml")).unwrap();

        let err = get_provider(&p, "openai").unwrap_err().to_string();

        assert!(err.contains("config file is not a regular file"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn edit_provider_rejects_symlinked_config_without_launching_editor() {
        use std::os::unix::fs::symlink;

        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), "model = \"outside\"\n").unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::remove_file(p.provider_file("openai", "config.toml")).unwrap();
        symlink(outside.path(), p.provider_file("openai", "config.toml")).unwrap();

        let editor_dir = tempfile::tempdir().unwrap();
        let editor_log = editor_dir.path().join("editor.log");
        let editor = crate::testutil::write_stub_script(
            editor_dir.path(),
            "editor",
            r#"#!/bin/sh
printf 'called\n' > "$AIBOX_FAKE_EDITOR_LOG"
printf 'model = "edited"\n' > "$1"
"#,
        );
        let _visual = crate::testutil::EnvGuard::set("VISUAL", editor.as_os_str());
        let _editor = crate::testutil::EnvGuard::remove("EDITOR");
        let _log = crate::testutil::EnvGuard::set("AIBOX_FAKE_EDITOR_LOG", editor_log.as_os_str());

        let err = edit_provider(&p, "openai", false).unwrap_err().to_string();

        assert!(err.contains("config.toml is not a regular file"), "{err}");
        assert!(
            !editor_log.exists(),
            "a symlinked provider file must be rejected before launching the editor"
        );
        assert_eq!(
            fs::read_to_string(outside.path()).unwrap(),
            "model = \"outside\"\n"
        );
        assert!(
            fs::symlink_metadata(p.provider_file("openai", "config.toml"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn codex_apply_rejects_symlinked_provider_auth_without_touching_active_files() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), r#"{"token":"outside"}"#).unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::write(
            p.provider_file("openai", "config.toml"),
            "model = \"new\"\n",
        )
        .unwrap();
        fs::remove_file(p.provider_file("openai", "auth.json")).unwrap();
        symlink(outside.path(), p.provider_file("openai", "auth.json")).unwrap();
        profile::ensure_real_dir(&p.active_agent_dir, "active").unwrap();
        fs::write(p.active_file("config.toml"), "model = \"old\"\n").unwrap();
        fs::write(p.active_file("auth.json"), r#"{"token":"old"}"#).unwrap();

        let err = apply_provider(&p, "openai").unwrap_err().to_string();

        assert!(err.contains("config file is not a regular file"), "{err}");
        assert!(err.contains("auth.json"), "{err}");
        assert_eq!(
            fs::read_to_string(p.active_file("config.toml")).unwrap(),
            "model = \"old\"\n"
        );
        assert_eq!(
            fs::read_to_string(p.active_file("auth.json")).unwrap(),
            r#"{"token":"old"}"#
        );
        assert_eq!(
            fs::read_to_string(outside.path()).unwrap(),
            r#"{"token":"outside"}"#
        );
        assert!(
            !p.state_path().exists(),
            "failed apply must not mark a symlinked provider auth as applied"
        );
    }

    #[cfg(unix)]
    #[test]
    fn codex_apply_rejects_symlinked_provider_config_without_touching_active_files() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside_config = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside_config.path(), "model = \"outside\"\n").unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::remove_file(p.provider_file("openai", "config.toml")).unwrap();
        symlink(
            outside_config.path(),
            p.provider_file("openai", "config.toml"),
        )
        .unwrap();
        fs::write(p.provider_file("openai", "auth.json"), r#"{"token":"new"}"#).unwrap();
        profile::ensure_real_dir(&p.active_agent_dir, "active").unwrap();
        fs::write(p.active_file("config.toml"), "model = \"old\"\n").unwrap();
        fs::write(p.active_file("auth.json"), r#"{"token":"old"}"#).unwrap();

        let err = format!("{:#}", apply_provider(&p, "openai").unwrap_err());

        assert!(err.contains("config file is not a regular file"), "{err}");
        assert!(err.contains("config.toml"), "{err}");
        assert_eq!(
            fs::read_to_string(p.active_file("config.toml")).unwrap(),
            "model = \"old\"\n"
        );
        assert_eq!(
            fs::read_to_string(p.active_file("auth.json")).unwrap(),
            r#"{"token":"old"}"#
        );
        assert_eq!(
            fs::read_to_string(outside_config.path()).unwrap(),
            "model = \"outside\"\n"
        );
        assert!(!p.state_path().exists());
        assert!(
            !p.backups_dir().exists(),
            "failed provider reads must not create a misleading backup"
        );
    }

    #[test]
    fn delete_providers_accepts_many_and_clears_last_applied_state() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        create_provider(&p, "anthropic").unwrap();
        create_provider(&p, "local").unwrap();
        fs::write(p.state_path(), r#"{"last_applied":"anthropic"}"#).unwrap();

        delete_providers(
            &p,
            &["openai".to_string(), "anthropic".to_string()],
            false,
            true,
        )
        .unwrap();

        assert!(!p.provider_dir("openai").exists());
        assert!(!p.provider_dir("anthropic").exists());
        assert!(p.provider_dir("local").exists());
        assert!(!p.state_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn delete_provider_rejects_symlinked_provider_dir_without_touching_target() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::remove_dir_all(p.provider_dir("openai")).unwrap();
        fs::write(outside.path().join("config.toml"), "model = \"outside\"\n").unwrap();
        symlink(outside.path(), p.provider_dir("openai")).unwrap();

        let err = delete_provider(&p, "openai", true).unwrap_err().to_string();

        assert!(
            err.contains("provider 'openai' is not a real directory"),
            "{err}"
        );
        assert!(
            p.provider_dir("openai")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink(),
            "failed delete must not remove the provider symlink itself"
        );
        assert!(
            outside.path().join("config.toml").exists(),
            "failed delete must not follow the provider symlink"
        );
    }

    #[test]
    fn list_providers_marks_last_applied_provider() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        create_provider(&p, "anthropic").unwrap();
        fs::write(p.state_path(), r#"{"last_applied":"openai"}"#).unwrap();

        assert_eq!(
            list_providers(&p).unwrap(),
            vec![
                ProviderListEntry {
                    name: "anthropic".to_string(),
                    last_applied: false
                },
                ProviderListEntry {
                    name: "openai".to_string(),
                    last_applied: true
                }
            ]
        );
    }

    #[test]
    fn list_providers_ignores_management_metadata_and_invalid_entries() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "beta").unwrap();
        create_provider(&p, "alpha").unwrap();
        fs::write(p.state_path(), r#"{"last_applied":"alpha"}"#).unwrap();
        fs::create_dir(p.backups_dir()).unwrap();
        fs::create_dir(p.provider_root_dir().join("bad.name")).unwrap();
        fs::write(p.provider_root_dir().join("plain-file"), "not a provider\n").unwrap();

        assert_eq!(
            list_providers(&p).unwrap(),
            vec![
                ProviderListEntry {
                    name: "alpha".to_string(),
                    last_applied: true
                },
                ProviderListEntry {
                    name: "beta".to_string(),
                    last_applied: false
                }
            ]
        );
    }

    #[test]
    fn delete_providers_keeps_state_when_last_applied_provider_remains() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        create_provider(&p, "anthropic").unwrap();
        fs::write(p.state_path(), r#"{"last_applied":"anthropic"}"#).unwrap();

        delete_providers(&p, &["openai".to_string()], false, true).unwrap();

        assert!(!p.provider_dir("openai").exists());
        assert!(p.provider_dir("anthropic").exists());
        assert_eq!(
            fs::read_to_string(p.state_path()).unwrap(),
            r#"{"last_applied":"anthropic"}"#
        );
    }

    #[test]
    fn delete_providers_dedupes_repeated_names() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();

        delete_providers(
            &p,
            &["openai".to_string(), "openai".to_string()],
            false,
            true,
        )
        .unwrap();

        assert!(!p.provider_dir("openai").exists());
    }

    #[test]
    fn delete_provider_refuses_noninteractive_delete_without_yes() {
        if io::stdin().is_terminal() {
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();

        let err = delete_provider(&p, "openai", false)
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("without --yes in a non-interactive shell"),
            "{err}"
        );
        assert!(p.provider_dir("openai").exists());
    }

    #[test]
    fn delete_providers_clears_state_when_every_provider_is_deleted() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        create_provider(&p, "anthropic").unwrap();
        fs::write(p.state_path(), r#"{"last_applied":"stale"}"#).unwrap();

        delete_providers(
            &p,
            &["openai".to_string(), "anthropic".to_string()],
            false,
            true,
        )
        .unwrap();

        assert!(list_providers(&p).unwrap().is_empty());
        assert!(
            !p.state_path().exists(),
            "deleting the final provider must not leave a stale last-applied marker"
        );
    }

    #[test]
    fn delete_providers_recovers_from_corrupt_state_metadata() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        create_provider(&p, "anthropic").unwrap();
        fs::write(p.state_path(), "{not json").unwrap();

        delete_providers(&p, &["openai".to_string()], false, true).unwrap();

        assert!(!p.provider_dir("openai").exists());
        assert!(p.provider_dir("anthropic").exists());
        assert!(
            !p.state_path().exists(),
            "delete should clear corrupt last-applied metadata instead of blocking cleanup"
        );
    }

    #[test]
    fn delete_all_providers_does_not_require_valid_state_metadata() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        create_provider(&p, "anthropic").unwrap();
        fs::write(p.state_path(), "{not json").unwrap();

        delete_providers(&p, &[], true, true).unwrap();

        assert!(list_providers(&p).unwrap().is_empty());
        assert!(!p.state_path().exists());
    }

    #[test]
    fn delete_providers_empty_or_all_flag_selects_every_provider() {
        for (target, all) in [(Vec::new(), false), (Vec::new(), true)] {
            let root = tempfile::tempdir().unwrap();
            let p = profile(root.path(), AgentKind::Codex);
            create_provider(&p, "openai").unwrap();
            create_provider(&p, "anthropic").unwrap();

            delete_providers(&p, &target, all, true).unwrap();

            assert!(list_providers(&p).unwrap().is_empty());
        }
    }

    #[test]
    fn delete_providers_treats_all_as_a_provider_name_without_all_flag() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "all").unwrap();
        create_provider(&p, "openai").unwrap();

        delete_providers(&p, &["all".to_string()], false, true).unwrap();

        assert!(!p.provider_dir("all").exists());
        assert!(p.provider_dir("openai").exists());
    }

    #[test]
    fn delete_providers_resolves_every_name_before_deleting() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();

        let err = delete_providers(
            &p,
            &["openai".to_string(), "missing".to_string()],
            false,
            true,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("provider 'missing' does not exist"), "{err}");
        assert!(p.provider_dir("openai").exists());
    }

    #[test]
    fn delete_providers_rejects_all_flag_mixed_with_names() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();

        let err = delete_providers(&p, &["openai".to_string()], true, true)
            .unwrap_err()
            .to_string();

        assert!(err.contains("--all cannot be combined"), "{err}");
        assert!(p.provider_dir("openai").exists());
    }

    #[test]
    fn codex_apply_merges_config_replaces_auth_and_marks_state() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::write(
            p.provider_file("openai", "config.toml"),
            r#"
model = "provider"

[model_providers.openai]
base_url = "new"
"#,
        )
        .unwrap();
        fs::write(p.provider_file("openai", "auth.json"), r#"{"token":"new"}"#).unwrap();

        profile::ensure_real_dir(&p.active_agent_dir, "active").unwrap();
        fs::write(
            p.active_file("config.toml"),
            r#"
model = "common"

[model_providers.openai]
base_url = "old"
legacy = true
"#,
        )
        .unwrap();
        fs::write(p.active_file("auth.json"), r#"{"token":"old"}"#).unwrap();

        apply_provider(&p, "openai").unwrap();

        let merged = fs::read_to_string(p.active_file("config.toml")).unwrap();
        assert!(merged.contains(r#"model = "provider""#));
        assert!(merged.contains(r#"base_url = "new""#));
        assert!(merged.contains("legacy = true"));
        assert_eq!(
            fs::read_to_string(p.active_file("auth.json")).unwrap(),
            r#"{"token":"new"}"#
        );
        assert!(fs::read_to_string(p.state_path())
            .unwrap()
            .contains(r#""last_applied": "openai""#));
        assert_eq!(
            fs::read_to_string(
                fs::read_dir(p.backups_dir())
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path()
                    .join("auth.json")
            )
            .unwrap(),
            r#"{"token":"old"}"#
        );
    }

    #[test]
    fn codex_apply_removes_config_paths_and_strips_metadata() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::write(
            p.provider_file("openai", "config.toml"),
            r#"
model = "gpt-5"

[aibox.config.apply]
remove = ["model_provider", "model_providers.custom"]
"#,
        )
        .unwrap();
        fs::write(p.provider_file("openai", "auth.json"), r#"{"token":"new"}"#).unwrap();

        profile::ensure_real_dir(&p.active_agent_dir, "active").unwrap();
        fs::write(
            p.active_file("config.toml"),
            r#"
model_provider = "custom"
model = "old"

[model_providers.custom]
base_url = "old"
"#,
        )
        .unwrap();

        apply_provider(&p, "openai").unwrap();

        let merged = fs::read_to_string(p.active_file("config.toml")).unwrap();
        assert!(merged.contains(r#"model = "gpt-5""#));
        assert!(!merged.contains("model_provider"));
        assert!(!merged.contains("[model_providers.custom]"));
        assert!(!merged.contains("[aibox"));
    }

    #[test]
    fn claude_apply_deep_merges_settings() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Claude);
        create_provider(&p, "anthropic").unwrap();
        fs::write(
            p.provider_file("anthropic", "settings.json"),
            r#"{"model":"provider","nested":{"replace":["new"]}}"#,
        )
        .unwrap();

        profile::ensure_real_dir(&p.active_agent_dir, "active").unwrap();
        fs::write(
            p.active_file("settings.json"),
            r#"{"model":"common","nested":{"keep":true,"replace":["old"]}}"#,
        )
        .unwrap();

        apply_provider(&p, "anthropic").unwrap();

        let merged: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(p.active_file("settings.json")).unwrap())
                .unwrap();
        assert_eq!(merged["model"], "provider");
        assert_eq!(merged["nested"]["keep"], true);
        assert_eq!(merged["nested"]["replace"], serde_json::json!(["new"]));
    }

    #[test]
    fn claude_apply_creates_profile_and_installs_statusline() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Claude);
        create_provider(&p, "anthropic").unwrap();
        fs::write(
            p.provider_file("anthropic", "settings.json"),
            r#"{"statusLine":{"type":"command","command":"bash ~/.claude/statusline.sh"}}"#,
        )
        .unwrap();

        apply_provider(&p, "anthropic").unwrap();

        assert!(p.active_file("settings.json").is_file());
        assert!(p.active_file("statusline.sh").is_file());
        assert!(fs::read_to_string(p.active_file("statusline.sh"))
            .unwrap()
            .contains("context_window"));
    }

    #[test]
    fn claude_apply_removes_settings_paths_and_strips_metadata() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Claude);
        create_provider(&p, "anthropic").unwrap();
        fs::write(
            p.provider_file("anthropic", "settings.json"),
            r#"{"model":"provider","aibox":{"config":{"apply":{"remove":["old","nested.drop"]}}}}"#,
        )
        .unwrap();

        profile::ensure_real_dir(&p.active_agent_dir, "active").unwrap();
        fs::write(
            p.active_file("settings.json"),
            r#"{"model":"common","old":true,"nested":{"keep":true,"drop":true}}"#,
        )
        .unwrap();

        apply_provider(&p, "anthropic").unwrap();

        let merged: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(p.active_file("settings.json")).unwrap())
                .unwrap();
        assert_eq!(merged["model"], "provider");
        assert_eq!(merged["nested"]["keep"], true);
        assert!(merged.get("old").is_none());
        assert!(merged["nested"].get("drop").is_none());
        assert!(merged.get("aibox").is_none());
    }

    #[test]
    fn empty_codex_auth_is_invalid() {
        assert!(validate_codex_auth("{}").is_err());
        assert!(validate_codex_auth("[]").is_err());
        assert!(validate_codex_auth(r#"{"OPENAI_API_KEY":"sk-example"}"#).is_err());
        assert!(validate_codex_auth(r#"{"nested":{"token":"sk-example"}}"#).is_err());
        assert!(validate_codex_auth(r#"{"token":"x"}"#).is_ok());
    }

    #[test]
    fn codex_apply_rejects_unedited_auth_template_without_touching_active_files() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        profile::ensure_real_dir(&p.active_agent_dir, "active").unwrap();
        fs::write(p.active_file("config.toml"), "model = \"old\"\n").unwrap();
        fs::write(p.active_file("auth.json"), r#"{"token":"old"}"#).unwrap();

        let err = format!("{:#}", apply_provider(&p, "openai").unwrap_err());

        assert!(err.contains("placeholder credentials"), "{err}");
        assert_eq!(
            fs::read_to_string(p.active_file("config.toml")).unwrap(),
            "model = \"old\"\n"
        );
        assert_eq!(
            fs::read_to_string(p.active_file("auth.json")).unwrap(),
            r#"{"token":"old"}"#
        );
        assert!(
            !p.state_path().exists(),
            "failed apply must not mark the provider as applied"
        );
        assert!(
            !p.backups_dir().exists(),
            "failed apply must not create a misleading backup"
        );
    }

    #[test]
    fn failed_ordinary_apply_does_not_create_profile_home_from_manual_provider() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        fs::create_dir_all(p.provider_dir("openai")).unwrap();
        fs::write(
            p.provider_file("openai", "config.toml"),
            "model = \"new\"\n",
        )
        .unwrap();
        fs::write(
            p.provider_file("openai", "auth.json"),
            r#"{"OPENAI_API_KEY":"sk-example"}"#,
        )
        .unwrap();

        let err = format!("{:#}", apply_provider(&p, "openai").unwrap_err());

        assert!(err.contains("placeholder credentials"), "{err}");
        assert!(
            !p.home_dir.exists(),
            "failed validation must not create an ordinary profile home"
        );
        assert!(
            !p.state_path().exists(),
            "failed apply must not mark the provider as applied"
        );
    }

    #[cfg(unix)]
    #[test]
    fn codex_apply_rejects_symlinked_active_auth_during_backup_without_committing() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::write(
            p.provider_file("openai", "config.toml"),
            "model = \"new\"\n",
        )
        .unwrap();
        fs::write(p.provider_file("openai", "auth.json"), r#"{"token":"new"}"#).unwrap();
        fs::write(p.active_file("config.toml"), "model = \"old\"\n").unwrap();
        let outside_auth = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside_auth.path(), r#"{"token":"outside"}"#).unwrap();
        symlink(outside_auth.path(), p.active_file("auth.json")).unwrap();

        let err = format!("{:#}", apply_provider(&p, "openai").unwrap_err());

        assert!(err.contains("auth.json is not a regular file"), "{err}");
        assert_eq!(
            fs::read_to_string(p.active_file("config.toml")).unwrap(),
            "model = \"old\"\n"
        );
        assert!(
            fs::symlink_metadata(p.active_file("auth.json"))
                .unwrap()
                .file_type()
                .is_symlink(),
            "failed backup must not replace a symlinked active auth file"
        );
        assert_eq!(
            fs::read_to_string(outside_auth.path()).unwrap(),
            r#"{"token":"outside"}"#
        );
        assert!(
            !p.state_path().exists(),
            "failed backup must not mark the provider as applied"
        );
        let backup_entries = if p.backups_dir().exists() {
            fs::read_dir(p.backups_dir()).unwrap().count()
        } else {
            0
        };
        assert_eq!(backup_entries, 0, "failed backup must be cleaned up");
    }

    #[cfg(unix)]
    #[test]
    fn edit_provider_creates_missing_auth_privately_and_invokes_configured_editor() {
        use std::os::unix::fs::PermissionsExt;

        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        let auth = p.provider_file("openai", "auth.json");
        fs::remove_file(&auth).unwrap();

        let editor_dir = tempfile::tempdir().unwrap();
        let editor_log = editor_dir.path().join("editor.log");
        let editor = crate::testutil::write_stub_script(
            editor_dir.path(),
            "editor",
            r#"#!/bin/sh
log="$AIBOX_FAKE_EDITOR_LOG"
printf 'ARGS:' >> "$log"
for arg in "$@"; do
    printf ' <%s>' "$arg" >> "$log"
done
printf '\n' >> "$log"
printf '{"token":"edited"}\n' > "$1"
"#,
        );
        let _visual = crate::testutil::EnvGuard::set("VISUAL", editor.as_os_str());
        let _editor = crate::testutil::EnvGuard::remove("EDITOR");
        let _log = crate::testutil::EnvGuard::set("AIBOX_FAKE_EDITOR_LOG", editor_log.as_os_str());

        edit_provider(&p, "openai", true).unwrap();

        assert_eq!(
            fs::read_to_string(&auth).unwrap(),
            "{\"token\":\"edited\"}\n"
        );
        let mode = fs::metadata(&auth).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(
            fs::read_to_string(editor_log).unwrap(),
            format!("ARGS: <{}>\n", auth.display())
        );
    }

    #[test]
    fn edit_provider_rejects_auth_for_agents_without_auth_file() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Claude);
        create_provider(&p, "anthropic").unwrap();

        let err = edit_provider(&p, "anthropic", true)
            .unwrap_err()
            .to_string();

        assert!(err.contains("claude does not have an auth file"), "{err}");
    }

    #[test]
    fn editor_command_splits_common_shell_words_without_a_shell() {
        assert_eq!(
            split_shell_words(r#"code --wait "two words" escaped\ space 'literal "quote"'"#)
                .unwrap(),
            vec![
                "code",
                "--wait",
                "two words",
                "escaped space",
                "literal \"quote\""
            ]
        );

        let err = split_shell_words(r#""unterminated"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("unterminated quote"), "{err}");
    }

    #[test]
    fn failed_host_apply_does_not_create_active_agent_dir() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let host_home = tempfile::tempdir().unwrap();
        let _home = crate::testutil::EnvGuard::set("HOME", host_home.path().as_os_str());
        let p = Profile::resolve(AgentKind::Codex, root.path(), "host").unwrap();
        create_provider(&p, "openai").unwrap();

        let err = format!("{:#}", apply_provider(&p, "openai").unwrap_err());

        assert!(err.contains("placeholder credentials"), "{err}");
        assert!(
            !host_home.path().join(".codex").exists(),
            "failed validation must not create host-side active agent state"
        );
    }

    #[test]
    fn claude_apply_rejects_unedited_token_template_without_touching_active_settings() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Claude);
        create_provider(&p, "anthropic").unwrap();
        profile::ensure_real_dir(&p.active_agent_dir, "active").unwrap();
        fs::write(p.active_file("settings.json"), r#"{"model":"old"}"#).unwrap();

        let err = format!("{:#}", apply_provider(&p, "anthropic").unwrap_err());

        assert!(err.contains("placeholder credentials"), "{err}");
        assert_eq!(
            fs::read_to_string(p.active_file("settings.json")).unwrap(),
            r#"{"model":"old"}"#
        );
        assert!(
            !p.state_path().exists(),
            "failed apply must not mark the provider as applied"
        );
        assert!(
            !p.backups_dir().exists(),
            "failed apply must not create a misleading backup"
        );
    }

    #[test]
    fn backup_retention_keeps_latest_twenty() {
        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::write(p.provider_file("openai", "auth.json"), r#"{"token":"new"}"#).unwrap();

        profile::ensure_real_dir(&p.active_agent_dir, "active").unwrap();
        for index in 0..25 {
            fs::write(
                p.active_file("config.toml"),
                format!("model = \"{index}\"\n"),
            )
            .unwrap();
            fs::write(
                p.active_file("auth.json"),
                format!(r#"{{"token":"old-{index}"}}"#),
            )
            .unwrap();
            apply_provider(&p, "openai").unwrap();
        }

        let backups = fs::read_dir(p.backups_dir())
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(backups.len(), 20);
    }

    #[cfg(unix)]
    #[test]
    fn codex_apply_preserves_config_permissions_and_writes_auth_privately() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::write(
            p.provider_file("openai", "config.toml"),
            "model = \"new\"\n",
        )
        .unwrap();
        fs::write(p.provider_file("openai", "auth.json"), r#"{"token":"new"}"#).unwrap();

        profile::ensure_real_dir(&p.active_agent_dir, "active").unwrap();
        fs::write(p.active_file("config.toml"), "model = \"old\"\n").unwrap();
        fs::write(p.active_file("auth.json"), r#"{"token":"old"}"#).unwrap();
        fs::set_permissions(
            p.active_file("config.toml"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        fs::set_permissions(
            p.active_file("auth.json"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();

        apply_provider(&p, "openai").unwrap();

        let active_config_mode = fs::metadata(p.active_file("config.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            active_config_mode & 0o777,
            0o600,
            "atomic replacement must preserve an existing config file's mode"
        );
        let active_mode = fs::metadata(p.active_file("auth.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            active_mode & 0o777,
            0o600,
            "auth.json must be private even when the old active file was wider"
        );
        let backup = fs::read_dir(p.backups_dir())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let backup_config_mode = fs::metadata(backup.join("config.toml"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            backup_config_mode & 0o777,
            0o600,
            "non-secret backup files should preserve the active file's mode"
        );
        let backup_mode = fs::metadata(backup.join("auth.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            backup_mode & 0o777,
            0o600,
            "auth backups must be private regardless of the old active mode"
        );
    }

    #[cfg(unix)]
    #[test]
    fn active_file_symlink_is_rejected_before_apply() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let p = profile(root.path(), AgentKind::Codex);
        create_provider(&p, "openai").unwrap();
        fs::write(p.provider_file("openai", "auth.json"), r#"{"token":"new"}"#).unwrap();
        profile::ensure_real_dir(&p.active_agent_dir, "active").unwrap();
        symlink(outside.path(), p.active_file("config.toml")).unwrap();

        let err = apply_provider(&p, "openai").unwrap_err().to_string();
        assert!(err.contains("is not a regular file"), "{err}");
    }

    #[test]
    fn host_profile_apply_writes_real_home_and_backs_up_under_aibox_root() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let host_home = tempfile::tempdir().unwrap();
        let _home = crate::testutil::EnvGuard::set("HOME", host_home.path().as_os_str());
        let p = Profile::resolve(AgentKind::Codex, root.path(), "host").unwrap();

        create_provider(&p, "openai").unwrap();
        fs::write(
            p.provider_file("openai", "config.toml"),
            "model = \"new\"\n",
        )
        .unwrap();
        fs::write(p.provider_file("openai", "auth.json"), r#"{"token":"new"}"#).unwrap();
        profile::ensure_real_dir(&p.active_agent_dir, "active").unwrap();
        fs::write(p.active_file("config.toml"), "model = \"old\"\n").unwrap();
        fs::write(p.active_file("auth.json"), r#"{"token":"old"}"#).unwrap();

        apply_provider(&p, "openai").unwrap();

        assert_eq!(
            fs::read_to_string(host_home.path().join(".codex/config.toml")).unwrap(),
            "model = \"new\"\n"
        );
        assert!(!host_home.path().join(".gitconfig").exists());
        assert!(!host_home.path().join(".claude").exists());
        let backup = fs::read_dir(root.path().join("host/config/codex/.backup"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(
            fs::read_to_string(backup.join("auth.json")).unwrap(),
            r#"{"token":"old"}"#
        );
    }
}
