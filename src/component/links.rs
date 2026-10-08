//! Component-local launcher and link inspection mechanics.

use crate::sandbox::CONTAINER_HOME;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum LinkState {
    Absent,
    Symlink(PathBuf),
    Other,
}

pub(super) fn link_state(path: &Path, label: &str) -> Result<LinkState> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Ok(LinkState::Symlink(
            fs::read_link(path).with_context(|| format!("read {label} {}", path.display()))?,
        )),
        Ok(_) => Ok(LinkState::Other),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(LinkState::Absent),
        Err(error) => Err(error).with_context(|| format!("inspect {label} {}", path.display())),
    }
}

pub(super) fn map_home_symlink_target(home: &Path, link: &Path, target: &Path) -> Option<PathBuf> {
    let mapped = if target.is_absolute() {
        if let Ok(relative) = target.strip_prefix(CONTAINER_HOME) {
            home.join(relative)
        } else if target.starts_with(home) {
            target.to_path_buf()
        } else {
            return None;
        }
    } else {
        link.parent()?.join(target)
    };
    normalize_absolute_path(&mapped).filter(|path| path.starts_with(home))
}

fn normalize_absolute_path(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                normalized.push(component.as_os_str());
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            std::path::Component::Normal(_) => normalized.push(component.as_os_str()),
        }
    }
    Some(normalized)
}

pub(super) fn one_relative_component(path: &Path, root: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut components = relative.components();
    let std::path::Component::Normal(name) = components.next()? else {
        return None;
    };
    if components.next().is_some() {
        return None;
    }
    name.to_str().map(str::to_owned)
}
