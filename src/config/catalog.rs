//! Named Config catalog discovery, lifecycle, and structural validation.

use super::definition::{NamedConfigDefinition, NamedConfigValidation};
use super::files::{
    ensure_named_config_directory, private_directory, private_regular_file, read_regular_string,
    validate_private_directory, validate_private_file, write_named_config_file,
};
use super::{ConfigFile, NamedConfigName, layout};
use crate::tenant::{Tenant, TenantAgent};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;
use std::io;

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
pub(super) struct NamedConfigLayout {
    pub(super) main: bool,
    pub(super) auth: bool,
}

impl NamedConfigLayout {
    pub(super) fn complete(self, selected: &TenantAgent) -> bool {
        self.main && (selected.agent().native_auth_file().is_none() || self.auth)
    }

    fn missing_files(self, selected: &TenantAgent) -> Vec<&'static str> {
        ConfigFile::all(selected.agent())
            .filter(|file| match file {
                ConfigFile::Main => !self.main,
                ConfigFile::Auth => !self.auth,
            })
            .map(|file| file.as_str(selected.agent()))
            .collect()
    }
}

/// Create a Named Config from the selected Coding Agent's built-in template.
pub(crate) fn create_named_config(selected: &TenantAgent, config: &NamedConfigName) -> Result<()> {
    selected.ensure_named_config_catalog()?;

    if let Some(layout) = inspect_named_config_directory(selected, config)? {
        if layout.complete(selected) {
            bail!("Named Config '{config}' already exists");
        }
        return repair_incomplete_named_config(selected, config, layout);
    }

    let prospective_main = selected.agent().config_template().to_string();
    let prospective_auth = selected.agent().config_auth_template().map(str::to_string);
    NamedConfigDefinition::parse(
        selected.agent(),
        &prospective_main,
        prospective_auth.as_deref(),
    )
    .context("validate built-in Named Config template")?;
    ensure_named_config_directory(selected, config)?;
    write_named_config_file(
        selected,
        config,
        ConfigFile::Main,
        prospective_main.as_bytes(),
    )?;
    if let Some(auth) = prospective_auth {
        write_named_config_file(selected, config, ConfigFile::Auth, auth.as_bytes())?;
    }
    Ok(())
}

fn repair_incomplete_named_config(
    selected: &TenantAgent,
    config: &NamedConfigName,
    layout: NamedConfigLayout,
) -> Result<()> {
    let config_dir = layout::named_config_dir(selected, config);
    validate_private_directory(&config_dir)?;
    let prospective_main = if layout.main {
        let path = layout::named_config_file(selected, config, ConfigFile::Main);
        validate_private_file(&path)?;
        read_regular_string(&path)?
    } else {
        selected.agent().config_template().to_string()
    };
    let prospective_auth = match selected.agent().native_auth_file() {
        Some(_) if layout.auth => {
            let path = layout::named_config_file(selected, config, ConfigFile::Auth);
            validate_private_file(&path)?;
            Some(read_regular_string(&path)?)
        }
        Some(_) => Some(
            selected
                .agent()
                .config_auth_template()
                .expect("agent with auth file has auth template")
                .to_string(),
        ),
        None => None,
    };
    NamedConfigDefinition::parse(
        selected.agent(),
        &prospective_main,
        prospective_auth.as_deref(),
    )
    .with_context(|| format!("validate incomplete Named Config '{config}'"))?;
    if !layout.main {
        write_named_config_file(
            selected,
            config,
            ConfigFile::Main,
            prospective_main.as_bytes(),
        )?;
    }
    if !layout.auth {
        if selected.agent().native_auth_file().is_none() {
            return Ok(());
        }
        write_named_config_file(
            selected,
            config,
            ConfigFile::Auth,
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
        let Ok(config) = NamedConfigName::parse(&name) else {
            continue;
        };
        let (state, detail, warnings) = match inspect_named_config_directory(selected, &config) {
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
                if private_directory(&layout::named_config_dir(selected, &config))
                    && ConfigFile::all(selected.agent()).all(|file| {
                        private_regular_file(&layout::named_config_file(selected, &config, file))
                    }) =>
            {
                match read_named_config_validation(selected, &config) {
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
    let expected_files = selected.agent().config_files().len();
    let home_label = match &selected.tenant() {
        Tenant::Managed(_) => "Tenant Home",
        Tenant::Host { .. } => "Host Home",
    };
    if !crate::foundation::safe_fs::real_dir_exists(selected.home_dir(), home_label)?
        || !crate::foundation::safe_fs::real_dir_exists(
            selected.agent_state_dir(),
            "Agent state directory",
        )?
    {
        return Ok(CurrentConfigInspection {
            present_files: 0,
            expected_files,
        });
    }
    let mut present_files = 0;
    for file in selected.agent().config_files() {
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

pub(super) fn ensure_safe_named_config(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<()> {
    let Some(layout) = inspect_named_config_directory(selected, config)? else {
        bail!("Named Config '{config}' does not exist");
    };
    let _ = layout;
    validate_private_directory(&layout::named_config_dir(selected, config))?;
    for file in ConfigFile::all(selected.agent()) {
        let path = layout::named_config_file(selected, config, file);
        if crate::foundation::safe_fs::real_file_exists(&path, "Named Config file")? {
            validate_private_file(&path)?;
        }
    }
    Ok(())
}

/// Delete explicitly selected Named Configs or every safe Named Config directory.
pub(crate) fn delete_named_configs(
    selected: &TenantAgent,
    configs: &[NamedConfigName],
    all: bool,
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

pub(super) fn read_named_config_definition(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<NamedConfigDefinition> {
    Ok(read_named_config_validation(selected, config)?.definition)
}

fn read_named_config_validation(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<NamedConfigValidation> {
    ensure_complete_named_config(selected, config)?;
    let main = read_regular_string(&layout::named_config_file(
        selected,
        config,
        ConfigFile::Main,
    ))?;
    let auth = selected
        .agent()
        .native_auth_file()
        .map(|_| {
            read_regular_string(&layout::named_config_file(
                selected,
                config,
                ConfigFile::Auth,
            ))
        })
        .transpose()?;
    NamedConfigDefinition::parse_with_warnings(selected.agent(), &main, auth.as_deref())
        .with_context(|| format!("parse Named Config '{config}'"))
}

fn ensure_complete_named_config(selected: &TenantAgent, config: &NamedConfigName) -> Result<()> {
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
    validate_private_directory(&layout::named_config_dir(selected, config))?;
    for file in ConfigFile::all(selected.agent()) {
        validate_private_file(&layout::named_config_file(selected, config, file))?;
    }
    Ok(())
}

pub(super) fn ensure_named_config_main(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<()> {
    let Some(layout) = inspect_named_config_directory(selected, config)? else {
        bail!("Named Config '{config}' does not exist");
    };
    if !layout.main {
        bail!(
            "Named Config '{config}' is incomplete: missing {}",
            selected.agent().main_config_file()
        );
    }
    validate_private_directory(&layout::named_config_dir(selected, config))?;
    validate_private_file(&layout::named_config_file(
        selected,
        config,
        ConfigFile::Main,
    ))?;
    Ok(())
}

pub(super) fn inspect_named_config_directory(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<Option<NamedConfigLayout>> {
    if !selected.named_config_catalog_exists()? {
        return Ok(None);
    }
    let path = layout::named_config_dir(selected, config);
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
        if name == selected.agent().main_config_file() {
            layout.main = true;
        } else if selected.agent().native_auth_file() == Some(name.as_str()) {
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
    selected.agent().config_files().iter().any(|file| {
        ["write", "edit", "propagate-auth"].iter().any(|purpose| {
            let prefix = format!(".{file}.aibox-{purpose}-");
            name.strip_prefix(&prefix).is_some_and(|suffix| {
                suffix.len() == 6 && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
        })
    })
}

fn deletable_named_config_names(selected: &TenantAgent) -> Result<Vec<NamedConfigName>> {
    if !selected.named_config_catalog_exists()? {
        return Ok(Vec::new());
    }
    let mut configs = Vec::new();
    for entry in fs::read_dir(selected.named_config_catalog_dir())? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(name) = NamedConfigName::parse(&name) else {
            continue;
        };
        if inspect_deletable_named_config(selected, &name)? {
            configs.push(name);
        }
    }
    configs.sort();
    Ok(configs)
}

fn inspect_deletable_named_config(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<bool> {
    if !selected.named_config_catalog_exists()? {
        return Ok(false);
    }
    let path = layout::named_config_dir(selected, config);
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
        if !selected.agent().config_files().contains(&name.as_str())
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

fn remove_named_config_directory(selected: &TenantAgent, config: &NamedConfigName) -> Result<()> {
    if !inspect_deletable_named_config(selected, config)? {
        return Ok(());
    }
    for file in ConfigFile::all(selected.agent()) {
        crate::foundation::safe_fs::remove_real_file_if_exists(
            &layout::named_config_file(selected, config, file),
            "Named Config file",
        )?;
    }
    let path = layout::named_config_dir(selected, config);
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
