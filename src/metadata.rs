//! Shared Tenant-and-Agent metadata storage.
//!
//! One host-only document lives beside the selected Named Config catalog.
//! Known feature sections remain typed by their owning modules while this
//! module preserves other top-level sections across atomic updates.

use crate::tenant::TenantAgent;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const METADATA_FILE: &str = "metadata.json";
const MAX_METADATA_BYTES: u64 = 16 * 1024;

/// One parsed metadata document with opaque top-level sections.
#[derive(Debug, Default)]
pub(crate) struct MetadataDocument {
    sections: Map<String, Value>,
}

impl MetadataDocument {
    /// Deserialize one typed top-level section without consuming the document.
    pub(crate) fn section<T: DeserializeOwned>(&self, name: &str) -> Result<Option<T>> {
        self.sections
            .get(name)
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .with_context(|| format!("parse metadata section '{name}'"))
    }

    /// Replace one top-level section while preserving every other JSON value.
    pub(crate) fn set_section<T: Serialize>(&mut self, name: &str, value: &T) -> Result<()> {
        self.sections.insert(
            name.to_string(),
            serde_json::to_value(value)
                .with_context(|| format!("serialize metadata section '{name}'"))?,
        );
        Ok(())
    }

    /// Serialize and validate an atomic write before other Application writes begin.
    pub(crate) fn prepare(self, selected: &TenantAgent) -> Result<PreparedMetadataWrite> {
        if !selected.named_config_catalog_exists()? {
            bail!(
                "Named Config catalog does not exist: {}",
                selected.named_config_catalog_dir().display()
            );
        }
        let path = metadata_path(selected);
        validate_existing_target(&path)?;
        let mut content = serde_json::to_vec_pretty(&Value::Object(self.sections))
            .context("serialize metadata document")?;
        content.push(b'\n');
        validate_size(content.len() as u64, &path)?;
        Ok(PreparedMetadataWrite { path, content })
    }
}

/// One fully validated metadata replacement ready for its final atomic commit.
pub(crate) struct PreparedMetadataWrite {
    path: PathBuf,
    content: Vec<u8>,
}

impl PreparedMetadataWrite {
    /// Atomically replace the metadata document and sync its catalog directory.
    pub(crate) fn commit(self) -> Result<()> {
        let parent = self.path.parent().context("metadata path has no parent")?;
        if !crate::foundation::safe_fs::real_dir_exists(parent, "Named Config catalog")? {
            bail!("Named Config catalog does not exist: {}", parent.display());
        }
        validate_existing_target(&self.path)?;
        let prefix = temporary_file_prefix(&self.path)?;
        let mut write = crate::foundation::safe_fs::PreparedAtomicWrite::new(
            parent,
            &prefix,
            Some(0o600),
            "metadata file",
        )?;
        write.write_all(&self.content)?;
        write.commit(&self.path, "replace metadata file")
    }
}

/// Read the selected metadata document without creating any filesystem state.
pub(crate) fn read(selected: &TenantAgent) -> Result<MetadataDocument> {
    if !selected.named_config_catalog_exists()? {
        return Ok(MetadataDocument::default());
    }
    let path = metadata_path(selected);
    if !crate::foundation::safe_fs::real_file_exists(&path, "metadata file")? {
        return Ok(MetadataDocument::default());
    }
    validate_private_file(&path)?;
    let file = crate::foundation::safe_fs::open_real_file(&path, "metadata file")?;
    validate_size(file.metadata()?.len(), &path)?;
    let mut content = Vec::new();
    file.take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut content)
        .with_context(|| format!("read metadata file {}", path.display()))?;
    validate_size(content.len() as u64, &path)?;
    let value: Value =
        serde_json::from_slice(&content).with_context(|| format!("parse {}", path.display()))?;
    let Value::Object(sections) = value else {
        bail!("metadata document is not a JSON object: {}", path.display());
    };
    Ok(MetadataDocument { sections })
}

/// Resolve the host-only metadata path for one Tenant and Coding Agent.
pub(crate) fn metadata_path(selected: &TenantAgent) -> PathBuf {
    selected.named_config_catalog_dir().join(METADATA_FILE)
}

fn validate_existing_target(path: &Path) -> Result<()> {
    if crate::foundation::safe_fs::real_file_exists(path, "metadata file")? {
        validate_private_file(path)?;
    }
    Ok(())
}

fn validate_private_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::symlink_metadata(path)?.permissions().mode() & 0o777;
        if mode != 0o600 {
            bail!("metadata file must have mode 0600: {}", path.display());
        }
    }
    Ok(())
}

fn validate_size(size: u64, path: &Path) -> Result<()> {
    if size > MAX_METADATA_BYTES {
        bail!(
            "metadata file exceeds {MAX_METADATA_BYTES} bytes: {}",
            path.display()
        );
    }
    Ok(())
}

fn temporary_file_prefix(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .context("metadata file name is not valid UTF-8")?;
    Ok(format!(".{name}.aibox-write-"))
}
