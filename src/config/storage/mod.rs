//! Config-private storage: paths, structural validation, bounded reads and replacement.
//!
//! Catalog, editing, application and propagation use this boundary; none owns
//! the shared storage protocol on behalf of the other use cases.

mod files;

use super::definition::{NamedConfigDefinition, NamedConfigValidation};
use super::{ConfigFile, MAX_CONFIG_BYTES, NamedConfigName};
use crate::tenant::TenantAgent;
use anyhow::{Context, Result, bail};
pub(super) use files::{
    capture_optional_agent_file, ensure_named_config_directory, file_revision, private_directory,
    private_regular_file, read_regular_string, replace_existing_atomic, snapshot_text,
    temporary_file_prefix, validate_private_directory, validate_private_file, write_atomic,
    write_named_config_file, write_temporary_file,
};
use std::fs;
use std::io;
use std::path::PathBuf;

pub(super) fn named_config_dir(selected: &TenantAgent, config: &NamedConfigName) -> PathBuf {
    selected.named_config_catalog_dir().join(config.as_str())
}

pub(super) fn named_config_file(
    selected: &TenantAgent,
    config: &NamedConfigName,
    file: ConfigFile,
) -> PathBuf {
    named_config_dir(selected, config).join(file.as_str(selected.agent()))
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

    pub(super) fn missing_files(self, selected: &TenantAgent) -> Vec<&'static str> {
        ConfigFile::all(selected.agent())
            .filter(|file| match file {
                ConfigFile::Main => !self.main,
                ConfigFile::Auth => !self.auth,
            })
            .map(|file| file.as_str(selected.agent()))
            .collect()
    }
}

pub(super) fn ensure_safe_named_config(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<()> {
    if inspect_named_config_directory(selected, config)?.is_none() {
        bail!("Named Config '{config}' does not exist");
    }
    validate_private_directory(&named_config_dir(selected, config))?;
    for file in ConfigFile::all(selected.agent()) {
        let path = named_config_file(selected, config, file);
        if crate::foundation::safe_fs::real_file_exists(&path, "Named Config file")? {
            validate_private_file(&path)?;
        }
    }
    Ok(())
}

pub(super) fn read_named_config_definition(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<NamedConfigDefinition> {
    Ok(read_named_config_validation(selected, config)?.definition)
}

pub(super) fn read_named_config_validation(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<NamedConfigValidation> {
    ensure_complete_named_config(selected, config)?;
    let main = read_regular_string(&named_config_file(selected, config, ConfigFile::Main))?;
    let auth = selected
        .agent()
        .native_auth_file()
        .map(|_| read_regular_string(&named_config_file(selected, config, ConfigFile::Auth)))
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
    validate_private_directory(&named_config_dir(selected, config))?;
    for file in ConfigFile::all(selected.agent()) {
        validate_private_file(&named_config_file(selected, config, file))?;
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
    validate_private_directory(&named_config_dir(selected, config))?;
    validate_private_file(&named_config_file(selected, config, ConfigFile::Main))?;
    Ok(())
}

pub(super) fn inspect_named_config_directory(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<Option<NamedConfigLayout>> {
    if !selected.named_config_catalog_exists()? {
        return Ok(None);
    }
    let path = named_config_dir(selected, config);
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

pub(super) fn deletable_named_config_names(selected: &TenantAgent) -> Result<Vec<NamedConfigName>> {
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

pub(super) fn inspect_deletable_named_config(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<bool> {
    if !selected.named_config_catalog_exists()? {
        return Ok(false);
    }
    let path = named_config_dir(selected, config);
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

pub(super) fn remove_named_config_directory(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<()> {
    if !inspect_deletable_named_config(selected, config)? {
        return Ok(());
    }
    for file in ConfigFile::all(selected.agent()) {
        crate::foundation::safe_fs::remove_real_file_if_exists(
            &named_config_file(selected, config, file),
            "Named Config file",
        )?;
    }
    let path = named_config_dir(selected, config);
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
