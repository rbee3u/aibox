//! Named Config catalog discovery, lifecycle, and Current Config inspection.

use super::definition::NamedConfigDefinition;
use super::storage::{
    NamedConfigLayout, deletable_named_config_names, ensure_named_config_directory,
    inspect_deletable_named_config, inspect_named_config_directory, private_directory,
    private_regular_file, read_named_config_validation, read_regular_string,
    remove_named_config_directory, validate_private_directory, validate_private_file,
    write_named_config_file,
};
use super::{ConfigFile, NamedConfigName, storage};
use crate::tenant::{Tenant, TenantAgent};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs;

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

impl ConfigCatalogEntry {
    pub(crate) fn needs_attention(&self) -> bool {
        matches!(
            self.state,
            ConfigCatalogState::Incomplete | ConfigCatalogState::Invalid
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct CurrentConfigInspection {
    pub(crate) present_files: usize,
    pub(crate) expected_files: usize,
}

/// Create a Named Config from the selected Agent's built-in template.
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
    let config_dir = storage::named_config_dir(selected, config);
    validate_private_directory(&config_dir)?;
    let prospective_main = if layout.main {
        let path = storage::named_config_file(selected, config, ConfigFile::Main);
        validate_private_file(&path)?;
        read_regular_string(&path)?
    } else {
        selected.agent().config_template().to_string()
    };
    let prospective_auth = match selected.agent().native_auth_file() {
        Some(_) if layout.auth => {
            let path = storage::named_config_file(selected, config, ConfigFile::Auth);
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
    if !layout.auth
        && let Some(auth) = prospective_auth
    {
        write_named_config_file(selected, config, ConfigFile::Auth, auth.as_bytes())?;
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
                if private_directory(&storage::named_config_dir(selected, &config))
                    && ConfigFile::all(selected.agent()).all(|file| {
                        private_regular_file(&storage::named_config_file(selected, &config, file))
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
