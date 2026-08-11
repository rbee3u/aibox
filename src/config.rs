//! Tenant-local Named Config catalog and one-time application commands.

use crate::cli::ConfigCommand;
use crate::config_model::NamedConfigDefinition;
use crate::tenant::{self, FileSnapshot, Tenant, TenantAgent};
use anyhow::{Context, Result, bail};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::Path;
use std::process::Command;

#[path = "config_auth.rs"]
mod auth;

#[cfg(test)]
use auth::{
    AuthPropagationReport, PropagationCounts, PropagationOutcome, execute_auth_propagation,
    plan_auth_propagation_from,
};

const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct NamedConfigLayout {
    main: bool,
    auth: bool,
}

impl NamedConfigLayout {
    fn complete(self, selected: &TenantAgent) -> bool {
        self.main && (selected.agent.native_auth_file().is_none() || self.auth)
    }
}

/// Execute one parsed Config command.
pub fn dispatch(selected: &TenantAgent, command: &ConfigCommand) -> Result<i32> {
    match command {
        ConfigCommand::List => {
            for config in list_named_configs(selected)? {
                if !crate::print_line(&config)? {
                    break;
                }
            }
        }
        ConfigCommand::Get { config, current } => {
            let output = if *current {
                get_current_config(selected)?
            } else {
                get_named_config(
                    selected,
                    config.as_deref().context("Named Config name is missing")?,
                )?
            };
            crate::print_bytes(&output)?;
        }
        ConfigCommand::Create { config } => create_named_config(selected, config)?,
        ConfigCommand::Edit { config, current } => {
            if *current {
                edit_current_config(selected)?;
            } else {
                edit_named_config_with_apply_prompt(
                    selected,
                    config.as_deref().context("Named Config name is missing")?,
                    confirm_apply_after_edit,
                )?;
            }
        }
        ConfigCommand::Delete { configs, all, yes } => {
            delete_named_configs(selected, configs, *all, *yes)?;
        }
        ConfigCommand::Apply { config } => apply_named_config(selected, config)?,
        ConfigCommand::PropagateAuth { .. } => {
            bail!("config propagate-auth must be dispatched as a global Config operation")
        }
    }
    Ok(0)
}

/// Propagate newer Host ChatGPT credentials to every matching existing Codex Config.
pub fn propagate_auth(root: &Path) -> Result<i32> {
    let host_home = tenant::host_home()?;
    propagate_auth_from(root, &host_home)
}

pub(crate) fn propagate_auth_from(root: &Path, host_home: &Path) -> Result<i32> {
    auth::propagate_auth_from(root, host_home)
}

/// Create a Named Config from the selected Coding Agent's built-in template.
pub fn create_named_config(selected: &TenantAgent, config: &str) -> Result<()> {
    tenant::validate_name("config", config)?;
    selected.ensure_named_config_catalog()?;

    if let Some(layout) = inspect_named_config_directory(selected, config)? {
        if layout.complete(selected) {
            bail!("Named Config '{config}' already exists");
        }
        return repair_incomplete_named_config(selected, config, layout);
    }

    let prospective_main = selected.agent.config_template().to_string();
    let prospective_auth = selected.agent.config_auth_template().map(str::to_string);
    NamedConfigDefinition::parse(
        selected.agent,
        &prospective_main,
        prospective_auth.as_deref(),
    )
    .context("validate built-in Named Config template")?;
    ensure_named_config_directory(selected, config)?;
    write_named_config_file(
        selected,
        config,
        selected.agent.main_config_file(),
        prospective_main.as_bytes(),
    )?;
    if let (Some(file), Some(auth)) = (selected.agent.native_auth_file(), prospective_auth) {
        write_named_config_file(selected, config, file, auth.as_bytes())?;
    }
    Ok(())
}

fn repair_incomplete_named_config(
    selected: &TenantAgent,
    config: &str,
    layout: NamedConfigLayout,
) -> Result<()> {
    let config_dir = selected.named_config_dir(config);
    validate_private_directory(&config_dir)?;
    let prospective_main = if layout.main {
        let path = selected.named_config_file(config, selected.agent.main_config_file());
        validate_private_file(&path)?;
        read_regular_string(&path)?
    } else {
        selected.agent.config_template().to_string()
    };
    let prospective_auth = match selected.agent.native_auth_file() {
        Some(file) if layout.auth => {
            let path = selected.named_config_file(config, file);
            validate_private_file(&path)?;
            Some(read_regular_string(&path)?)
        }
        Some(_) => Some(
            selected
                .agent
                .config_auth_template()
                .expect("agent with auth file has auth template")
                .to_string(),
        ),
        None => None,
    };
    NamedConfigDefinition::parse(
        selected.agent,
        &prospective_main,
        prospective_auth.as_deref(),
    )
    .with_context(|| format!("validate incomplete Named Config '{config}'"))?;
    if !layout.main {
        write_named_config_file(
            selected,
            config,
            selected.agent.main_config_file(),
            prospective_main.as_bytes(),
        )?;
    }
    if !layout.auth {
        let Some(file) = selected.agent.native_auth_file() else {
            return Ok(());
        };
        write_named_config_file(
            selected,
            config,
            file,
            prospective_auth
                .as_deref()
                .expect("agent with auth file has auth template")
                .as_bytes(),
        )?;
    }
    Ok(())
}

/// List complete, structurally safe Named Config names without parsing them.
pub fn list_named_configs(selected: &TenantAgent) -> Result<Vec<String>> {
    if !selected.named_config_catalog_exists()? {
        return Ok(Vec::new());
    }
    let root = selected.named_config_catalog_dir();
    let mut configs = Vec::new();
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
        if tenant::validate_name("config", &name).is_err() {
            continue;
        }
        let visible = inspect_named_config_directory(selected, &name)
            .ok()
            .flatten()
            .is_some_and(|layout| layout.complete(selected))
            && private_directory(&selected.named_config_dir(&name))
            && selected
                .agent
                .config_files()
                .iter()
                .all(|file| private_regular_file(&selected.named_config_file(&name, file)));
        if visible {
            configs.push(name);
        }
    }
    configs.sort();
    Ok(configs)
}

/// Return every raw file in a Named Config, including invalid content for repair.
pub fn get_named_config(selected: &TenantAgent, config: &str) -> Result<Vec<u8>> {
    ensure_complete_named_config(selected, config)?;
    let files = selected
        .agent
        .config_files()
        .iter()
        .map(|file| {
            read_regular_bytes(&selected.named_config_file(config, file))
                .map(|content| (*file, Some(content)))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(render_config_files(&files))
}

/// Return every Current Config file, marking absent files without creating them.
pub fn get_current_config(selected: &TenantAgent) -> Result<Vec<u8>> {
    let files = selected
        .agent
        .config_files()
        .iter()
        .map(|file| {
            capture_optional_agent_file(selected, file).map(|snapshot| {
                let content = snapshot.present.then_some(snapshot.content);
                (*file, content)
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(render_config_files(&files))
}

fn edit_named_config_with_editor(
    selected: &TenantAgent,
    config: &str,
    editor: &OsStr,
) -> Result<()> {
    ensure_complete_named_config(selected, config)?;
    for file in selected.agent.config_files() {
        let path = selected.named_config_file(config, file);
        let current = read_regular_bytes(&path)?;
        edit_file(&path, &current, 0o600, editor, |content| {
            let content = std::str::from_utf8(content)
                .with_context(|| format!("edited Named Config {file} is not valid UTF-8"))?;
            NamedConfigDefinition::validate_file(selected.agent, file, content)
                .with_context(|| format!("validate edited Named Config '{config}' {file}"))
        })?;
    }
    Ok(())
}

/// Edit every Current Config file in native order without parsing its content.
pub fn edit_current_config(selected: &TenantAgent) -> Result<()> {
    let editor = configured_editor();
    edit_current_config_with_editor(selected, &editor)
}

fn edit_current_config_with_editor(selected: &TenantAgent, editor: &OsStr) -> Result<()> {
    selected.ensure_agent_state_dir()?;
    let snapshots = selected
        .agent
        .config_files()
        .iter()
        .map(|file| capture_optional_agent_file(selected, file).map(|snapshot| (*file, snapshot)))
        .collect::<Result<Vec<_>>>()?;
    for (file, snapshot) in snapshots {
        let content = if snapshot.present {
            snapshot.content
        } else {
            selected
                .agent
                .empty_config_file(file)
                .expect("AgentKind config file contract is complete")
                .as_bytes()
                .to_vec()
        };
        edit_file(
            &selected.state_file(file),
            &content,
            snapshot.mode.unwrap_or(0o600),
            editor,
            |_| Ok(()),
        )?;
    }
    Ok(())
}

fn edit_named_config_with_apply_prompt<F>(
    selected: &TenantAgent,
    config: &str,
    confirm: F,
) -> Result<()>
where
    F: FnOnce(&TenantAgent, &str) -> Result<bool>,
{
    let editor = configured_editor();
    edit_named_config_with_editor_and_apply_prompt(selected, config, &editor, confirm)
}

fn edit_named_config_with_editor_and_apply_prompt<F>(
    selected: &TenantAgent,
    config: &str,
    editor: &OsStr,
    confirm: F,
) -> Result<()>
where
    F: FnOnce(&TenantAgent, &str) -> Result<bool>,
{
    edit_named_config_with_editor(selected, config, editor)?;
    if confirm(selected, config)? {
        let target = current_config_target(selected);
        apply_named_config(selected, config).with_context(|| {
            format!(
                "Named Config '{config}' was edited successfully, but applying it to {target} failed"
            )
        })?;
    }
    Ok(())
}

fn confirm_apply_after_edit(selected: &TenantAgent, config: &str) -> Result<bool> {
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        return Ok(false);
    }
    let mut input = stdin.lock();
    let mut output = io::stderr().lock();
    read_apply_confirmation(selected, config, &mut input, &mut output)
}

fn read_apply_confirmation(
    selected: &TenantAgent,
    config: &str,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> Result<bool> {
    write!(
        output,
        "Apply Named Config '{config}' to {} now? [y/N] ",
        current_config_target(selected)
    )?;
    output.flush().context("flush Config Application prompt")?;
    let mut answer = String::new();
    input
        .read_line(&mut answer)
        .context("read Config Application confirmation")?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "YES"))
}

fn current_config_target(selected: &TenantAgent) -> String {
    match &selected.tenant {
        Tenant::Managed(tenant) => format!(
            "{} Current Config for Managed Tenant '{}'",
            selected.agent.display_name(),
            tenant.name
        ),
        Tenant::Host { .. } => format!(
            "{} Current Config for Host Tenant",
            selected.agent.display_name()
        ),
    }
}

/// Delete explicitly selected Named Configs or every safe Named Config directory.
pub fn delete_named_configs(
    selected: &TenantAgent,
    configs: &[String],
    all: bool,
    yes: bool,
) -> Result<()> {
    if all && !configs.is_empty() {
        bail!("--all cannot be combined with Named Config names");
    }
    if !all && configs.is_empty() {
        bail!("provide at least one Named Config name or use --all");
    }

    let targets = if all {
        deletable_named_config_names(selected)?
    } else {
        let mut targets = Vec::new();
        for config in configs {
            tenant::validate_name("config", config)?;
            if inspect_deletable_named_config(selected, config)? && !targets.contains(config) {
                targets.push(config.clone());
            }
        }
        targets
    };
    if targets.is_empty() {
        eprintln!(">> no Named Configs in this Tenant and Coding Agent");
        return Ok(());
    }
    if !yes {
        for config in &targets {
            if !confirm_delete(config)? {
                bail!("aborted");
            }
        }
    }
    for config in targets {
        remove_named_config_directory(selected, &config)?;
    }
    Ok(())
}

/// Apply every fixed Config Field to the Current Config once.
pub fn apply_named_config(selected: &TenantAgent, config: &str) -> Result<()> {
    let definition = read_named_config_definition(selected, config)?;
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
    if let Some(file) = selected.agent.native_auth_file() {
        // A missing Current auth file is still a writable target: applying a
        // Named Config must materialize its complete native auth object.
        let absent = FileSnapshot {
            present: false,
            content: Vec::new(),
            mode: None,
        };
        let current = current_auth.as_ref().unwrap_or(&absent);
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
            .context("Current Config path has no parent")?;
        let prefix = temporary_file_prefix(&target, "apply")?;
        let temp = write_temporary_file(parent, &prefix, &write.content, write.mode)?;
        prepared.push((target, temp));
    }
    for (target, temp) in prepared {
        let parent = target
            .parent()
            .context("Current Config path has no parent")?;
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

fn read_named_config_definition(
    selected: &TenantAgent,
    config: &str,
) -> Result<NamedConfigDefinition> {
    ensure_complete_named_config(selected, config)?;
    let main = read_regular_string(
        &selected.named_config_file(config, selected.agent.main_config_file()),
    )?;
    let auth = selected
        .agent
        .native_auth_file()
        .map(|file| read_regular_string(&selected.named_config_file(config, file)))
        .transpose()?;
    NamedConfigDefinition::parse(selected.agent, &main, auth.as_deref())
        .with_context(|| format!("parse Named Config '{config}'"))
}

fn ensure_complete_named_config(selected: &TenantAgent, config: &str) -> Result<()> {
    tenant::validate_name("config", config)?;
    let Some(layout) = inspect_named_config_directory(selected, config)? else {
        bail!("Named Config '{config}' does not exist");
    };
    if !layout.complete(selected) {
        let missing = if layout.main {
            selected
                .agent
                .native_auth_file()
                .expect("incomplete config with main file must require auth")
        } else {
            selected.agent.main_config_file()
        };
        bail!("Named Config '{config}' is incomplete: missing {missing}");
    }
    validate_private_directory(&selected.named_config_dir(config))?;
    for file in selected.agent.config_files() {
        validate_private_file(&selected.named_config_file(config, file))?;
    }
    Ok(())
}

fn inspect_named_config_directory(
    selected: &TenantAgent,
    config: &str,
) -> Result<Option<NamedConfigLayout>> {
    tenant::validate_name("config", config)?;
    if !selected.named_config_catalog_exists()? {
        return Ok(None);
    }
    let path = selected.named_config_dir(config);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("inspect {}", path.display())),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            bail!(
                "Named Config directory is not a real directory: {}",
                path.display()
            )
        }
        Ok(_) => {}
    }
    let mut layout = NamedConfigLayout::default();
    for entry in fs::read_dir(&path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry?;
        let name = entry
            .file_name()
            .to_str()
            .context("Named Config file name is not valid UTF-8")?
            .to_string();
        let kind = entry.file_type()?;
        if !kind.is_file() || kind.is_symlink() {
            bail!(
                "Named Config contains a non-regular file: {}",
                entry.path().display()
            );
        }
        if name == selected.agent.main_config_file() {
            layout.main = true;
        } else if selected.agent.native_auth_file() == Some(name.as_str()) {
            layout.auth = true;
        } else {
            bail!("Named Config contains an unknown entry: {name}");
        }
    }
    Ok(Some(layout))
}

fn deletable_named_config_names(selected: &TenantAgent) -> Result<Vec<String>> {
    if !selected.named_config_catalog_exists()? {
        return Ok(Vec::new());
    }
    let mut configs = Vec::new();
    for entry in fs::read_dir(selected.named_config_catalog_dir())? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if tenant::validate_name("config", &name).is_err() {
            continue;
        }
        if inspect_deletable_named_config(selected, &name)? {
            configs.push(name);
        }
    }
    configs.sort();
    Ok(configs)
}

fn inspect_deletable_named_config(selected: &TenantAgent, config: &str) -> Result<bool> {
    if !selected.named_config_catalog_exists()? {
        return Ok(false);
    }
    let path = selected.named_config_dir(config);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
        Ok(metadata) if !metadata.file_type().is_dir() => {
            bail!(
                "Named Config directory is not a real directory: {}",
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
            .context("Named Config file name is not valid UTF-8")?
            .to_string();
        if !selected.agent.config_files().contains(&name.as_str()) {
            bail!("Named Config contains an unknown entry: {name}");
        }
        let kind = entry.file_type()?;
        if !kind.is_file() || kind.is_symlink() {
            bail!(
                "Named Config contains a non-regular file: {}",
                entry.path().display()
            );
        }
    }
    Ok(true)
}

fn remove_named_config_directory(selected: &TenantAgent, config: &str) -> Result<()> {
    if !inspect_deletable_named_config(selected, config)? {
        return Ok(());
    }
    for file in selected.agent.config_files() {
        tenant::remove_real_file_if_exists(
            &selected.named_config_file(config, file),
            "Named Config file",
        )?;
    }
    let path = selected.named_config_dir(config);
    fs::remove_dir(&path)
        .with_context(|| format!("remove Named Config directory {}", path.display()))?;
    tenant::sync_dir(selected.named_config_catalog_dir())
}

fn ensure_named_config_directory(selected: &TenantAgent, config: &str) -> Result<()> {
    let path = selected.named_config_dir(config);
    tenant::ensure_real_dir(&path, "Named Config directory")?;
    validate_private_directory(&path)
}

fn write_named_config_file(
    selected: &TenantAgent,
    config: &str,
    file: &str,
    content: &[u8],
) -> Result<()> {
    write_atomic(&selected.named_config_file(config, file), content, 0o600)
}

fn render_config_files(files: &[(&str, Option<Vec<u8>>)]) -> Vec<u8> {
    let mut output = Vec::new();
    for (index, (file, content)) in files.iter().enumerate() {
        if index > 0 {
            output.push(b'\n');
        }
        match content {
            Some(content) => {
                output.extend_from_slice(format!("==> {file} <==\n").as_bytes());
                output.extend_from_slice(content);
                if !content.ends_with(b"\n") {
                    output.push(b'\n');
                }
            }
            None => output.extend_from_slice(format!("==> {file} (missing) <==\n").as_bytes()),
        }
    }
    output
}

fn edit_file(
    path: &Path,
    current: &[u8],
    mode: u32,
    editor: &OsStr,
    validate: impl FnOnce(&[u8]) -> Result<()>,
) -> Result<()> {
    let parent = path.parent().context("configuration path has no parent")?;
    let prefix = temporary_file_prefix(path, "edit")?;
    let temp = write_temporary_file(parent, &prefix, current, mode)?;
    let status = editor_command(editor)?
        .arg(temp.path())
        .status()
        .with_context(|| format!("run editor {editor:?}"))?;
    if !status.success() {
        bail!("editor exited with status {status}");
    }

    let edited = read_regular_bytes(temp.path())?;
    validate(&edited)?;
    set_file_mode(temp.as_file(), mode)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("replace {}", path.display()))?;
    tenant::sync_dir(parent)
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
        .with_context(|| format!("Current Config {file} is not valid UTF-8"))
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
    if !tenant::real_file_exists(path, "Named Config file")? {
        bail!("Named Config file does not exist: {}", path.display());
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
    if !tenant::real_dir_exists(path, "Named Config directory")? {
        bail!("Named Config directory does not exist: {}", path.display());
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

fn replace_existing_atomic(path: &Path, content: &[u8], mode: u32) -> Result<()> {
    if content.len() as u64 > MAX_CONFIG_BYTES {
        bail!("refusing oversized configuration write: {}", path.display());
    }
    let parent = path.parent().context("configuration path has no parent")?;
    if !tenant::real_dir_exists(parent, "configuration parent directory")? {
        bail!(
            "configuration parent directory does not exist: {}",
            parent.display()
        );
    }
    if !tenant::real_file_exists(path, "configuration file")? {
        bail!("configuration file does not exist: {}", path.display());
    }
    let prefix = temporary_file_prefix(path, "propagate-auth")?;
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
    set_file_mode(temp.as_file(), mode)?;
    temp.as_file().sync_all()?;
    Ok(temp)
}

fn set_file_mode(file: &fs::File, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = (file, mode);
    Ok(())
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

fn confirm_delete(config: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!(
            "refusing to delete Named Config '{config}' without --yes in a non-interactive shell"
        );
    }
    eprint!("Delete Named Config '{config}'? [y/N] ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim(), "y" | "Y" | "yes" | "YES"))
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
