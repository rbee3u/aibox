//! Safe flat Request layout, filenames, atomic writes, and path validation.

use super::reading::summary_ended_at;
use super::{RecordedHeader, SummaryMetadata};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct RequestFile {
    pub(super) schema_version: u32,
    pub(super) request_id: String,
    pub(super) kind: String,
    pub(super) method: String,
    pub(super) upstream_url: Option<String>,
    pub(super) headers: Vec<RecordedHeader>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ResponseFile {
    pub(super) schema_version: u32,
    pub(super) request_id: String,
    pub(super) kind: String,
    pub(super) http_version: String,
    pub(super) status: u16,
    pub(super) headers: Vec<RecordedHeader>,
}

pub(super) fn read_json<T: serde::de::DeserializeOwned>(path: &Path, kind: &str) -> Result<T> {
    let file = crate::foundation::safe_fs::open_real_file(path, kind)?;
    serde_json::from_reader(file).with_context(|| format!("parse {kind} {}", path.display()))
}

pub(super) fn optional_json<T: serde::de::DeserializeOwned>(
    path: &Path,
    kind: &str,
) -> Result<Option<T>> {
    if !crate::foundation::safe_fs::real_file_exists(path, kind)? {
        return Ok(None);
    }
    read_json(path, kind).map(Some)
}

pub(super) fn regular_file_length(path: &Path, kind: &str) -> Result<u64> {
    validate_regular_file(path, kind)?;
    Ok(fs::symlink_metadata(path)?.len())
}

pub(super) fn validate_regular_file(path: &Path, kind: &str) -> Result<()> {
    if !crate::foundation::safe_fs::real_file_exists(path, kind)? {
        bail!("{kind} does not exist: {}", path.display());
    }
    Ok(())
}

pub(super) fn validate_request_ancestor(root: &Path, directory: &Path) -> Result<()> {
    if directory.parent() != Some(root) {
        bail!("Request is not a direct child of the Request collection");
    }
    if !crate::foundation::safe_fs::real_dir_exists(root, "Request collection")?
        || !crate::foundation::safe_fs::real_dir_exists(directory, "Request")?
    {
        bail!("Request disappeared: {}", directory.display());
    }
    Ok(())
}

pub(super) fn validate_id(id: &str) -> Result<()> {
    let parsed = Uuid::parse_str(id).with_context(|| format!("invalid Request id: {id}"))?;
    if parsed.get_version_num() != 7 {
        bail!("Request id is not UUID v7: {id}");
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RequestDirectoryName {
    pub(super) host: String,
}

pub(super) fn parse_request_directory_name(path: &Path, id: &str) -> Result<RequestDirectoryName> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Request directory name is not valid UTF-8")?;
    let suffix = format!("-{id}");
    let prefix = name
        .strip_suffix(&suffix)
        .context("Request directory name does not match its UUID")?;
    let prefix = prefix.strip_prefix("active-").unwrap_or(prefix);
    let (timestamp, host) = prefix
        .split_once('-')
        .context("Request directory name has no host slug")?;
    let timestamp = timestamp.as_bytes();
    let timestamp_is_valid = timestamp.len() == 20
        && timestamp[8] == b'T'
        && timestamp[15] == b'.'
        && timestamp[19] == b'Z'
        && timestamp
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 15 | 19) || byte.is_ascii_digit());
    let host_is_valid = (1..=48).contains(&host.len())
        && !host.starts_with(['.', '-'])
        && !host.ends_with(['.', '-'])
        && host.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'-')
        });
    if !timestamp_is_valid || !host_is_valid {
        bail!("Request directory name is not structurally valid");
    }
    Ok(RequestDirectoryName {
        host: host.to_string(),
    })
}

pub(super) fn canonical_sort_key(
    summary: &SummaryMetadata,
    host: &str,
    id: &str,
) -> Result<String> {
    if summary.terminal {
        Ok(format!(
            "{}-{host}-{id}",
            utc_basic_at(&summary_ended_at(summary))?
        ))
    } else {
        Ok(format!(
            "active-{}-{host}-{id}",
            utc_basic_at(&summary.observed_at)?
        ))
    }
}

pub(super) fn create_private_file(path: &Path) -> Result<fs::File> {
    crate::foundation::safe_fs::create_new_file(path, "private Request file", 0o600)
}

pub(super) fn atomic_write_json(path: &Path, name: &str, value: &impl Serialize) -> Result<()> {
    let temporary = path.join(format!(".{name}.{}.tmp", Uuid::new_v4()));
    let final_path = path.join(name);
    let result = (|| -> Result<()> {
        let mut file = create_private_file(&temporary)?;
        serde_json::to_writer_pretty(&mut file, value)
            .with_context(|| format!("serialize Request metadata {}", final_path.display()))?;
        file.write_all(b"\n")?;
        file.flush()?;
        file.sync_all()?;
        crate::foundation::safe_fs::publish_atomic_file(
            &temporary,
            &final_path,
            "publish Request metadata",
        )
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(super) fn remove_controlled_request_dir(path: &Path) -> Result<()> {
    let files = validate_controlled_request_dir(path)?;
    for file in files {
        fs::remove_file(&file)
            .with_context(|| format!("delete Request file {}", file.display()))?;
    }
    fs::remove_dir(path).with_context(|| format!("delete Request {}", path.display()))
}

pub(super) fn validate_controlled_request_dir(path: &Path) -> Result<Vec<PathBuf>> {
    if !crate::foundation::safe_fs::real_dir_exists(path, "Request")? {
        bail!("Request disappeared: {}", path.display());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.file_type().is_file() {
            bail!(
                "refusing to delete Request with unsafe entry: {}",
                entry.path().display()
            );
        }
        files.push(entry.path());
    }
    Ok(files)
}

pub(super) fn restrict_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("chmod 0700 {}", path.display()))?;
    }
    Ok(())
}

pub(super) fn utc_basic_at(timestamp: &str) -> Result<String> {
    let observed = OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
        .with_context(|| format!("parse Request timestamp {timestamp}"))?;
    let format = time::format_description::parse_borrowed::<1>(
        "[year][month][day]T[hour][minute][second].[subsecond digits:3]Z",
    )
    .expect("static UTC filename format is valid");
    observed
        .format(&format)
        .context("format Request filename timestamp")
}

pub(super) fn rename_noreplace(source: &Path, target: &Path) -> Result<()> {
    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))]
    {
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            source,
            rustix::fs::CWD,
            target,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .with_context(|| format!("rename {} to {}", source.display(), target.display()))
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    )))]
    {
        let _ = (source, target);
        bail!("atomic no-clobber Request rename is unsupported on this platform")
    }
}

pub(super) fn sanitize_host(host: &str) -> String {
    let mut slug = String::with_capacity(host.len().min(48));
    for character in host.chars() {
        if slug.len() >= 48 {
            break;
        }
        let character = character.to_ascii_lowercase();
        if character.is_ascii_alphanumeric() || character == '.' || character == '-' {
            slug.push(character);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches(['.', '-']);
    if slug.is_empty() {
        "invalid".to_string()
    } else {
        slug.to_string()
    }
}

pub(super) fn safe_display_host(host: &str) -> String {
    let mut value = String::with_capacity(host.len().min(256));
    for character in host.chars().take(256) {
        if character.is_ascii_alphanumeric() || matches!(character, '.' | ':' | '-' | '[' | ']') {
            value.push(character.to_ascii_lowercase());
        } else if !value.ends_with('-') {
            value.push('-');
        }
    }
    let value = value.trim_matches(['.', '-']);
    if value.is_empty() {
        "invalid".to_string()
    } else {
        value.to_string()
    }
}

pub(super) fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub(crate) fn offset_ns(origin: Instant) -> String {
    origin.elapsed().as_nanos().to_string()
}
