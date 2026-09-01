//! Safe Request collection layout, filenames, atomic writes, and path validation.

use super::summary::summary_ended_at;
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
    let parent = directory
        .parent()
        .context("Request directory has no parent")?;
    let parent_name = parent.file_name().and_then(|name| name.to_str());
    let parent_is_root = parent == root;
    let parent_is_group = parent.parent() == Some(root)
        && parent_name.is_some_and(|name| parse_request_group_name(name).is_ok());
    let parent_is_grouping_tmp =
        parent.parent() == Some(root) && parent_name.is_some_and(is_grouping_tmp_name);
    if !parent_is_root && !parent_is_group && !parent_is_grouping_tmp {
        bail!("Request is not a child of the Request collection or a Request Group");
    }
    if !crate::foundation::safe_fs::real_dir_exists(root, "Request collection")?
        || !crate::foundation::safe_fs::real_dir_exists(directory, "Request")?
    {
        bail!("Request disappeared: {}", directory.display());
    }
    if parent_is_group && !crate::foundation::safe_fs::real_dir_exists(parent, "Request Group")? {
        bail!("Request Group disappeared: {}", parent.display());
    }
    if parent_is_grouping_tmp
        && !crate::foundation::safe_fs::real_dir_exists(parent, "Request grouping directory")?
    {
        bail!(
            "Request grouping directory disappeared: {}",
            parent.display()
        );
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
    let timestamp_is_valid = is_utc_basic(timestamp.as_bytes());
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

/// A Request directory basename parsed without opening any of its files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RequestBasename {
    pub(super) name: String,
    pub(super) id: String,
    pub(super) timestamp: String,
    pub(super) active_prefixed: bool,
}

/// One ungrouped Request directory, including a child of an unfinished grouping directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RequestLocation {
    pub(super) path: PathBuf,
    pub(super) name: String,
    pub(super) id: String,
    pub(super) active_prefixed: bool,
}

/// A published Request Group as named on the collection root.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RequestGroup {
    pub(super) path: PathBuf,
    pub(super) timestamp: String,
    pub(super) counted: usize,
}

/// Direct children of the Request collection that listing and compaction understand.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct CollectionInventory {
    pub(super) hot: Vec<RequestLocation>,
    pub(super) groups: Vec<RequestGroup>,
    pub(super) grouping_tmps: Vec<PathBuf>,
}

impl CollectionInventory {
    /// Count ungrouped Request directories plus each Request Group's named count.
    pub(super) fn total(&self) -> usize {
        self.hot.len() + self.groups.iter().map(|group| group.counted).sum::<usize>()
    }
}

/// Parsed `{UTC-basic}-{count}` Request Group directory name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct RequestGroupName {
    pub(super) timestamp: String,
    pub(super) count: usize,
}

/// Prefix of an unfinished Request Group directory, followed by a UUID.
pub(super) const GROUPING_TMP_PREFIX: &str = ".grouping-";

/// Whether `timestamp` is the 20-byte UTC-basic form used in Request names.
pub(super) fn is_utc_basic(timestamp: &[u8]) -> bool {
    timestamp.len() == 20
        && timestamp[8] == b'T'
        && timestamp[15] == b'.'
        && timestamp[19] == b'Z'
        && timestamp
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 15 | 19) || byte.is_ascii_digit())
}

/// The trailing UUID v7 candidate in a directory basename, when present.
pub(super) fn directory_name_uuid_suffix(name: &str) -> Option<&str> {
    let id_start = name.len().checked_sub(36)?;
    if name.as_bytes().get(id_start.checked_sub(1)?) != Some(&b'-') {
        return None;
    }
    name.get(id_start..)
}

/// Parse a Request directory basename without opening the directory.
pub(super) fn parse_request_basename(name: &str) -> Result<RequestBasename> {
    let id = directory_name_uuid_suffix(name)
        .context("Request directory name has no UUID suffix")?
        .to_string();
    validate_id(&id)?;
    parse_request_directory_name(Path::new(name), &id)?;
    let active_prefixed = name.starts_with("active-");
    let prefix = name
        .strip_suffix(&format!("-{id}"))
        .context("Request directory name does not match its UUID")?;
    let prefix = prefix.strip_prefix("active-").unwrap_or(prefix);
    let timestamp = prefix
        .split_once('-')
        .context("Request directory name has no host slug")?
        .0
        .to_string();
    Ok(RequestBasename {
        name: name.to_string(),
        id,
        timestamp,
        active_prefixed,
    })
}

/// Parse a published Request Group directory basename.
pub(super) fn parse_request_group_name(name: &str) -> Result<RequestGroupName> {
    let (timestamp, count) = name
        .split_once('-')
        .context("Request Group directory name has no count suffix")?;
    if !is_utc_basic(timestamp.as_bytes()) {
        bail!("Request Group directory name is not structurally valid");
    }
    if count.is_empty()
        || count.starts_with('0')
        || !count.bytes().all(|byte| byte.is_ascii_digit())
    {
        bail!("Request Group directory name is not structurally valid");
    }
    let count: usize = count
        .parse()
        .context("Request Group directory name is not structurally valid")?;
    Ok(RequestGroupName {
        timestamp: timestamp.to_string(),
        count,
    })
}

/// Materialize a Request Group directory name from a frozen timestamp and count.
pub(super) fn request_group_directory_name(timestamp: &str, count: usize) -> String {
    format!("{timestamp}-{count}")
}

/// Whether `name` is an unfinished Request Group directory.
pub(super) fn is_grouping_tmp_name(name: &str) -> bool {
    name.strip_prefix(GROUPING_TMP_PREFIX)
        .is_some_and(|rest| Uuid::parse_str(rest).is_ok())
}

/// Allocate a new unfinished Request Group directory name.
pub(super) fn new_grouping_tmp_name() -> String {
    format!("{GROUPING_TMP_PREFIX}{}", Uuid::new_v4())
}

/// Classify every direct child of the Request collection.
pub(super) fn read_collection_inventory(
    root: &Path,
    mut warn: impl FnMut(&str),
) -> Result<CollectionInventory> {
    if !crate::foundation::safe_fs::real_dir_exists(root, "Request collection")? {
        return Ok(CollectionInventory::default());
    }
    let mut inventory = CollectionInventory::default();
    for entry in
        fs::read_dir(root).with_context(|| format!("read Request collection {}", root.display()))?
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                warn("request collection entry could not be inspected");
                continue;
            }
        };
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                warn("request entry could not be inspected");
                continue;
            }
        };
        if !metadata.file_type().is_dir() {
            warn("unexpected request entry ignored");
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            warn("unexpected request entry ignored");
            continue;
        };
        if is_grouping_tmp_name(name) {
            inventory.grouping_tmps.push(path.clone());
            inventory
                .hot
                .extend(request_locations_in(&path, &mut warn)?);
            continue;
        }
        if let Ok(group) = parse_request_group_name(name) {
            inventory.groups.push(RequestGroup {
                path,
                timestamp: group.timestamp,
                counted: group.count,
            });
            continue;
        }
        match parse_request_basename(name) {
            Ok(parsed) => inventory.hot.push(RequestLocation {
                path,
                name: parsed.name,
                id: parsed.id,
                active_prefixed: parsed.active_prefixed,
            }),
            Err(_) => warn("incomplete or invalid request ignored"),
        }
    }
    Ok(inventory)
}

/// List Request directories directly inside a Request Group or grouping directory.
pub(super) fn request_locations_in(
    directory: &Path,
    mut warn: impl FnMut(&str),
) -> Result<Vec<RequestLocation>> {
    let mut locations = Vec::new();
    if !crate::foundation::safe_fs::real_dir_exists(directory, "Request holder")? {
        return Ok(locations);
    }
    for entry in fs::read_dir(directory)
        .with_context(|| format!("read Request holder {}", directory.display()))?
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                warn("request entry could not be inspected");
                continue;
            }
        };
        let path = entry.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                warn("request entry could not be inspected");
                continue;
            }
        };
        if !metadata.file_type().is_dir() {
            warn("unexpected request entry ignored");
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            warn("unexpected request entry ignored");
            continue;
        };
        match parse_request_basename(name) {
            Ok(parsed) => locations.push(RequestLocation {
                path,
                name: parsed.name,
                id: parsed.id,
                active_prefixed: parsed.active_prefixed,
            }),
            Err(_) => warn("incomplete or invalid request ignored"),
        }
    }
    Ok(locations)
}

/// Walk Request directories that may hold a selected UUID, including Group interiors.
pub(super) fn visit_request_candidates(
    root: &Path,
    mut visit: impl FnMut(&Path, &str) -> Result<()>,
) -> Result<()> {
    if !crate::foundation::safe_fs::real_dir_exists(root, "Request collection")? {
        return Ok(());
    }
    for entry in
        fs::read_dir(root).with_context(|| format!("read Request collection {}", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if is_grouping_tmp_name(name) || parse_request_group_name(name).is_ok() {
            visit_named_children(&path, &mut visit)?;
            continue;
        }
        if let Some(id) = directory_name_uuid_suffix(name) {
            visit(&path, id)?;
        }
    }
    Ok(())
}

fn visit_named_children(
    directory: &Path,
    visit: &mut impl FnMut(&Path, &str) -> Result<()>,
) -> Result<()> {
    if !crate::foundation::safe_fs::real_dir_exists(directory, "Request holder")? {
        return Ok(());
    }
    for entry in fs::read_dir(directory)
        .with_context(|| format!("read Request holder {}", directory.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if let Some(id) = directory_name_uuid_suffix(name) {
            visit(&path, id)?;
        }
    }
    Ok(())
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
