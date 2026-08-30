use crate::foundation::safe_fs::real_dir_exists;
use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// Resolve `$AIBOX_ROOT`, defaulting to `$HOME/.aibox`.
pub(crate) fn aibox_root() -> Result<PathBuf> {
    let root = aibox_root_path(
        std::env::var_os("AIBOX_ROOT").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )?;
    absolutize(&root)
}

fn aibox_root_path(
    configured_root: Option<&OsStr>,
    configured_home: Option<&OsStr>,
) -> Result<PathBuf> {
    match configured_root {
        Some(value) if value.is_empty() => bail!("AIBOX_ROOT is set but empty"),
        Some(value) => Ok(PathBuf::from(value)),
        None => Ok(host_home_path(configured_home)?.join(".aibox")),
    }
}

#[cfg(test)]
pub(super) fn aibox_root_from(
    configured_root: Option<&OsStr>,
    configured_home: Option<&OsStr>,
    cwd: &Path,
) -> Result<PathBuf> {
    let root = aibox_root_path(configured_root, configured_home)?;
    absolutize_from(&root, cwd)
}

pub(crate) fn host_home() -> Result<PathBuf> {
    let home = host_home_path(std::env::var_os("HOME").as_deref())?;
    absolutize(&home)
}

fn host_home_path(home: Option<&OsStr>) -> Result<PathBuf> {
    let home = home.context("HOME is not set")?;
    if home.is_empty() {
        bail!("HOME is set but empty");
    }
    Ok(PathBuf::from(home))
}

#[cfg(test)]
pub(super) fn host_home_from(home: Option<&OsStr>, cwd: &Path) -> Result<PathBuf> {
    absolutize_from(&host_home_path(home)?, cwd)
}

pub(super) fn require_host_home(home: &Path) -> Result<()> {
    if !real_dir_exists(home, "Host Home")? {
        bail!("Host Home does not exist: {}", home.display());
    }
    Ok(())
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        absolutize_from(path, Path::new(""))
    } else {
        absolutize_from(path, &std::env::current_dir()?)
    }
}

fn absolutize_from(path: &Path, cwd: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                resolved.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !resolved.pop() {
                    bail!("path escapes its filesystem root: {}", absolute.display());
                }
            }
        }
    }
    Ok(resolved)
}
