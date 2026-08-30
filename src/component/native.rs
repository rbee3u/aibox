//! Shared bounded native-file mechanics for Component ownership modules.

use super::MAX_CONFIG_BYTES;
use crate::foundation::safe_fs::FileSnapshot;
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};
use std::fs;
use std::path::Path;

pub(super) fn capture_limited(path: &Path, label: &str) -> Result<FileSnapshot> {
    FileSnapshot::capture_with_limit(path, MAX_CONFIG_BYTES)
        .with_context(|| format!("inspect {label}"))
}

pub(super) fn parse_json_config(
    snapshot: &FileSnapshot,
    label: &str,
) -> Result<Map<String, Value>> {
    if !snapshot.present || snapshot.content.iter().all(u8::is_ascii_whitespace) {
        return Ok(Map::new());
    }
    let value: Value =
        serde_json::from_slice(&snapshot.content).with_context(|| format!("parse {label}"))?;
    value
        .as_object()
        .cloned()
        .with_context(|| format!("{label} must be a JSON object"))
}

pub(super) fn remove_local_launcher(home: &Path, name: &str, label: &str) -> Result<()> {
    let local = home.join(".local");
    if !crate::foundation::safe_fs::real_dir_exists(&local, "Tenant-local data directory")? {
        return Ok(());
    }
    let bin = local.join("bin");
    if !crate::foundation::safe_fs::real_dir_exists(&bin, "Tenant-local binary directory")? {
        return Ok(());
    }
    let launcher = bin.join(name);
    match fs::symlink_metadata(&launcher) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {label} {}", launcher.display())),
        Ok(metadata) if metadata.file_type().is_file() || metadata.file_type().is_symlink() => {
            fs::remove_file(&launcher)
                .with_context(|| format!("remove {label} {}", launcher.display()))?;
            crate::foundation::safe_fs::sync_dir(&bin)
        }
        Ok(_) => bail!("{label} is not a file or symlink: {}", launcher.display()),
    }
}

pub(super) fn write_atomic(path: &Path, content: &[u8], mode: Option<u32>) -> Result<()> {
    if content.len() as u64 > MAX_CONFIG_BYTES {
        bail!("refusing oversized Component write: {}", path.display());
    }
    let parent = path.parent().context("Component path has no parent")?;
    crate::foundation::safe_fs::ensure_real_dir(parent, "Component parent directory")?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            bail!("Component path is not a regular file: {}", path.display())
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let mut write = crate::foundation::safe_fs::PreparedAtomicWrite::new(
        parent,
        ".aibox-component-",
        mode,
        "Component file",
    )?;
    write.write_all(content)?;
    write.commit(path, "replace Component file")
}

#[cfg(unix)]
pub(super) fn executable_mode_is_current(mode: Option<u32>) -> bool {
    mode.is_some_and(|mode| mode & 0o777 == 0o755)
}

#[cfg(not(unix))]
pub(super) fn executable_mode_is_current(_mode: Option<u32>) -> bool {
    true
}

pub(super) fn executable_file_exists(path: &Path, label: &str) -> Result<bool> {
    if !crate::foundation::safe_fs::real_file_exists(path, label)? {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Ok(fs::symlink_metadata(path)?.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        Ok(true)
    }
}
