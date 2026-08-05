//! Tenant-local Agent Profile catalog and one-time application commands.

use crate::cli::ProfileCommand;
use crate::profile_model::ProfileDefinition;
use crate::tenant::{self, FileSnapshot, TenantAgent};
use anyhow::{bail, Context, Result};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::Path;
use std::process::Command;

const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ProfileLayout {
    main: bool,
    auth: bool,
}

impl ProfileLayout {
    fn complete(self) -> bool {
        self.main && self.auth
    }
}

/// Execute one parsed Agent Profile command.
pub fn dispatch(selected: &TenantAgent, command: &ProfileCommand) -> Result<i32> {
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
        ProfileCommand::Apply { profile } => apply_profile(selected, profile)?,
    }
    Ok(0)
}

/// Create an Agent Profile from the selected Coding Agent's built-in template.
pub fn create_profile(selected: &TenantAgent, profile: &str) -> Result<()> {
    tenant::validate_name("profile", profile)?;
    selected.ensure_profile_catalog()?;

    let prospective_main;
    let prospective_auth;
    let layout = inspect_profile_directory(selected, profile)?;
    match layout {
        Some(layout) if layout.complete() => {
            bail!("Agent Profile '{profile}' already exists");
        }
        Some(layout) => {
            validate_private_directory(&selected.profile_dir(profile))?;
            prospective_main = if layout.main {
                let path = selected.profile_file(profile, selected.agent.main_config_file());
                validate_private_file(&path)?;
                read_regular_string(&path)?
            } else {
                selected.agent.profile_template().to_string()
            };
            prospective_auth = if layout.auth {
                let path = selected.profile_file(profile, selected.agent.profile_auth_file());
                validate_private_file(&path)?;
                read_regular_string(&path)?
            } else {
                selected.agent.profile_auth_template().to_string()
            };
            ProfileDefinition::parse(selected.agent, &prospective_main, &prospective_auth)
                .with_context(|| format!("validate incomplete Agent Profile '{profile}'"))?;
            if !layout.main {
                write_profile_file(
                    selected,
                    profile,
                    selected.agent.main_config_file(),
                    prospective_main.as_bytes(),
                )?;
            }
            if !layout.auth {
                write_profile_file(
                    selected,
                    profile,
                    selected.agent.profile_auth_file(),
                    prospective_auth.as_bytes(),
                )?;
            }
            return Ok(());
        }
        None => {
            prospective_main = selected.agent.profile_template().to_string();
            prospective_auth = selected.agent.profile_auth_template().to_string();
        }
    }

    ProfileDefinition::parse(selected.agent, &prospective_main, &prospective_auth)
        .context("validate built-in Agent Profile template")?;
    ensure_profile_directory(selected, profile)?;
    write_profile_file(
        selected,
        profile,
        selected.agent.main_config_file(),
        prospective_main.as_bytes(),
    )?;
    write_profile_file(
        selected,
        profile,
        selected.agent.profile_auth_file(),
        prospective_auth.as_bytes(),
    )
}

/// List complete, structurally safe Agent Profile names without parsing them.
pub fn list_profiles(selected: &TenantAgent) -> Result<Vec<String>> {
    if !selected.profile_catalog_exists()? {
        return Ok(Vec::new());
    }
    let root = selected.profile_catalog_dir();
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
        if tenant::validate_name("profile", &name).is_err() {
            continue;
        }
        let visible = inspect_profile_directory(selected, &name)
            .ok()
            .flatten()
            .is_some_and(ProfileLayout::complete)
            && private_directory(&selected.profile_dir(&name))
            && selected
                .agent
                .profile_files()
                .iter()
                .all(|file| private_regular_file(&selected.profile_file(&name, file)));
        if visible {
            profiles.push(name);
        }
    }
    profiles.sort();
    Ok(profiles)
}

/// Return one raw Agent Profile file, including invalid content for repair.
pub fn get_profile(selected: &TenantAgent, profile: &str, auth: bool) -> Result<String> {
    ensure_complete_profile(selected, profile)?;
    let file = if auth {
        selected.agent.profile_auth_file()
    } else {
        selected.agent.main_config_file()
    };
    read_regular_string(&selected.profile_file(profile, file))
}

/// Edit one Agent Profile file and commit it only when the full Profile is valid.
pub fn edit_profile(selected: &TenantAgent, profile: &str, auth: bool) -> Result<()> {
    ensure_complete_profile(selected, profile)?;
    let file = if auth {
        selected.agent.profile_auth_file()
    } else {
        selected.agent.main_config_file()
    };
    let path = selected.profile_file(profile, file);
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

    let edited = read_regular_string(temp.path())?;
    let other_file = if auth {
        selected.agent.main_config_file()
    } else {
        selected.agent.profile_auth_file()
    };
    let other = read_regular_string(&selected.profile_file(profile, other_file))?;
    let (main, auth_content) = if auth {
        (other.as_str(), edited.as_str())
    } else {
        (edited.as_str(), other.as_str())
    };
    ProfileDefinition::parse(selected.agent, main, auth_content)
        .with_context(|| format!("validate edited Agent Profile '{profile}'"))?;
    temp.persist(&path)
        .map_err(|error| error.error)
        .with_context(|| format!("replace {}", path.display()))?;
    tenant::sync_dir(parent)
}

/// Delete explicitly selected Profiles or every safe Profile directory.
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

    let targets = if all {
        deletable_profile_names(selected)?
    } else {
        let mut targets = Vec::new();
        for profile in profiles {
            tenant::validate_name("profile", profile)?;
            if inspect_deletable_profile(selected, profile)? && !targets.contains(profile) {
                targets.push(profile.clone());
            }
        }
        targets
    };
    if targets.is_empty() {
        eprintln!(">> no Agent Profiles in this Tenant and Coding Agent");
        return Ok(());
    }
    if !yes {
        for profile in &targets {
            if !confirm_delete(profile)? {
                bail!("aborted");
            }
        }
    }
    for profile in targets {
        remove_profile_directory(selected, &profile)?;
    }
    Ok(())
}

/// Apply every fixed Profile Field to the current Agent Configuration once.
pub fn apply_profile(selected: &TenantAgent, profile: &str) -> Result<()> {
    let definition = read_profile_definition(selected, profile)?;
    let current_main = capture_optional_agent_file(selected, selected.agent.main_config_file())?;
    let current_auth = selected
        .agent
        .native_auth_file()
        .map(|file| capture_optional_agent_file(selected, file))
        .transpose()?;
    let main_text = snapshot_text(&current_main, selected.agent.main_config_file())?;
    let auth_text = current_auth
        .as_ref()
        .map(|snapshot| snapshot_text(snapshot, "auth.json"))
        .transpose()?
        .flatten();
    let desired = definition.apply(main_text.as_deref(), auth_text.as_deref())?;

    let mut writes = Vec::new();
    collect_agent_write(
        selected.agent.main_config_file(),
        &current_main,
        desired.main,
        &mut writes,
    );
    if let (Some(file), Some(current)) = (selected.agent.native_auth_file(), current_auth.as_ref())
    {
        collect_agent_write(file, current, desired.auth, &mut writes);
    }
    if writes.is_empty() {
        return Ok(());
    }

    tenant::ensure_agent_state(selected.agent, selected.home_dir())?;
    let mut prepared = Vec::with_capacity(writes.len());
    for write in writes {
        let target = selected.state_file(write.file);
        let parent = target
            .parent()
            .context("Agent Configuration path has no parent")?;
        let prefix = temporary_file_prefix(&target, "apply")?;
        let temp = write_temporary_file(parent, &prefix, &write.content, write.mode)?;
        prepared.push((target, temp));
    }
    for (target, temp) in prepared {
        let parent = target
            .parent()
            .context("Agent Configuration path has no parent")?;
        temp.persist(&target)
            .map_err(|error| error.error)
            .with_context(|| format!("replace {}", target.display()))?;
        tenant::sync_dir(parent)?;
    }
    Ok(())
}

struct AgentWrite {
    file: &'static str,
    content: Vec<u8>,
    mode: u32,
}

fn collect_agent_write(
    file: &'static str,
    current: &FileSnapshot,
    desired: Option<String>,
    writes: &mut Vec<AgentWrite>,
) {
    let Some(desired) = desired else {
        debug_assert!(!current.present);
        return;
    };
    let content = desired.into_bytes();
    if current.present && current.content == content {
        return;
    }
    writes.push(AgentWrite {
        file,
        content,
        mode: current.mode.unwrap_or(0o600),
    });
}

fn read_profile_definition(selected: &TenantAgent, profile: &str) -> Result<ProfileDefinition> {
    ensure_complete_profile(selected, profile)?;
    let main =
        read_regular_string(&selected.profile_file(profile, selected.agent.main_config_file()))?;
    let auth =
        read_regular_string(&selected.profile_file(profile, selected.agent.profile_auth_file()))?;
    ProfileDefinition::parse(selected.agent, &main, &auth)
        .with_context(|| format!("parse Agent Profile '{profile}'"))
}

fn ensure_complete_profile(selected: &TenantAgent, profile: &str) -> Result<()> {
    tenant::validate_name("profile", profile)?;
    let Some(layout) = inspect_profile_directory(selected, profile)? else {
        bail!("Agent Profile '{profile}' does not exist");
    };
    if !layout.complete() {
        let missing = if !layout.main {
            selected.agent.main_config_file()
        } else {
            selected.agent.profile_auth_file()
        };
        bail!("Agent Profile '{profile}' is incomplete: missing {missing}");
    }
    validate_private_directory(&selected.profile_dir(profile))?;
    for file in selected.agent.profile_files() {
        validate_private_file(&selected.profile_file(profile, file))?;
    }
    Ok(())
}

fn inspect_profile_directory(
    selected: &TenantAgent,
    profile: &str,
) -> Result<Option<ProfileLayout>> {
    tenant::validate_name("profile", profile)?;
    if !selected.profile_catalog_exists()? {
        return Ok(None);
    }
    let path = selected.profile_dir(profile);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("inspect {}", path.display())),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            bail!(
                "Profile directory is not a real directory: {}",
                path.display()
            )
        }
        Ok(_) => {}
    }
    let mut layout = ProfileLayout::default();
    for entry in fs::read_dir(&path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry?;
        let name = entry
            .file_name()
            .to_str()
            .context("Agent Profile file name is not valid UTF-8")?
            .to_string();
        let kind = entry.file_type()?;
        if !kind.is_file() || kind.is_symlink() {
            bail!(
                "Agent Profile contains a non-regular file: {}",
                entry.path().display()
            );
        }
        if name == selected.agent.main_config_file() {
            layout.main = true;
        } else if name == selected.agent.profile_auth_file() {
            layout.auth = true;
        } else {
            bail!("Agent Profile contains an unknown entry: {name}");
        }
    }
    Ok(Some(layout))
}

fn deletable_profile_names(selected: &TenantAgent) -> Result<Vec<String>> {
    if !selected.profile_catalog_exists()? {
        return Ok(Vec::new());
    }
    let mut profiles = Vec::new();
    for entry in fs::read_dir(selected.profile_catalog_dir())? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if tenant::validate_name("profile", &name).is_err() {
            continue;
        }
        if inspect_deletable_profile(selected, &name)? {
            profiles.push(name);
        }
    }
    profiles.sort();
    Ok(profiles)
}

fn inspect_deletable_profile(selected: &TenantAgent, profile: &str) -> Result<bool> {
    if !selected.profile_catalog_exists()? {
        return Ok(false);
    }
    let path = selected.profile_dir(profile);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            bail!(
                "Profile directory is not a real directory: {}",
                path.display()
            )
        }
        Ok(_) => {}
    }
    for entry in fs::read_dir(&path)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .to_str()
            .context("Agent Profile file name is not valid UTF-8")?
            .to_string();
        if !selected.agent.profile_files().contains(&name.as_str()) {
            bail!("Agent Profile contains an unknown entry: {name}");
        }
        let kind = entry.file_type()?;
        if !kind.is_file() || kind.is_symlink() {
            bail!(
                "Agent Profile contains a non-regular file: {}",
                entry.path().display()
            );
        }
    }
    Ok(true)
}

fn remove_profile_directory(selected: &TenantAgent, profile: &str) -> Result<()> {
    if !inspect_deletable_profile(selected, profile)? {
        return Ok(());
    }
    for file in selected.agent.profile_files() {
        tenant::remove_real_file_if_exists(
            &selected.profile_file(profile, file),
            "Agent Profile file",
        )?;
    }
    let path = selected.profile_dir(profile);
    fs::remove_dir(&path)
        .with_context(|| format!("remove Profile directory {}", path.display()))?;
    tenant::sync_dir(selected.profile_catalog_dir())
}

fn ensure_profile_directory(selected: &TenantAgent, profile: &str) -> Result<()> {
    let path = selected.profile_dir(profile);
    tenant::ensure_real_dir(&path, "Profile directory")?;
    validate_private_directory(&path)
}

fn write_profile_file(
    selected: &TenantAgent,
    profile: &str,
    file: &str,
    content: &[u8],
) -> Result<()> {
    write_atomic(&selected.profile_file(profile, file), content, 0o600)
}

fn capture_optional_agent_file(selected: &TenantAgent, file: &str) -> Result<FileSnapshot> {
    if !tenant::real_dir_exists(selected.home_dir(), "Tenant Home")? {
        bail!(
            "Tenant Home does not exist: {}",
            selected.home_dir().display()
        );
    }
    if !tenant::real_dir_exists(&selected.agent_state_dir, "Agent state directory")? {
        return Ok(FileSnapshot {
            present: false,
            content: Vec::new(),
            mode: None,
        });
    }
    FileSnapshot::capture_with_limit(&selected.state_file(file), MAX_CONFIG_BYTES)
}

fn snapshot_text(snapshot: &FileSnapshot, file: &str) -> Result<Option<String>> {
    if !snapshot.present {
        return Ok(None);
    }
    String::from_utf8(snapshot.content.clone())
        .map(Some)
        .with_context(|| format!("Agent Configuration {file} is not valid UTF-8"))
}

fn read_regular_string(path: &Path) -> Result<String> {
    String::from_utf8(read_regular_bytes(path)?)
        .with_context(|| format!("{} is not valid UTF-8", path.display()))
}

fn read_regular_bytes(path: &Path) -> Result<Vec<u8>> {
    let file = tenant::open_real_file(path, "configuration file")?;
    let size = file.metadata()?.len();
    if size > MAX_CONFIG_BYTES {
        bail!(
            "configuration file exceeds {MAX_CONFIG_BYTES} bytes: {}",
            path.display()
        );
    }
    let mut content = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut content)?;
    if content.len() as u64 > MAX_CONFIG_BYTES {
        bail!(
            "configuration file exceeds {MAX_CONFIG_BYTES} bytes: {}",
            path.display()
        );
    }
    Ok(content)
}

fn validate_private_file(path: &Path) -> Result<()> {
    if !tenant::real_file_exists(path, "Agent Profile file")? {
        bail!("Agent Profile file does not exist: {}", path.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::symlink_metadata(path)?.permissions().mode() & 0o777;
        if mode != 0o600 {
            bail!("private file must have mode 0600: {}", path.display());
        }
    }
    Ok(())
}

fn validate_private_directory(path: &Path) -> Result<()> {
    if !tenant::real_dir_exists(path, "Agent Profile directory")? {
        bail!("Agent Profile directory does not exist: {}", path.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::symlink_metadata(path)?.permissions().mode() & 0o777;
        if mode != 0o700 {
            bail!("private directory must have mode 0700: {}", path.display());
        }
    }
    Ok(())
}

fn private_regular_file(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
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
}

fn private_directory(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        if !metadata.file_type().is_dir() {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o777 == 0o700
        }
        #[cfg(not(unix))]
        {
            true
        }
    })
}

fn write_atomic(path: &Path, content: &[u8], mode: u32) -> Result<()> {
    if content.len() as u64 > MAX_CONFIG_BYTES {
        bail!("refusing oversized configuration write: {}", path.display());
    }
    let parent = path.parent().context("configuration path has no parent")?;
    tenant::ensure_real_dir(parent, "configuration parent directory")?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
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
    let temp = write_temporary_file(parent, &prefix, content, mode)?;
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
