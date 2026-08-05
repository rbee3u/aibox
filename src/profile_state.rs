//! Persistent Agent Profile files, Active Profile state, and roll-forward
//! transactions.

use crate::agent::AgentKind;
use crate::profile_model::{ProfileDefinition, PROFILE_METADATA_FILE};
use crate::tenant::{self, FileSnapshot, Tenant, TenantAgent};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;
const MAX_STATE_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) type AgentFileSnapshots = BTreeMap<String, FileSnapshot>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ActiveProfileState {
    pub(crate) profile: String,
    pub(crate) base: AgentFileSnapshots,
    pub(crate) applied: ProfileDefinition,
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
pub(crate) enum PendingChange {
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

#[derive(Clone, Copy)]
pub(crate) struct SnapshotOptions {
    pub(crate) preserve_component_config: bool,
    pub(crate) restore_base_main_mode: bool,
}

pub(crate) fn read_profile_definition(
    selected: &TenantAgent,
    profile: &str,
) -> Result<ProfileDefinition> {
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

pub(crate) fn profile_definition_changes(
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

pub(crate) fn ensure_profile_exists(selected: &TenantAgent, profile: &str) -> Result<()> {
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

pub(crate) fn profile_exists(selected: &TenantAgent, profile: &str) -> Result<bool> {
    tenant::validate_name("profile", profile)?;
    if !selected.metadata_dir_exists()? {
        return Ok(false);
    }
    tenant::real_dir_exists(&selected.profile_dir(profile), "Profile directory")
}

pub(crate) fn profile_files_are_regular(selected: &TenantAgent, profile: &str) -> bool {
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

pub(crate) fn read_active_state(selected: &TenantAgent) -> Result<Option<ActiveProfileState>> {
    Ok(read_scope_metadata(selected)?.active_profile)
}

fn write_scope_metadata(selected: &TenantAgent, metadata: &ScopeMetadata) -> Result<()> {
    selected.ensure_for_management()?;
    let content = format!("{}\n", serde_json::to_string_pretty(metadata)?);
    write_atomic(&selected.metadata_file(), content.as_bytes(), Some(0o600))
}

pub(crate) fn capture_agent_files(selected: &TenantAgent) -> Result<AgentFileSnapshots> {
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

pub(crate) fn effective_from_snapshots(
    agent: AgentKind,
    snapshots: &AgentFileSnapshots,
    auth_keys: &BTreeSet<String>,
) -> Result<serde_json::Value> {
    let main = snapshot_text(snapshots, agent.main_config_file())?;
    let auth = agent
        .native_auth_file()
        .map(|name| snapshot_text(snapshots, name))
        .transpose()?;
    agent.normalize_config_files(&main, auth.as_deref(), auth_keys)
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

pub(crate) fn snapshots_from_effective(
    selected: &TenantAgent,
    tree: &serde_json::Value,
    current: &AgentFileSnapshots,
    base: &AgentFileSnapshots,
    profile: &ProfileDefinition,
    options: SnapshotOptions,
) -> Result<AgentFileSnapshots> {
    let (main, auth) = selected.agent.render_config_files(tree)?;
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

pub(crate) fn profile_owns_main(agent: AgentKind, profile: &ProfileDefinition) -> bool {
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

pub(crate) fn commit_transaction(
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

pub(crate) fn profile_file_change(
    profile: &str,
    file: &str,
    content: &str,
    mode: u32,
) -> PendingChange {
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

pub(crate) fn agent_file_changes(snapshots: &AgentFileSnapshots) -> Vec<PendingChange> {
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

pub(crate) fn read_regular_string(path: &Path) -> Result<String> {
    String::from_utf8(read_regular_bytes_with_limit(path, MAX_CONFIG_BYTES)?)
        .with_context(|| format!("{} is not valid UTF-8", path.display()))
}

pub(crate) fn read_regular_bytes(path: &Path) -> Result<Vec<u8>> {
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

pub(crate) fn validate_private_file(path: &Path) -> Result<()> {
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

pub(crate) fn write_temporary_file(
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

pub(crate) fn temporary_file_prefix(path: &Path, purpose: &str) -> Result<String> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .context("configuration file name is not valid UTF-8")?;
    Ok(format!(".{name}.aibox-{purpose}-"))
}

#[cfg(test)]
#[path = "profile_state_tests.rs"]
mod tests;
