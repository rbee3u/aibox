//! Named Config catalog, Current Config access, one-shot Config Application,
//! and the entry points for global Codex Credential Propagation.

use crate::cli::ConfigCommand;
use crate::config_model::NamedConfigDefinition;
use crate::metadata::{self, PreparedMetadataWrite};
use crate::tenant::{self, FileSnapshot, Tenant, TenantAgent};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::Path;
use std::process::Command;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[path = "config_auth.rs"]
mod auth;

pub(crate) use auth::{
    AuthPropagationPlan, AuthPropagationPreview, credential_propagation_source_available,
    execute_auth_propagation, plan_auth_propagation_from, preview_auth_propagation,
};
#[cfg(test)]
use auth::{AuthPropagationReport, PropagationCounts, PropagationOutcome};

// Config commands buffer one native file at a time. Bound both untrusted
// Current Config input and an editor's replacement before allocating it all.
const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;
const LAST_APPLICATION_SECTION: &str = "last_application";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LastApplication {
    pub(crate) applied: String,
    pub(crate) applied_at: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConfigDrift {
    Untracked,
    Clean,
    Dirty,
    SourceMissing,
    ComparisonError,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ApplicationStatus {
    pub(crate) last_application: Option<LastApplication>,
    pub(crate) drift: ConfigDrift,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConfigFileSnapshot {
    pub(crate) file: String,
    pub(crate) exists: bool,
    pub(crate) content: Vec<u8>,
    pub(crate) revision: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct ConfigCatalogEntry {
    pub(crate) name: String,
    pub(crate) state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct CurrentConfigInspection {
    pub(crate) present_files: usize,
    pub(crate) expected_files: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct NamedConfigLayout {
    main: bool,
    auth: bool,
}

impl NamedConfigLayout {
    fn complete(self, selected: &TenantAgent) -> bool {
        self.main && (selected.agent.native_auth_file().is_none() || self.auth)
    }

    fn missing_files(self, selected: &TenantAgent) -> Vec<&'static str> {
        selected
            .agent
            .config_files()
            .iter()
            .copied()
            .filter(|file| {
                if *file == selected.agent.main_config_file() {
                    !self.main
                } else {
                    debug_assert_eq!(Some(*file), selected.agent.native_auth_file());
                    !self.auth
                }
            })
            .collect()
    }
}

/// Execute one parsed Config command and return its process exit code.
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

/// Propagate newer Host ChatGPT credentials to every matching existing Codex
/// Config.
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

pub(crate) fn inspect_named_configs(selected: &TenantAgent) -> Result<Vec<ConfigCatalogEntry>> {
    if !selected.named_config_catalog_exists()? {
        return Ok(Vec::new());
    }
    let root = selected.named_config_catalog_dir();
    let mut configs = Vec::new();
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let Ok(entry) = entry else { continue };
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
        let (state, detail) = match inspect_named_config_directory(selected, &name) {
            Ok(Some(layout)) if !layout.complete(selected) => {
                let missing = layout.missing_files(selected);
                let noun = if missing.len() == 1 { "file" } else { "files" };
                (
                    "incomplete",
                    Some(format!(
                        "Missing required {noun}: {}. Use Repair to restore this Named Config.",
                        missing.join(", ")
                    )),
                )
            }
            Ok(Some(_))
                if private_directory(&selected.named_config_dir(&name))
                    && selected.agent.config_files().iter().all(|file| {
                        private_regular_file(&selected.named_config_file(&name, file))
                    }) =>
            {
                ("ready", None)
            }
            Ok(Some(_)) => (
                "invalid",
                Some("Named Config permissions must be 0700/0600".to_string()),
            ),
            Ok(None) => continue,
            Err(error) => ("invalid", Some(format!("{error:#}"))),
        };
        configs.push(ConfigCatalogEntry {
            name,
            state,
            detail,
        });
    }
    configs.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(configs)
}

/// Inspect fixed Current Config file presence without reading their contents.
pub(crate) fn inspect_current_config(selected: &TenantAgent) -> Result<CurrentConfigInspection> {
    let expected_files = selected.agent.config_files().len();
    let home_label = match &selected.tenant {
        Tenant::Managed(_) => "Tenant Home",
        Tenant::Host { .. } => "Host Home",
    };
    if !tenant::real_dir_exists(selected.home_dir(), home_label)?
        || !tenant::real_dir_exists(&selected.agent_state_dir, "Agent state directory")?
    {
        return Ok(CurrentConfigInspection {
            present_files: 0,
            expected_files,
        });
    }
    let mut present_files = 0;
    for file in selected.agent.config_files() {
        if tenant::real_file_exists(&selected.state_file(file), "Current Config file")? {
            present_files += 1;
        }
    }
    Ok(CurrentConfigInspection {
        present_files,
        expected_files,
    })
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

pub(crate) fn read_config_file(
    selected: &TenantAgent,
    config: Option<&str>,
    current: bool,
    file: &str,
) -> Result<ConfigFileSnapshot> {
    validate_config_selection(selected, config, current, file)?;
    let snapshot = if current {
        capture_optional_agent_file(selected, file)?
    } else {
        let config = config.context("Named Config name is missing")?;
        ensure_complete_named_config(selected, config)?;
        FileSnapshot::capture_with_limit(
            &selected.named_config_file(config, file),
            MAX_CONFIG_BYTES,
        )?
    };
    let content = if snapshot.present {
        snapshot.content.clone()
    } else {
        selected
            .agent
            .empty_config_file(file)
            .context("Agent Config file contract is incomplete")?
            .as_bytes()
            .to_vec()
    };
    Ok(ConfigFileSnapshot {
        file: file.to_string(),
        exists: snapshot.present,
        revision: file_revision(snapshot.present, &snapshot.content),
        content,
    })
}

pub(crate) fn save_config_file(
    selected: &TenantAgent,
    config: Option<&str>,
    current: bool,
    file: &str,
    expected_revision: &str,
    content: &[u8],
) -> Result<ConfigFileSnapshot> {
    if content.len() as u64 > MAX_CONFIG_BYTES {
        bail!("configuration file exceeds {MAX_CONFIG_BYTES} bytes");
    }
    let before = read_config_file(selected, config, current, file)?;
    if before.revision != expected_revision {
        bail!("configuration file changed since it was revealed");
    }
    let (path, mode) = if current {
        selected.ensure_agent_state_dir()?;
        let snapshot = capture_optional_agent_file(selected, file)?;
        (selected.state_file(file), snapshot.mode.unwrap_or(0o600))
    } else {
        let config = config.context("Named Config name is missing")?;
        let content_text = std::str::from_utf8(content)
            .with_context(|| format!("Named Config {file} is not valid UTF-8"))?;
        NamedConfigDefinition::validate_file(selected.agent, file, content_text)
            .with_context(|| format!("validate Named Config '{config}' {file}"))?;
        (selected.named_config_file(config, file), 0o600)
    };
    write_atomic(&path, content, mode)?;
    read_config_file(selected, config, current, file)
}

fn validate_config_selection(
    selected: &TenantAgent,
    config: Option<&str>,
    current: bool,
    file: &str,
) -> Result<()> {
    if current == config.is_some() {
        bail!("select exactly one of Current Config or a Named Config");
    }
    if !selected.agent.config_files().contains(&file) {
        bail!(
            "unsupported Config file for {}: {file}",
            selected.agent.tag()
        );
    }
    if let Some(config) = config {
        tenant::validate_name("config", config)?;
    }
    Ok(())
}

fn file_revision(present: bool, content: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update([u8::from(present)]);
    digest.update(content);
    let digest = digest.finalize();
    let mut revision = String::with_capacity(digest.len() * 2);
    use std::fmt::Write as _;
    for byte in digest {
        write!(&mut revision, "{byte:02x}").expect("writing to a String cannot fail");
    }
    revision
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
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
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
    let metadata = prepare_last_application(selected, config)?;

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
    if !writes.is_empty() {
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
    }
    metadata.commit().context("write Last Application metadata")
}

pub(crate) fn application_status(selected: &TenantAgent) -> ApplicationStatus {
    match application_status_inner(selected) {
        Ok(status) => status,
        Err(error) => ApplicationStatus {
            last_application: None,
            drift: ConfigDrift::ComparisonError,
            detail: Some(format!("{error:#}")),
        },
    }
}

fn application_status_inner(selected: &TenantAgent) -> Result<ApplicationStatus> {
    let Some(last_application) = read_last_application(selected)? else {
        return Ok(ApplicationStatus {
            last_application: None,
            drift: ConfigDrift::Untracked,
            detail: None,
        });
    };
    let layout = match inspect_named_config_directory(selected, &last_application.applied) {
        Ok(Some(layout)) if layout.complete(selected) => layout,
        Ok(_) => {
            return Ok(ApplicationStatus {
                last_application: Some(last_application),
                drift: ConfigDrift::SourceMissing,
                detail: None,
            });
        }
        Err(error) => {
            return Ok(ApplicationStatus {
                last_application: Some(last_application),
                drift: ConfigDrift::ComparisonError,
                detail: Some(format!("{error:#}")),
            });
        }
    };
    let _ = layout;
    let comparison = compare_application_source(selected, &last_application.applied);
    Ok(match comparison {
        Ok(clean) => ApplicationStatus {
            last_application: Some(last_application),
            drift: if clean {
                ConfigDrift::Clean
            } else {
                ConfigDrift::Dirty
            },
            detail: None,
        },
        Err(error) => ApplicationStatus {
            last_application: Some(last_application),
            drift: ConfigDrift::ComparisonError,
            detail: Some(format!("{error:#}")),
        },
    })
}

fn compare_application_source(selected: &TenantAgent, config: &str) -> Result<bool> {
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
    let main_matches = desired_file_matches(&current_main, desired.main.as_deref());
    let auth_matches = match (selected.agent.native_auth_file(), current_auth.as_ref()) {
        (Some(_), Some(current)) => desired_file_matches(current, desired.auth.as_deref()),
        (None, None) => true,
        _ => false,
    };
    Ok(main_matches && auth_matches)
}

fn desired_file_matches(current: &FileSnapshot, desired: Option<&str>) -> bool {
    match desired {
        Some(desired) => current.present && current.content == desired.as_bytes(),
        None => !current.present,
    }
}

fn prepare_last_application(selected: &TenantAgent, config: &str) -> Result<PreparedMetadataWrite> {
    let mut document = metadata::read(selected)?;
    if let Some(existing) = document.section::<LastApplication>(LAST_APPLICATION_SECTION)? {
        validate_last_application(&existing)?;
    }
    let record = LastApplication {
        applied: config.to_string(),
        applied_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .context("format Last Application time")?,
    };
    document.set_section(LAST_APPLICATION_SECTION, &record)?;
    document.prepare(selected)
}

fn read_last_application(selected: &TenantAgent) -> Result<Option<LastApplication>> {
    let document = metadata::read(selected)?;
    let Some(record): Option<LastApplication> = document.section(LAST_APPLICATION_SECTION)? else {
        return Ok(None);
    };
    validate_last_application(&record)?;
    Ok(Some(record))
}

fn validate_last_application(record: &LastApplication) -> Result<()> {
    tenant::validate_name("config", &record.applied)?;
    OffsetDateTime::parse(&record.applied_at, &Rfc3339).context("parse Last Application time")?;
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
        let missing = layout
            .missing_files(selected)
            .into_iter()
            .next()
            .expect("incomplete Named Config must have a missing file");
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
        } else if is_stale_temporary_file(selected, &name) {
            // An interrupted aibox write leaves its temporary file behind;
            // tolerating it keeps the Named Config usable and deletable.
        } else {
            bail!("Named Config contains an unknown entry: {name}");
        }
    }
    Ok(Some(layout))
}

/// True when `name` matches a Named Config temporary file that aibox can have
/// left behind after an interrupted write or edit. Keep this exact: unknown
/// entries must not become silently deletable just because they share a prefix.
fn is_stale_temporary_file(selected: &TenantAgent, name: &str) -> bool {
    selected.agent.config_files().iter().any(|file| {
        ["write", "edit", "propagate-auth"].iter().any(|purpose| {
            let prefix = format!(".{file}.aibox-{purpose}-");
            name.strip_prefix(&prefix).is_some_and(|suffix| {
                suffix.len() == 6 && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        })
    })
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
        if !selected.agent.config_files().contains(&name.as_str())
            && !is_stale_temporary_file(selected, &name)
        {
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
    for entry in fs::read_dir(&path).with_context(|| format!("read {}", path.display()))? {
        let entry = entry?;
        let is_stale = entry
            .file_name()
            .to_str()
            .is_some_and(|name| is_stale_temporary_file(selected, name));
        if is_stale {
            tenant::remove_real_file_if_exists(&entry.path(), "stale temporary file")?;
        }
    }
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

    // Some editors save by replacing the file rather than rewriting it, so
    // reopen the temporary path: mode and fsync must apply to the inode that
    // `persist` will rename into place, not to the pre-editor handle.
    let edited_file = tenant::open_real_file(temp.path(), "edited configuration file")?;
    let edited = read_open_bytes(&edited_file, temp.path())?;
    validate(&edited)?;
    set_file_mode(&edited_file, mode)?;
    edited_file.sync_all()?;
    temp.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("replace {}", path.display()))?;
    tenant::sync_dir(parent)
}

fn capture_optional_agent_file(selected: &TenantAgent, file: &str) -> Result<FileSnapshot> {
    let home_label = match &selected.tenant {
        Tenant::Managed(_) => "Tenant Home",
        Tenant::Host { .. } => "Host Home",
    };
    if !tenant::real_dir_exists(selected.home_dir(), home_label)? {
        bail!(
            "{home_label} does not exist: {}",
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
    read_open_bytes(&file, path)
}

fn read_open_bytes(file: &fs::File, path: &Path) -> Result<Vec<u8>> {
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
    Ok(matches!(input.trim().to_lowercase().as_str(), "y" | "yes"))
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
