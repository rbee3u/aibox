//! Named Config catalog, Current Config access, one-shot Config Application,
//! and the entry points for global Codex Credential Propagation.

pub(crate) mod model;

use crate::application_error::{ApplicationErrorKind, application_error};
pub(crate) use crate::config::model::{
    CodexAuthInspection, VisualConfigOptionState, VisualConfigState,
};
use crate::config::model::{
    CustomProviderInput, NamedConfigDefinition, VisualAuthInput, VisualConfigOptionInput,
    inspect_codex_auth, inspect_visual_config, render_visual_auth, render_visual_main,
};
use crate::foundation::safe_fs::FileSnapshot;
use crate::metadata::{self, PreparedMetadataWrite};
use crate::tenant::{self, Tenant, TenantAgent};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::Path;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

mod auth;

pub(crate) use auth::{
    AuthPropagationPlan, AuthPropagationPreview, AuthPropagationReport,
    credential_propagation_source_available, execute_auth_propagation, plan_auth_propagation_from,
    preview_auth_propagation,
};

#[cfg(test)]
pub(crate) use auth::{PropagationEntry, PropagationOutcome, PropagationPreviewEntry};

// Bound every untrusted native Config file before allocating it all.
const MAX_CONFIG_BYTES: u64 = 16 * 1024 * 1024;
const LAST_APPLICATION_SECTION: &str = "last_application";

/// A validated Named Config name.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct NamedConfigName(String);

impl NamedConfigName {
    /// Parse a lowercase DNS label.
    pub(crate) fn parse(value: &str) -> Result<Self> {
        tenant::validate_name("config", value)?;
        Ok(Self(value.to_string()))
    }

    /// Return the validated name as text.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NamedConfigName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One Agent-defined native Config file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfigFile {
    /// The Coding Agent's main native configuration file.
    Main,
    /// The Coding Agent's native credential file.
    Auth,
}

impl ConfigFile {
    /// Resolve a wire filename against one Coding Agent contract.
    pub(crate) fn parse(agent: crate::agent::AgentKind, value: &str) -> Result<Self> {
        if value == agent.main_config_file() {
            return Ok(Self::Main);
        }
        if agent.native_auth_file() == Some(value) {
            return Ok(Self::Auth);
        }
        bail!("unsupported Config file for {}: {value}", agent.tag())
    }

    /// Return the native filename for one Coding Agent.
    pub(crate) fn as_str(self, agent: crate::agent::AgentKind) -> &'static str {
        match self {
            Self::Main => agent.main_config_file(),
            Self::Auth => agent
                .native_auth_file()
                .expect("ConfigFile::Auth requires an Agent auth contract"),
        }
    }
}

/// A mutually exclusive Current or Named Config selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ConfigTarget {
    /// The selected Coding Agent's Current Config.
    Current,
    /// One validated Named Config.
    Named(NamedConfigName),
}

impl ConfigTarget {
    /// Convert the existing wire selector into one legal internal target.
    pub(crate) fn from_wire(config: Option<&str>, current: bool) -> Result<Self> {
        match (current, config) {
            (true, None) => Ok(Self::Current),
            (false, Some(config)) => Ok(Self::Named(NamedConfigName::parse(config)?)),
            _ => bail!("select exactly one of Current Config or a Named Config"),
        }
    }

    pub(crate) fn named(&self) -> Option<&NamedConfigName> {
        match self {
            Self::Current => None,
            Self::Named(name) => Some(name),
        }
    }

    pub(crate) fn is_current(&self) -> bool {
        matches!(self, Self::Current)
    }
}

/// One legal Raw or Visual Config edit submitted after wire decoding.
#[derive(Clone, Debug)]
pub(crate) enum ConfigEdit {
    /// Replace the selected native file with arbitrary decoded bytes.
    Raw {
        content: Vec<u8>,
        custom_provider: Option<CustomProviderInput>,
    },
    /// Render the main Named Config from Visual Editor options.
    VisualMain {
        options: Vec<VisualConfigOptionInput>,
        custom_provider: Option<CustomProviderInput>,
    },
    /// Render the Codex credential file from the Visual Editor.
    VisualAuth(VisualAuthInput),
}

impl ConfigEdit {
    /// Convert the existing wire fields into one mutually exclusive edit.
    pub(crate) fn from_wire(
        content: Vec<u8>,
        custom_provider: Option<CustomProviderInput>,
        visual_options: Option<Vec<VisualConfigOptionInput>>,
        visual_auth: Option<VisualAuthInput>,
    ) -> Result<Self> {
        if content.len() as u64 > MAX_CONFIG_BYTES {
            return Err(application_error(
                ApplicationErrorKind::InputTooLarge,
                format!("configuration file exceeds {MAX_CONFIG_BYTES} bytes"),
            ));
        }
        match (visual_options, visual_auth) {
            (Some(_), Some(_)) => {
                bail!("select exactly one Visual Config editor operation")
            }
            (Some(options), None) => Ok(Self::VisualMain {
                options,
                custom_provider,
            }),
            (None, Some(auth)) => {
                if custom_provider.is_some() {
                    bail!("Custom Provider is only available for the main Config file");
                }
                Ok(Self::VisualAuth(auth))
            }
            (None, None) => Ok(Self::Raw {
                content,
                custom_provider,
            }),
        }
    }

    fn custom_provider(&self) -> Option<&CustomProviderInput> {
        match self {
            Self::Raw {
                custom_provider, ..
            }
            | Self::VisualMain {
                custom_provider, ..
            } => custom_provider.as_ref(),
            Self::VisualAuth(_) => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct LastApplication {
    pub(crate) applied: String,
    pub(crate) applied_at: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConfigDrift {
    Untracked,
    Clean,
    Dirty,
    SourceMissing,
    ComparisonError,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ApplicationStatus {
    pub(crate) last_application: Option<LastApplication>,
    pub(crate) drift: ConfigDrift,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub(crate) detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConfigFileSnapshot {
    pub(crate) file: String,
    pub(crate) exists: bool,
    pub(crate) content: Vec<u8>,
    pub(crate) revision: String,
}

pub(crate) struct ConfigSaveResult {
    pub(crate) snapshot: ConfigFileSnapshot,
    pub(crate) linked: Option<ConfigFileSnapshot>,
}

pub(crate) fn visual_config_state(
    selected: &TenantAgent,
    config: &str,
    content: &str,
) -> Result<VisualConfigState> {
    tenant::validate_name("config", config)?;
    ensure_named_config_main(selected, config)?;
    inspect_visual_config(selected.agent, content)
}

pub(crate) fn inspect_named_codex_auth(
    selected: &TenantAgent,
    config: &str,
    content: &str,
) -> Result<CodexAuthInspection> {
    tenant::validate_name("config", config)?;
    ensure_safe_named_config(selected, config)?;
    inspect_codex_auth(content, None)
}

pub(crate) fn config_file_warnings(
    selected: &TenantAgent,
    config: &str,
    file: &str,
    content: &[u8],
) -> Result<Vec<String>> {
    tenant::validate_name("config", config)?;
    let text = std::str::from_utf8(content)
        .with_context(|| format!("Named Config {file} is not valid UTF-8"))?;
    crate::config::model::NamedConfigDefinition::validate_file_with_warnings(
        selected.agent,
        file,
        text,
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConfigCatalogState {
    Ready,
    Incomplete,
    Invalid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ConfigCatalogEntry {
    pub(crate) name: String,
    pub(crate) state: ConfigCatalogState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub(crate) detail: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
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
        let (state, detail, warnings) = match inspect_named_config_directory(selected, &name) {
            Ok(Some(layout)) if !layout.complete(selected) => {
                let missing = layout.missing_files(selected);
                let noun = if missing.len() == 1 { "file" } else { "files" };
                (
                    ConfigCatalogState::Incomplete,
                    Some(format!(
                        "Missing required {noun}: {}. Use Repair to restore this Named Config.",
                        missing.join(", ")
                    )),
                    Vec::new(),
                )
            }
            Ok(Some(_))
                if private_directory(&selected.named_config_dir(&name))
                    && selected.agent.config_files().iter().all(|file| {
                        private_regular_file(&selected.named_config_file(&name, file))
                    }) =>
            {
                match read_named_config_validation(selected, &name) {
                    Ok(validation) => (ConfigCatalogState::Ready, None, validation.warnings),
                    Err(error) => (
                        ConfigCatalogState::Invalid,
                        Some(format!("{error:#}")),
                        Vec::new(),
                    ),
                }
            }
            Ok(Some(_)) => (
                ConfigCatalogState::Invalid,
                Some("Named Config permissions must be 0700/0600".to_string()),
                Vec::new(),
            ),
            Ok(None) => continue,
            Err(error) => (
                ConfigCatalogState::Invalid,
                Some(format!("{error:#}")),
                Vec::new(),
            ),
        };
        configs.push(ConfigCatalogEntry {
            name,
            state,
            detail,
            warnings,
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
    if !crate::foundation::safe_fs::real_dir_exists(selected.home_dir(), home_label)?
        || !crate::foundation::safe_fs::real_dir_exists(
            &selected.agent_state_dir,
            "Agent state directory",
        )?
    {
        return Ok(CurrentConfigInspection {
            present_files: 0,
            expected_files,
        });
    }
    let mut present_files = 0;
    for file in selected.agent.config_files() {
        if crate::foundation::safe_fs::real_file_exists(
            &selected.state_file(file),
            "Current Config file",
        )? {
            present_files += 1;
        }
    }
    Ok(CurrentConfigInspection {
        present_files,
        expected_files,
    })
}

pub(crate) fn read_config_file_target(
    selected: &TenantAgent,
    target: &ConfigTarget,
    file: ConfigFile,
) -> Result<ConfigFileSnapshot> {
    let file = file.as_str(selected.agent);
    let snapshot = if target.is_current() {
        capture_optional_agent_file(selected, file)?
    } else {
        let config = target
            .named()
            .expect("non-current ConfigTarget must have a name");
        ensure_safe_named_config(selected, config.as_str())?;
        let path = selected.named_config_file(config.as_str(), file);
        if crate::foundation::safe_fs::real_file_exists(&path, "Named Config file")? {
            FileSnapshot::capture_with_limit(&path, MAX_CONFIG_BYTES)?
        } else {
            FileSnapshot {
                present: false,
                content: Vec::new(),
                mode: None,
            }
        }
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

#[allow(dead_code)]
pub(crate) fn read_config_file(
    selected: &TenantAgent,
    config: Option<&str>,
    current: bool,
    file: &str,
) -> Result<ConfigFileSnapshot> {
    let target = ConfigTarget::from_wire(config, current)?;
    let file = ConfigFile::parse(selected.agent, file)?;
    read_config_file_target(selected, &target, file)
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn save_config_file(
    selected: &TenantAgent,
    config: Option<&str>,
    current: bool,
    file: &str,
    expected_revision: &str,
    content: &[u8],
    visual: Option<&[VisualConfigOptionInput]>,
    visual_auth: Option<&VisualAuthInput>,
) -> Result<ConfigFileSnapshot> {
    save_config_file_with_linked(
        selected,
        config,
        current,
        file,
        expected_revision,
        content,
        None,
        visual,
        visual_auth,
    )
    .map(|result| result.snapshot)
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub(crate) fn save_config_file_with_linked(
    selected: &TenantAgent,
    config: Option<&str>,
    current: bool,
    file: &str,
    expected_revision: &str,
    content: &[u8],
    custom_provider: Option<&CustomProviderInput>,
    visual: Option<&[VisualConfigOptionInput]>,
    visual_auth: Option<&VisualAuthInput>,
) -> Result<ConfigSaveResult> {
    let target = ConfigTarget::from_wire(config, current)?;
    let file = ConfigFile::parse(selected.agent, file)?;
    let edit = ConfigEdit::from_wire(
        content.to_vec(),
        custom_provider.cloned(),
        visual.map(<[VisualConfigOptionInput]>::to_vec),
        visual_auth.cloned(),
    )?;
    save_config_file_target(selected, &target, file, expected_revision, edit)
}

pub(crate) fn save_config_file_target(
    selected: &TenantAgent,
    target: &ConfigTarget,
    file: ConfigFile,
    expected_revision: &str,
    edit: ConfigEdit,
) -> Result<ConfigSaveResult> {
    let file = file.as_str(selected.agent);
    let before =
        read_config_file_target(selected, target, ConfigFile::parse(selected.agent, file)?)?;
    if before.revision != expected_revision {
        return Err(application_error(
            ApplicationErrorKind::Conflict,
            "configuration file changed since it was revealed",
        ));
    }
    let (path, mode, content) = if target.is_current() {
        selected.ensure_agent_state_dir()?;
        let snapshot = capture_optional_agent_file(selected, file)?;
        let ConfigEdit::Raw { content, .. } = &edit else {
            bail!("Visual editing is only available for a Named Config");
        };
        (
            selected.state_file(file),
            snapshot.mode.unwrap_or(0o600),
            content.clone(),
        )
    } else {
        let config = target
            .named()
            .expect("non-current ConfigTarget must have a name");
        ensure_safe_named_config(selected, config.as_str())?;
        let content = match &edit {
            ConfigEdit::VisualMain {
                options,
                custom_provider,
            } => {
                if file != selected.agent.main_config_file() {
                    bail!("Visual main fields are only available for the main Config file");
                }
                let original = std::str::from_utf8(&before.content)
                    .with_context(|| format!("Named Config {file} is not valid UTF-8"))?;
                render_visual_main(selected.agent, original, options, custom_provider.as_ref())?
                    .into_bytes()
            }
            ConfigEdit::VisualAuth(auth) => {
                if selected.agent.native_auth_file() != Some(file) {
                    bail!("Visual auth is only available for Codex auth.json");
                }
                render_visual_auth(auth)?.into_bytes()
            }
            ConfigEdit::Raw { content, .. } => content.clone(),
        };
        if content.len() as u64 > MAX_CONFIG_BYTES {
            return Err(application_error(
                ApplicationErrorKind::InputTooLarge,
                format!("configuration file exceeds {MAX_CONFIG_BYTES} bytes"),
            ));
        }
        let content_text = std::str::from_utf8(&content)
            .with_context(|| format!("Named Config {file} is not valid UTF-8"))?;
        let layout = inspect_named_config_directory(selected, config.as_str())?
            .context("Named Config directory disappeared while saving")?;
        let _ = layout;
        NamedConfigDefinition::validate_file(selected.agent, file, content_text)
            .with_context(|| format!("validate Named Config '{config}' {file}"))?;
        (
            selected.named_config_file(config.as_str(), file),
            0o600,
            content,
        )
    };
    write_atomic(&path, &content, mode)?;
    let snapshot =
        read_config_file_target(selected, target, ConfigFile::parse(selected.agent, file)?)?;
    let linked = if !target.is_current()
        && file == selected.agent.main_config_file()
        && edit
            .custom_provider()
            .is_some_and(|provider| provider.included)
        && selected.agent.native_auth_file().is_some()
    {
        let auth_file = selected.agent.native_auth_file().expect("Codex auth file");
        let auth_kind = ConfigFile::Auth;
        let auth_before = read_config_file_target(selected, target, auth_kind)?;
        let empty_auth = if !auth_before.exists {
            true
        } else {
            serde_json::from_slice::<Value>(&auth_before.content)
                .ok()
                .and_then(|value| value.as_object().map(Map::is_empty))
                .unwrap_or(false)
        };
        if empty_auth {
            let placeholder = selected
                .agent
                .config_auth_template()
                .context("Codex auth template is missing")?
                .as_bytes();
            let config = target.named().expect("Named Config");
            let auth_path = selected.named_config_file(config.as_str(), auth_file);
            write_atomic(&auth_path, placeholder, 0o600)?;
            Some(read_config_file_target(selected, target, auth_kind)?)
        } else {
            None
        }
    } else {
        None
    };
    Ok(ConfigSaveResult { snapshot, linked })
}

fn ensure_safe_named_config(selected: &TenantAgent, config: &str) -> Result<()> {
    let Some(layout) = inspect_named_config_directory(selected, config)? else {
        bail!("Named Config '{config}' does not exist");
    };
    let _ = layout;
    validate_private_directory(&selected.named_config_dir(config))?;
    for file in selected.agent.config_files() {
        let path = selected.named_config_file(config, file);
        if crate::foundation::safe_fs::real_file_exists(&path, "Named Config file")? {
            validate_private_file(&path)?;
        }
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

/// Delete explicitly selected Named Configs or every safe Named Config directory.
pub fn delete_named_configs(selected: &TenantAgent, configs: &[String], all: bool) -> Result<()> {
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
            temp.persist(&target, "replace")?;
            crate::foundation::safe_fs::sync_dir(parent)?;
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
    Ok(read_named_config_validation(selected, config)?.definition)
}

fn read_named_config_validation(
    selected: &TenantAgent,
    config: &str,
) -> Result<crate::config::model::NamedConfigValidation> {
    ensure_complete_named_config(selected, config)?;
    let main = read_regular_string(
        &selected.named_config_file(config, selected.agent.main_config_file()),
    )?;
    let auth = selected
        .agent
        .native_auth_file()
        .map(|file| read_regular_string(&selected.named_config_file(config, file)))
        .transpose()?;
    NamedConfigDefinition::parse_with_warnings(selected.agent, &main, auth.as_deref())
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

fn ensure_named_config_main(selected: &TenantAgent, config: &str) -> Result<()> {
    tenant::validate_name("config", config)?;
    let Some(layout) = inspect_named_config_directory(selected, config)? else {
        bail!("Named Config '{config}' does not exist");
    };
    if !layout.main {
        bail!(
            "Named Config '{config}' is incomplete: missing {}",
            selected.agent.main_config_file()
        );
    }
    validate_private_directory(&selected.named_config_dir(config))?;
    validate_private_file(&selected.named_config_file(config, selected.agent.main_config_file()))?;
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
            // An interrupted AIBox write leaves its temporary file behind;
            // tolerating it keeps the Named Config usable and deletable.
        } else {
            bail!("Named Config contains an unknown entry: {name}");
        }
    }
    Ok(Some(layout))
}

/// True when `name` matches a Named Config temporary file that AIBox can have
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
        crate::foundation::safe_fs::remove_real_file_if_exists(
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
            crate::foundation::safe_fs::remove_real_file_if_exists(
                &entry.path(),
                "stale temporary file",
            )?;
        }
    }
    fs::remove_dir(&path)
        .with_context(|| format!("remove Named Config directory {}", path.display()))?;
    crate::foundation::safe_fs::sync_dir(selected.named_config_catalog_dir())
}

fn ensure_named_config_directory(selected: &TenantAgent, config: &str) -> Result<()> {
    let path = selected.named_config_dir(config);
    crate::foundation::safe_fs::ensure_real_dir(&path, "Named Config directory")?;
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

fn capture_optional_agent_file(selected: &TenantAgent, file: &str) -> Result<FileSnapshot> {
    let home_label = match &selected.tenant {
        Tenant::Managed(_) => "Tenant Home",
        Tenant::Host { .. } => "Host Home",
    };
    if !crate::foundation::safe_fs::real_dir_exists(selected.home_dir(), home_label)? {
        if matches!(&selected.tenant, Tenant::Managed(_)) {
            return Ok(FileSnapshot {
                present: false,
                content: Vec::new(),
                mode: None,
            });
        }
        bail!(
            "{home_label} does not exist: {}",
            selected.home_dir().display()
        );
    }
    if !crate::foundation::safe_fs::real_dir_exists(
        &selected.agent_state_dir,
        "Agent state directory",
    )? {
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
    let file = crate::foundation::safe_fs::open_real_file(path, "configuration file")?;
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
    if !crate::foundation::safe_fs::real_file_exists(path, "Named Config file")? {
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
    if !crate::foundation::safe_fs::real_dir_exists(path, "Named Config directory")? {
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
    crate::foundation::safe_fs::ensure_real_dir(parent, "configuration parent directory")?;
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
    let write = write_temporary_file(parent, &prefix, content, mode)?;
    write.commit(path, "replace")
}

fn replace_existing_atomic(path: &Path, content: &[u8], mode: u32) -> Result<()> {
    if content.len() as u64 > MAX_CONFIG_BYTES {
        bail!("refusing oversized configuration write: {}", path.display());
    }
    let parent = path.parent().context("configuration path has no parent")?;
    if !crate::foundation::safe_fs::real_dir_exists(parent, "configuration parent directory")? {
        bail!(
            "configuration parent directory does not exist: {}",
            parent.display()
        );
    }
    if !crate::foundation::safe_fs::real_file_exists(path, "configuration file")? {
        bail!("configuration file does not exist: {}", path.display());
    }
    let prefix = temporary_file_prefix(path, "propagate-auth")?;
    let write = write_temporary_file(parent, &prefix, content, mode)?;
    write.commit(path, "replace")
}

fn write_temporary_file(
    parent: &Path,
    prefix: &str,
    content: &[u8],
    mode: u32,
) -> Result<crate::foundation::safe_fs::PreparedAtomicWrite> {
    let mut write = crate::foundation::safe_fs::PreparedAtomicWrite::new(
        parent,
        prefix,
        Some(mode),
        "configuration file",
    )?;
    write.write_all(content)?;
    Ok(write)
}

fn temporary_file_prefix(path: &Path, purpose: &str) -> Result<String> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .context("configuration file name is not valid UTF-8")?;
    Ok(format!(".{name}.aibox-{purpose}-"))
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
