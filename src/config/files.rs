//! Bounded Config filesystem snapshots and atomic replacement.
//!
//! This module owns native file reads, revisions, permission checks, and
//! atomic writes. Config editing and application modules only orchestrate
//! these operations.

use super::{ConfigFile, MAX_CONFIG_BYTES, NamedConfigName};
use crate::foundation::safe_fs::FileSnapshot;
use crate::tenant::{Tenant, TenantAgent};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
use std::path::Path;

pub(super) fn file_revision(present: bool, content: &[u8]) -> String {
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

pub(crate) fn ensure_named_config_directory(
    selected: &TenantAgent,
    config: &NamedConfigName,
) -> Result<()> {
    let path = super::layout::named_config_dir(selected, config);
    crate::foundation::safe_fs::ensure_real_dir(&path, "Named Config directory")?;
    validate_private_directory(&path)
}

pub(super) fn write_named_config_file(
    selected: &TenantAgent,
    config: &NamedConfigName,
    file: ConfigFile,
    content: &[u8],
) -> Result<()> {
    write_atomic(
        &super::layout::named_config_file(selected, config, file),
        content,
        0o600,
    )
}

pub(super) fn capture_optional_agent_file(
    selected: &TenantAgent,
    file: &str,
) -> Result<FileSnapshot> {
    let home_label = match &selected.tenant() {
        Tenant::Managed(_) => "Tenant Home",
        Tenant::Host { .. } => "Host Home",
    };
    if !crate::foundation::safe_fs::real_dir_exists(selected.home_dir(), home_label)? {
        if matches!(&selected.tenant(), Tenant::Managed(_)) {
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
        selected.agent_state_dir(),
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

pub(super) fn snapshot_text(snapshot: &FileSnapshot, file: &str) -> Result<Option<String>> {
    if !snapshot.present {
        return Ok(None);
    }
    String::from_utf8(snapshot.content.clone())
        .map(Some)
        .with_context(|| format!("Current Config {file} is not valid UTF-8"))
}

pub(super) fn read_regular_string(path: &Path) -> Result<String> {
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

pub(super) fn validate_private_file(path: &Path) -> Result<()> {
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

pub(super) fn validate_private_directory(path: &Path) -> Result<()> {
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

pub(super) fn private_regular_file(path: &Path) -> bool {
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

pub(super) fn private_directory(path: &Path) -> bool {
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

pub(super) fn write_atomic(path: &Path, content: &[u8], mode: u32) -> Result<()> {
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

pub(super) fn replace_existing_atomic(path: &Path, content: &[u8], mode: u32) -> Result<()> {
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

pub(super) fn write_temporary_file(
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

pub(super) fn temporary_file_prefix(path: &Path, purpose: &str) -> Result<String> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .context("configuration file name is not valid UTF-8")?;
    Ok(format!(".{name}.aibox-{purpose}-"))
}
