//! Python and uv native ownership.

use super::native::{
    capture_limited, executable_file_exists, executable_mode_is_current, remove_local_launcher,
};
use super::node_agent::{LinkState, link_state, map_home_symlink_target, one_relative_component};
use super::{ComponentStatus, validate_stable_version};
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PythonLauncherState {
    Absent,
    Owned,
    Repairable,
    Foreign,
}

pub(super) fn inspect_python(home: &Path) -> Result<ComponentStatus> {
    let root = home.join(".python");
    let root_exists = match real_directory_entry(&root, "Python toolchain root")? {
        Some(true) => true,
        Some(false) => return Ok(ComponentStatus::Unmanaged),
        None => false,
    };

    let mut launcher_names = vec![
        "uv".to_string(),
        "uvx".to_string(),
        "python".to_string(),
        "python3".to_string(),
        "pip".to_string(),
        "pip3".to_string(),
    ];
    launcher_names.extend(python_versioned_launcher_names(home)?);
    launcher_names.sort();
    launcher_names.dedup();

    let launcher_states = launcher_names
        .iter()
        .map(|name| python_launcher_state(home, name))
        .collect::<Result<Vec<_>>>()?;
    if launcher_states.contains(&PythonLauncherState::Foreign) {
        return Ok(ComponentStatus::Unmanaged);
    }
    let has_owned_launcher = launcher_states.contains(&PythonLauncherState::Owned);
    if !root_exists {
        return Ok(if has_owned_launcher {
            ComponentStatus::Incomplete
        } else {
            ComponentStatus::NotInstalled
        });
    }

    let uv_releases = root.join("uv/releases");
    let python_releases = root.join("cpython/releases");
    let generations = root.join("generations");
    let python_bin = root.join("bin");
    for (path, label) in [
        (&uv_releases, "uv release collection"),
        (&python_releases, "CPython release collection"),
        (&generations, "Python generation collection"),
        (&python_bin, "uv Python launcher directory"),
    ] {
        match real_directory_entry(path, label)? {
            Some(true) => {}
            Some(false) => return Ok(ComponentStatus::Unmanaged),
            None => return Ok(ComponentStatus::Incomplete),
        }
    }

    let current = root.join("current");
    let current_target = match link_state(&current, "Python current generation")? {
        LinkState::Absent => return Ok(ComponentStatus::Incomplete),
        LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        LinkState::Symlink(target) => target,
    };
    let Some(current_target) = map_home_symlink_target(home, &current, &current_target) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some(generation_name) = one_relative_component(&current_target, &generations) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some((python_version, uv_version, platform)) = python_generation_versions(&generation_name)
    else {
        return Ok(ComponentStatus::Unmanaged);
    };
    if Some(platform.as_str()) != expected_python_platform() {
        return Ok(ComponentStatus::Unmanaged);
    }

    let generation = generations.join(&generation_name);
    match real_directory_entry(&generation, "active Python generation")? {
        Some(true) => {}
        Some(false) => return Ok(ComponentStatus::Unmanaged),
        None => return Ok(ComponentStatus::Incomplete),
    }
    let generation_bin = generation.join("bin");
    match real_directory_entry(&generation_bin, "active Python generation binaries")? {
        Some(true) => {}
        Some(false) => return Ok(ComponentStatus::Unmanaged),
        None => return Ok(ComponentStatus::Incomplete),
    }

    let uv_release = uv_releases.join(format!("v{uv_version}"));
    match real_directory_entry(&uv_release, "active uv release")? {
        Some(true) => {}
        Some(false) => return Ok(ComponentStatus::Unmanaged),
        None => return Ok(ComponentStatus::Incomplete),
    }
    for name in ["uv", "uvx"] {
        let path = generation_bin.join(name);
        let target = match link_state(&path, "Python generation uv launcher")? {
            LinkState::Symlink(target) => target,
            LinkState::Absent => return Ok(ComponentStatus::Incomplete),
            LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        };
        let Some(target) = map_home_symlink_target(home, &path, &target) else {
            return Ok(ComponentStatus::Unmanaged);
        };
        if target != uv_release.join(name) {
            return Ok(ComponentStatus::Unmanaged);
        }
        if !executable_file_exists(&target, "active uv executable")? {
            return Ok(ComponentStatus::Incomplete);
        }
    }

    let python_path = generation_bin.join("python");
    let python_target = match link_state(&python_path, "active Python executable")? {
        LinkState::Symlink(target) => target,
        LinkState::Absent => return Ok(ComponentStatus::Incomplete),
        LinkState::Other => return Ok(ComponentStatus::Unmanaged),
    };
    let Some(python_target) = map_home_symlink_target(home, &python_path, &python_target) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    if python_release_version_for_executable(&python_target, &python_releases, &platform).as_deref()
        != Some(&python_version)
    {
        return Ok(ComponentStatus::Unmanaged);
    }
    if !executable_file_exists(&python_target, "active CPython executable")? {
        return Ok(ComponentStatus::Incomplete);
    }

    let minor = python_version
        .rsplit_once('.')
        .map(|(minor, _)| minor)
        .context("validated Python version has no patch component")?;
    match real_file_entry(
        &generation.join("pyvenv.cfg"),
        "Python generation venv marker",
    )? {
        Some(true) => {}
        Some(false) => return Ok(ComponentStatus::Unmanaged),
        None => return Ok(ComponentStatus::Incomplete),
    }
    let pip_package = generation
        .join("lib")
        .join(format!("python{minor}"))
        .join("site-packages/pip");
    match real_directory_entry(&pip_package, "Python generation pip package")? {
        Some(true) => {}
        Some(false) => return Ok(ComponentStatus::Unmanaged),
        None => return Ok(ComponentStatus::Incomplete),
    }
    for name in ["python3", &format!("python{minor}")] {
        let path = generation_bin.join(name);
        let target = match link_state(&path, "Python generation launcher")? {
            LinkState::Symlink(target) => target,
            LinkState::Absent => return Ok(ComponentStatus::Incomplete),
            LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        };
        if map_home_symlink_target(home, &path, &target).as_ref() != Some(&python_target) {
            return Ok(ComponentStatus::Unmanaged);
        }
    }
    for name in ["pip", "pip3"] {
        if !executable_file_exists(&generation_bin.join(name), "pip launcher")? {
            return Ok(ComponentStatus::Incomplete);
        }
    }

    for name in [
        "uv".to_string(),
        "uvx".to_string(),
        "python".to_string(),
        "python3".to_string(),
        format!("python{minor}"),
        "pip".to_string(),
        "pip3".to_string(),
    ] {
        if python_launcher_state(home, &name)? != PythonLauncherState::Owned {
            return Ok(ComponentStatus::Incomplete);
        }
    }
    let active_versioned_launcher = format!("python{minor}");
    for name in launcher_names
        .iter()
        .filter(|name| name.starts_with("python3.") && name.as_str() != active_versioned_launcher)
    {
        let path = generation_bin.join(name);
        let target = match link_state(&path, "historical Python generation launcher")? {
            LinkState::Symlink(target) => target,
            LinkState::Absent => return Ok(ComponentStatus::Incomplete),
            LinkState::Other => return Ok(ComponentStatus::Unmanaged),
        };
        let Some(target) = map_home_symlink_target(home, &path, &target) else {
            return Ok(ComponentStatus::Unmanaged);
        };
        let Some(version) =
            python_release_version_for_executable(&target, &python_releases, &platform)
        else {
            return Ok(ComponentStatus::Unmanaged);
        };
        if format!("python{}", version.rsplit_once('.').unwrap().0) != *name {
            return Ok(ComponentStatus::Unmanaged);
        }
        if !executable_file_exists(&target, "historical CPython executable")? {
            return Ok(ComponentStatus::Incomplete);
        }
    }

    Ok(ComponentStatus::Installed {
        version: Some(python_version),
    })
}

fn real_directory_entry(path: &Path, label: &str) -> Result<Option<bool>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata.file_type().is_dir())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("inspect {label} {}", path.display())),
    }
}

fn real_file_entry(path: &Path, label: &str) -> Result<Option<bool>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata.file_type().is_file())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("inspect {label} {}", path.display())),
    }
}

fn python_versioned_launcher_names(home: &Path) -> Result<Vec<String>> {
    let local = home.join(".local");
    if real_directory_entry(&local, "Tenant-local data directory")? != Some(true) {
        return Ok(Vec::new());
    }
    let bin = local.join("bin");
    if real_directory_entry(&bin, "Tenant-local binary directory")? != Some(true) {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&bin)
        .with_context(|| format!("list Tenant-local binary directory {}", bin.display()))?
    {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if name.strip_prefix("python3.").is_some_and(|minor| {
            !minor.is_empty() && minor.bytes().all(|byte| byte.is_ascii_digit())
        }) {
            names.push(name);
        }
    }
    Ok(names)
}

fn python_launcher_state(home: &Path, name: &str) -> Result<PythonLauncherState> {
    let local = home.join(".local");
    match real_directory_entry(&local, "Tenant-local data directory")? {
        None => return Ok(PythonLauncherState::Absent),
        Some(false) => return Ok(PythonLauncherState::Foreign),
        Some(true) => {}
    }
    let bin = local.join("bin");
    match real_directory_entry(&bin, "Tenant-local binary directory")? {
        None => return Ok(PythonLauncherState::Absent),
        Some(false) => return Ok(PythonLauncherState::Foreign),
        Some(true) => {}
    }
    let launcher = bin.join(name);
    let metadata = match fs::symlink_metadata(&launcher) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PythonLauncherState::Absent);
        }
        Err(error) => {
            return Err(error).with_context(|| {
                format!("inspect Python toolchain launcher {}", launcher.display())
            });
        }
        Ok(metadata) => metadata,
    };
    let wrapper = python_launcher_wrapper(name);
    if metadata.file_type().is_file() {
        let snapshot = capture_limited(&launcher, "Python toolchain launcher")?;
        return Ok(
            if wrapper.as_deref() == Some(snapshot.content.as_slice())
                && executable_mode_is_current(snapshot.mode)
            {
                PythonLauncherState::Owned
            } else {
                PythonLauncherState::Foreign
            },
        );
    }
    if !metadata.file_type().is_symlink() {
        return Ok(PythonLauncherState::Foreign);
    }
    let target = fs::read_link(&launcher)
        .with_context(|| format!("read Python toolchain launcher {}", launcher.display()))?;
    let Some(target) = map_home_symlink_target(home, &launcher, &target) else {
        return Ok(PythonLauncherState::Foreign);
    };
    let expected = home.join(".python/current/bin").join(name);
    Ok(if target == expected {
        if wrapper.is_some() {
            PythonLauncherState::Repairable
        } else {
            PythonLauncherState::Owned
        }
    } else {
        PythonLauncherState::Foreign
    })
}

pub(super) fn python_launcher_wrapper(name: &str) -> Option<Vec<u8>> {
    let versioned = name
        .strip_prefix("python3.")
        .is_some_and(|minor| !minor.is_empty() && minor.bytes().all(|byte| byte.is_ascii_digit()));
    if name != "python" && name != "python3" && !versioned {
        return None;
    }
    Some(
        format!(
            "#!/usr/bin/env bash\nset -euo pipefail\nexec \"$HOME/.python/current/bin/{name}\" \"$@\"\n"
        )
        .into_bytes(),
    )
}

fn python_generation_versions(name: &str) -> Option<(String, String, String)> {
    let mut parts = name.split("__");
    let python = parts.next()?.strip_prefix("python-")?;
    let uv = parts.next()?.strip_prefix("uv-")?;
    let platform = parts.next()?;
    let nonce = parts.next()?;
    if parts.next().is_some()
        || !nonce.split_once('-').is_some_and(|(left, right)| {
            !left.is_empty()
                && !right.is_empty()
                && left.bytes().all(|byte| byte.is_ascii_digit())
                && right.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return None;
    }
    Some((
        validate_stable_version(python).ok()?,
        validate_stable_version(uv).ok()?,
        platform.to_string(),
    ))
}

pub(super) fn expected_python_platform() -> Option<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Some("x86_64-unknown-linux-gnu"),
        "aarch64" => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

fn python_release_version_for_executable(
    executable: &Path,
    releases: &Path,
    platform: &str,
) -> Option<String> {
    let relative = match executable.strip_prefix(releases) {
        Ok(relative) => relative,
        Err(_) => return None,
    };
    let mut parts = relative.components();
    let (
        Some(std::path::Component::Normal(release)),
        Some(std::path::Component::Normal(bin)),
        Some(std::path::Component::Normal(executable_name)),
    ) = (parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    if parts.next().is_some()
        || bin != "bin"
        || !executable_name.to_string_lossy().starts_with("python")
    {
        return None;
    }
    let architecture = match platform {
        "x86_64-unknown-linux-gnu" => "x86_64",
        "aarch64-unknown-linux-gnu" => "aarch64",
        _ => return None,
    };
    let release = release.to_str()?;
    let version = release
        .strip_prefix("cpython-")?
        .strip_suffix(&format!("-linux-{architecture}-gnu"))?;
    validate_stable_version(version).ok()
}

pub(super) fn remove_python(home: &Path) -> Result<()> {
    crate::foundation::safe_fs::real_dir_exists(home, "Tenant Home")?;
    let mut launchers = vec![
        "uv".to_string(),
        "uvx".to_string(),
        "python".to_string(),
        "python3".to_string(),
        "pip".to_string(),
        "pip3".to_string(),
    ];
    launchers.extend(python_versioned_launcher_names(home)?);
    launchers.sort();
    launchers.dedup();
    for launcher in launchers {
        remove_local_launcher(home, &launcher, "Python toolchain launcher")?;
    }
    crate::foundation::safe_fs::remove_real_dir_if_exists(
        &home.join(".python"),
        "Python toolchain root",
    )
}
