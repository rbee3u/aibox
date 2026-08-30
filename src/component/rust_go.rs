//! Rust and Go toolchain native ownership.

use super::native::{capture_limited, executable_file_exists};
use super::{ComponentStatus, validate_stable_version};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn inspect_rust(home: &Path) -> Result<ComponentStatus> {
    let rustup_home = home.join(".rustup");
    if !crate::foundation::safe_fs::real_dir_exists(&rustup_home, "Rustup Home")? {
        return Ok(ComponentStatus::NotInstalled);
    }
    let settings = capture_limited(&rustup_home.join("settings.toml"), "rustup settings")?;
    if !settings.present {
        return Ok(ComponentStatus::Incomplete);
    }
    let content =
        std::str::from_utf8(&settings.content).context("rustup settings are not UTF-8")?;
    let value: Value = toml_edit::de::from_str(content).context("parse rustup settings")?;
    let Some(toolchain) = value.get("default_toolchain").and_then(Value::as_str) else {
        return Ok(ComponentStatus::Unmanaged);
    };
    let Some(version) = stable_version_prefix(toolchain) else {
        return Ok(ComponentStatus::Unmanaged);
    };

    let cargo_home = home.join(".cargo");
    let cargo_exists = crate::foundation::safe_fs::real_dir_exists(&cargo_home, "Cargo Home")?;
    let cargo_bin_exists = cargo_exists
        && crate::foundation::safe_fs::real_dir_exists(
            &cargo_home.join("bin"),
            "Cargo binary directory",
        )?;
    let rustup_exists = cargo_bin_exists
        && executable_file_exists(&cargo_home.join("bin/rustup"), "rustup executable")?;

    let toolchains = rustup_home.join("toolchains");
    let toolchains_exist =
        crate::foundation::safe_fs::real_dir_exists(&toolchains, "Rust toolchain collection")?;
    let toolchain_dir = toolchains.join(toolchain);
    let toolchain_exists = toolchains_exist
        && crate::foundation::safe_fs::real_dir_exists(&toolchain_dir, "Rust toolchain")?;
    let toolchain_bin_exists = toolchain_exists
        && crate::foundation::safe_fs::real_dir_exists(
            &toolchain_dir.join("bin"),
            "Rust binary directory",
        )?;
    let rustc_exists = toolchain_bin_exists
        && executable_file_exists(&toolchain_dir.join("bin/rustc"), "rustc executable")?;
    let complete = rustup_exists && rustc_exists;
    if complete {
        Ok(ComponentStatus::Installed {
            version: Some(version),
        })
    } else {
        Ok(ComponentStatus::Incomplete)
    }
}

pub(super) fn inspect_go(home: &Path) -> Result<ComponentStatus> {
    let goroot = home.join(".goroot");
    if !crate::foundation::safe_fs::real_dir_exists(&goroot, "Go root")? {
        return Ok(ComponentStatus::NotInstalled);
    }
    let version_file = capture_limited(&goroot.join("VERSION"), "Go version file")?;
    if !version_file.present {
        return Ok(ComponentStatus::Incomplete);
    }
    let content = std::str::from_utf8(&version_file.content).context("Go VERSION is not UTF-8")?;
    let Some(version) = content
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("go"))
        .and_then(|version| validate_stable_version(version).ok())
    else {
        return Ok(ComponentStatus::Unmanaged);
    };
    if crate::foundation::safe_fs::real_dir_exists(&goroot.join("bin"), "Go binary directory")?
        && executable_file_exists(&goroot.join("bin/go"), "Go executable")?
    {
        Ok(ComponentStatus::Installed {
            version: Some(version),
        })
    } else {
        Ok(ComponentStatus::Incomplete)
    }
}

fn stable_version_prefix(toolchain: &str) -> Option<String> {
    let version = toolchain.split('-').next()?;
    let version = validate_stable_version(version).ok()?;
    let suffix = toolchain.strip_prefix(&version)?;
    matches!(
        suffix,
        "" | "-x86_64-unknown-linux-gnu" | "-aarch64-unknown-linux-gnu"
    )
    .then_some(version)
}

pub(super) fn remove_rust(home: &Path) -> Result<()> {
    crate::foundation::safe_fs::real_dir_exists(home, "Tenant Home")?;
    let rustup = home.join(".rustup");
    let rustup_exists = crate::foundation::safe_fs::real_dir_exists(&rustup, "Rustup Home")?;
    let cargo = home.join(".cargo");
    let cargo_exists = crate::foundation::safe_fs::real_dir_exists(&cargo, "Cargo Home")?;
    let bin = cargo.join("bin");
    let bin_exists = cargo_exists
        && crate::foundation::safe_fs::real_dir_exists(&bin, "Cargo binary directory")?;
    let proxies = if bin_exists {
        rustup_proxy_paths(&bin)?
    } else {
        Vec::new()
    };

    // Remove the cross-directory proxies first. If removal is interrupted,
    // `.rustup` remains as recognizable incomplete Component state and a
    // repeated command can finish the operation. Removing `.rustup` first
    // could leave only proxies, which inspection intentionally does not claim
    // as AIBox-owned state because they may belong to a manual Rust install.
    for proxy in proxies {
        fs::remove_file(&proxy)
            .with_context(|| format!("remove rustup proxy {}", proxy.display()))?;
    }
    if bin_exists {
        crate::foundation::safe_fs::sync_dir(&bin)?;
    }
    if rustup_exists {
        crate::foundation::safe_fs::remove_real_dir_if_exists(&rustup, "Rustup Home")?;
    }
    Ok(())
}

pub(super) fn rustup_proxy_paths(bin: &Path) -> Result<Vec<PathBuf>> {
    let rustup = bin.join("rustup");
    let rustup_metadata = match fs::symlink_metadata(&rustup) {
        Ok(metadata) if metadata.file_type().is_file() => Some(metadata),
        Ok(_) => bail!(
            "rustup executable is not a regular file: {}",
            rustup.display()
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(error)
                .with_context(|| format!("inspect rustup executable {}", rustup.display()));
        }
    };
    let mut proxies = Vec::new();
    for entry in fs::read_dir(bin)
        .with_context(|| format!("read Cargo binary directory {}", bin.display()))?
    {
        let entry =
            entry.with_context(|| format!("read Cargo binary entry in {}", bin.display()))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("inspect Cargo binary entry {}", path.display()))?;
        let owned = if path == rustup {
            rustup_metadata.is_some()
        } else if metadata.file_type().is_symlink() {
            fs::read_link(&path).with_context(|| format!("read rustup proxy {}", path.display()))?
                == Path::new("rustup")
        } else {
            rustup_metadata
                .as_ref()
                .is_some_and(|rustup| same_file_identity(&metadata, rustup))
        };
        if owned {
            proxies.push(path);
        }
    }
    // Keep the executable available until every hard-link proxy is gone so an
    // interrupted removal can rediscover ownership on the next attempt.
    proxies.sort_by_key(|path| path == &rustup);
    Ok(proxies)
}

#[cfg(unix)]
fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.file_type().is_file() && left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file_identity(_left: &fs::Metadata, _right: &fs::Metadata) -> bool {
    false
}

pub(super) fn remove_go(home: &Path) -> Result<()> {
    crate::foundation::safe_fs::remove_real_dir_if_exists(&home.join(".goroot"), "Go root")
}
