//! Node.js and Coding Agent executable ownership.

use super::native::{executable_file_exists, remove_local_launcher};
use super::{ComponentStatus, validate_stable_version};
use crate::tenant::CONTAINER_HOME;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum LinkState {
    Absent,
    Symlink(PathBuf),
    Other,
}

pub(super) fn inspect_node(home: &Path) -> Result<ComponentStatus> {
    let root = home.join(".node");
    if !crate::foundation::safe_fs::real_dir_exists(&root, "Node.js root")? {
        return Ok(ComponentStatus::NotInstalled);
    }
    let releases = root.join("releases");
    if !crate::foundation::safe_fs::real_dir_exists(&releases, "Node.js release collection")? {
        return Ok(ComponentStatus::Incomplete);
    }
    let current = root.join("current");
    let target = match link_state(&current, "Node.js current release")? {
        LinkState::Absent => return Ok(ComponentStatus::Incomplete),
        LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        LinkState::Symlink(target) => target,
    };
    let Some(target) = map_home_symlink_target(home, &current, &target) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some(name) = one_relative_component(&target, &releases) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some(version) = name
        .strip_prefix('v')
        .and_then(|value| validate_stable_version(value).ok())
    else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let release = releases.join(&name);
    if !crate::foundation::safe_fs::real_dir_exists(&release, "Node.js release")? {
        return Ok(ComponentStatus::Incomplete);
    }
    let bin = release.join("bin");
    if !crate::foundation::safe_fs::real_dir_exists(&bin, "Node.js binary directory")?
        || !executable_file_exists(&bin.join("node"), "Node.js executable")?
        || !safe_file_exists_under(&bin.join("npm"), &release, "npm launcher")?
    {
        return Ok(ComponentStatus::Incomplete);
    }
    Ok(ComponentStatus::Installed {
        version: Some(version),
    })
}

pub(super) fn inspect_codex(home: &Path) -> Result<ComponentStatus> {
    let launcher = home.join(".local/bin/codex");
    let standalone = home.join(".codex/packages/standalone");
    let launcher_state = local_launcher_state(home, "codex", "Codex launcher")?;
    let standalone_exists = codex_standalone_exists(home, &standalone)?;
    if launcher_state == LinkState::Absent && !standalone_exists {
        return Ok(ComponentStatus::NotInstalled);
    }
    let launcher_target = match launcher_state {
        LinkState::Symlink(target) => target,
        LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        LinkState::Absent => return Ok(ComponentStatus::Incomplete),
    };
    if !standalone_exists {
        return Ok(ComponentStatus::Incomplete);
    }

    let current = standalone.join("current");
    let current_target = match link_state(&current, "Codex current release")? {
        LinkState::Absent => return Ok(ComponentStatus::Incomplete),
        LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        LinkState::Symlink(target) => target,
    };
    let Some(current_target) = map_home_symlink_target(home, &current, &current_target) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let releases = standalone.join("releases");
    if !crate::foundation::safe_fs::real_dir_exists(&releases, "Codex release collection")? {
        return Ok(ComponentStatus::Incomplete);
    }
    let Some(release_name) = one_relative_component(&current_target, &releases) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some(version) = codex_release_version(&release_name) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let release = releases.join(&release_name);
    if !crate::foundation::safe_fs::real_dir_exists(&release, "Codex release")? {
        return Ok(ComponentStatus::Incomplete);
    }

    let Some(launcher_target) = map_home_symlink_target(home, &launcher, &launcher_target) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let package_launcher = standalone.join("current/bin/codex");
    let legacy_launcher = standalone.join("current/codex");
    let release_executable = if launcher_target == package_launcher {
        release.join("bin/codex")
    } else if launcher_target == legacy_launcher {
        release.join("codex")
    } else {
        return Ok(ComponentStatus::Unmanaged);
    };
    if !executable_file_exists(&release_executable, "Codex executable")? {
        return Ok(ComponentStatus::Incomplete);
    }
    Ok(ComponentStatus::Installed {
        version: Some(version),
    })
}

pub(super) fn inspect_claude(home: &Path) -> Result<ComponentStatus> {
    let launcher = home.join(".local/bin/claude");
    let versions = home.join(".local/share/claude/versions");
    let launcher_state = local_launcher_state(home, "claude", "Claude launcher")?;
    let versions_exist = claude_versions_exist(home, &versions)?;
    if launcher_state == LinkState::Absent && !versions_exist {
        return Ok(ComponentStatus::NotInstalled);
    }
    let target = match launcher_state {
        LinkState::Symlink(target) => target,
        LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        LinkState::Absent => return Ok(ComponentStatus::Incomplete),
    };
    if !versions_exist {
        return Ok(ComponentStatus::Incomplete);
    }
    let Some(target) = map_home_symlink_target(home, &launcher, &target) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some(version) = one_relative_component(&target, &versions)
        .and_then(|value| validate_stable_version(&value).ok())
    else {
        return Ok(ComponentStatus::Unmanaged);
    };
    if !executable_file_exists(&versions.join(&version), "Claude executable")? {
        return Ok(ComponentStatus::Incomplete);
    }
    Ok(ComponentStatus::Installed {
        version: Some(version),
    })
}

fn codex_standalone_exists(home: &Path, standalone: &Path) -> Result<bool> {
    let packages = home.join(".codex/packages");
    if !crate::foundation::safe_fs::real_dir_exists(&home.join(".codex"), "Codex state directory")?
        || !crate::foundation::safe_fs::real_dir_exists(&packages, "Codex package directory")?
    {
        return Ok(false);
    }
    crate::foundation::safe_fs::real_dir_exists(standalone, "Codex standalone package")
}

fn claude_versions_exist(home: &Path, versions: &Path) -> Result<bool> {
    let local = home.join(".local");
    let share = local.join("share");
    let claude = share.join("claude");
    if !crate::foundation::safe_fs::real_dir_exists(&local, "Tenant-local data directory")?
        || !crate::foundation::safe_fs::real_dir_exists(
            &share,
            "Tenant-local shared data directory",
        )?
        || !crate::foundation::safe_fs::real_dir_exists(&claude, "Claude data directory")?
    {
        return Ok(false);
    }
    crate::foundation::safe_fs::real_dir_exists(versions, "Claude version collection")
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

fn local_launcher_state(home: &Path, name: &str, label: &str) -> Result<LinkState> {
    let local = home.join(".local");
    if !crate::foundation::safe_fs::real_dir_exists(&local, "Tenant-local data directory")? {
        return Ok(LinkState::Absent);
    }
    let bin = local.join("bin");
    if !crate::foundation::safe_fs::real_dir_exists(&bin, "Tenant-local binary directory")? {
        return Ok(LinkState::Absent);
    }
    link_state(&bin.join(name), label)
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

fn codex_release_version(name: &str) -> Option<String> {
    for suffix in [
        "-x86_64-unknown-linux-musl",
        "-aarch64-unknown-linux-musl",
        "-x86_64-unknown-linux-gnu",
        "-aarch64-unknown-linux-gnu",
    ] {
        if let Some(version) = name.strip_suffix(suffix) {
            return validate_stable_version(version).ok();
        }
    }
    None
}

fn safe_file_exists_under(path: &Path, root: &Path, label: &str) -> Result<bool> {
    match fs::canonicalize(path) {
        Ok(resolved) => {
            let resolved_root = fs::canonicalize(root)
                .with_context(|| format!("resolve {label} root {}", root.display()))?;
            if !resolved.starts_with(&resolved_root) {
                bail!("{label} escapes its Component release: {}", path.display());
            }
            Ok(fs::metadata(&resolved)?.file_type().is_file())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("resolve {label} {}", path.display())),
    }
}

pub(super) fn remove_node(home: &Path) -> Result<()> {
    crate::foundation::safe_fs::real_dir_exists(home, "Tenant Home")?;
    crate::foundation::safe_fs::remove_real_dir_if_exists(&home.join(".node"), "Node.js root")
}

pub(super) fn remove_codex(home: &Path) -> Result<()> {
    crate::foundation::safe_fs::real_dir_exists(home, "Tenant Home")?;
    remove_local_launcher(home, "codex", "Codex launcher")?;
    let codex = home.join(".codex");
    if !crate::foundation::safe_fs::real_dir_exists(&codex, "Codex state directory")? {
        return Ok(());
    }
    let packages = codex.join("packages");
    if !crate::foundation::safe_fs::real_dir_exists(&packages, "Codex package directory")? {
        return Ok(());
    }
    crate::foundation::safe_fs::remove_real_dir_if_exists(
        &packages.join("standalone"),
        "Codex standalone package",
    )
}

pub(super) fn remove_claude(home: &Path) -> Result<()> {
    crate::foundation::safe_fs::real_dir_exists(home, "Tenant Home")?;
    remove_local_launcher(home, "claude", "Claude launcher")?;
    let local = home.join(".local");
    if !crate::foundation::safe_fs::real_dir_exists(&local, "Tenant-local data directory")? {
        return Ok(());
    }
    let share = local.join("share");
    if !crate::foundation::safe_fs::real_dir_exists(&share, "Tenant-local shared data directory")? {
        return Ok(());
    }
    let claude = share.join("claude");
    if !crate::foundation::safe_fs::real_dir_exists(&claude, "Claude data directory")? {
        return Ok(());
    }
    crate::foundation::safe_fs::remove_real_dir_if_exists(
        &claude.join("versions"),
        "Claude version collection",
    )
}
