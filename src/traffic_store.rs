use anyhow::{bail, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

const FORMAT_VERSION: u32 = 1;
const REQUEST_JSON: &str = "request.json";
const REQUEST_BODY: &str = "request.body";
const RESPONSE_JSON: &str = "response.json";
const RESPONSE_BODY: &str = "response.body";
const RESULT_JSON: &str = "result.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct RecordedHeader {
    pub name: String,
    pub value_base64: String,
}

impl RecordedHeader {
    pub fn from_headers(headers: &axum::http::HeaderMap) -> Vec<Self> {
        headers
            .iter()
            .map(|(name, value)| Self {
                name: name.as_str().to_string(),
                value_base64: base64::engine::general_purpose::STANDARD.encode(value.as_bytes()),
            })
            .collect()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct RequestMetadata {
    pub format_version: u32,
    pub id: String,
    pub started_at: String,
    pub method: String,
    pub incoming_uri: String,
    pub upstream_url: Option<String>,
    pub http_version: String,
    pub headers: Vec<RecordedHeader>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ResponseSource {
    Upstream,
    Proxy,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ResponseMetadata {
    pub format_version: u32,
    pub source: ResponseSource,
    pub headers_at: String,
    pub status: u16,
    pub http_version: String,
    pub headers: Vec<RecordedHeader>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Outcome {
    Completed,
    Rejected,
    UpstreamError,
    ClientDisconnected,
    RecordingFailed,
    ServerShutdown,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Rejected => "rejected",
            Self::UpstreamError => "upstream_error",
            Self::ClientDisconnected => "client_disconnected",
            Self::RecordingFailed => "recording_failed",
            Self::ServerShutdown => "server_shutdown",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ErrorMetadata {
    pub kind: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct ResultMetadata {
    pub format_version: u32,
    pub ended_at: String,
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub request_body_ms: Option<u64>,
    pub ttfb_ms: Option<u64>,
    pub total_ms: u64,
    pub outcome: Outcome,
    pub error: Option<ErrorMetadata>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct RuntimeMeasurements {
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub request_body_duration: Option<Duration>,
    pub ttfb: Option<Duration>,
}

#[derive(Clone)]
pub(super) struct TrafficStore {
    root: PathBuf,
    active: Arc<Mutex<HashSet<String>>>,
}

pub(super) struct NewRecord {
    pub id: String,
    pub directory: PathBuf,
    pub request_body: fs::File,
    pub response_body: fs::File,
}

#[derive(Clone, Debug)]
pub(super) struct StoredRecord {
    pub directory: PathBuf,
    pub request: RequestMetadata,
    pub response: Option<ResponseMetadata>,
    pub result: Option<ResultMetadata>,
    pub request_body_bytes: u64,
    pub response_body_bytes: u64,
    pub active: bool,
}

impl TrafficStore {
    pub fn open(aibox_root: &Path) -> Result<Self> {
        crate::tenant::ensure_real_dir(aibox_root, "aibox root")?;
        let root = aibox_root.join("traffic");
        crate::tenant::ensure_real_dir(&root, "Traffic Record collection")?;
        restrict_dir(&root)?;
        Ok(Self {
            root,
            active: Arc::new(Mutex::new(HashSet::new())),
        })
    }

    #[cfg(test)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn root_display(&self) -> String {
        self.root.display().to_string()
    }

    pub fn begin(
        &self,
        method: &str,
        incoming_uri: &str,
        upstream_url: Option<&str>,
        version: &str,
        headers: Vec<RecordedHeader>,
        host_hint: Option<&str>,
    ) -> Result<(NewRecord, RequestMetadata)> {
        crate::tenant::real_dir_exists(&self.root, "Traffic Record collection")?;
        let id = Uuid::now_v7().to_string();
        let started_at = utc_now();
        let directory_name = format!(
            "{}-{}-{id}",
            utc_basic_now(),
            sanitize_host(host_hint.unwrap_or("invalid"))
        );
        let directory = self.root.join(directory_name);
        fs::create_dir(&directory)
            .with_context(|| format!("create Traffic Record {}", directory.display()))?;
        restrict_dir(&directory)?;

        // Publish the in-memory active marker before any record files become
        // complete enough for a concurrent management scan to observe. This
        // prevents an in-flight request from being classified as interrupted
        // and selected for deletion during the metadata/body setup window.
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone());

        let created = (|| -> Result<_> {
            let request_body = create_private_file(&directory.join(REQUEST_BODY))?;
            let response_body = create_private_file(&directory.join(RESPONSE_BODY))?;
            let request = RequestMetadata {
                format_version: FORMAT_VERSION,
                id: id.clone(),
                started_at,
                method: method.to_string(),
                incoming_uri: incoming_uri.to_string(),
                upstream_url: upstream_url.map(str::to_string),
                http_version: version.to_string(),
                headers,
            };
            atomic_write_json(&directory, REQUEST_JSON, &request)?;
            crate::tenant::sync_dir(&directory)?;
            crate::tenant::sync_dir(&self.root)?;
            Ok((request_body, response_body, request))
        })();
        let (request_body, response_body, request) = match created {
            Ok(value) => value,
            Err(error) => {
                self.active
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&id);
                let _ = remove_controlled_record_dir(&directory);
                return Err(error);
            }
        };
        Ok((
            NewRecord {
                id,
                directory,
                request_body,
                response_body,
            },
            request,
        ))
    }

    pub fn write_response(&self, directory: &Path, metadata: &ResponseMetadata) -> Result<()> {
        validate_record_ancestor(&self.root, directory)?;
        atomic_write_json(directory, RESPONSE_JSON, metadata)
    }

    pub fn finish(
        &self,
        record: &NewRecord,
        started: std::time::Instant,
        measurements: &RuntimeMeasurements,
        outcome: Outcome,
        error: Option<ErrorMetadata>,
    ) -> Result<ResultMetadata> {
        let result = ResultMetadata {
            format_version: FORMAT_VERSION,
            ended_at: utc_now(),
            request_bytes: measurements.request_bytes,
            response_bytes: measurements.response_bytes,
            request_body_ms: measurements.request_body_duration.map(duration_ms),
            ttfb_ms: measurements.ttfb.map(duration_ms),
            total_ms: duration_ms(started.elapsed()),
            outcome,
            error,
        };
        let write = atomic_write_json(&record.directory, RESULT_JSON, &result);
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&record.id);
        write?;
        Ok(result)
    }

    pub fn abandon_active(&self, id: &str) {
        self.active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(id);
    }

    pub fn scan(&self) -> Result<Vec<StoredRecord>> {
        if !crate::tenant::real_dir_exists(&self.root, "Traffic Record collection")? {
            return Ok(Vec::new());
        }
        let active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.root)
            .with_context(|| format!("read Traffic Record collection {}", self.root.display()))?
        {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    eprintln!("warning: cannot inspect Traffic Record entry: {error}");
                    continue;
                }
            };
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    eprintln!(
                        "warning: cannot inspect Traffic Record {}: {error}",
                        path.display()
                    );
                    continue;
                }
            };
            if !metadata.file_type().is_dir() {
                eprintln!(
                    "warning: ignoring unexpected Traffic entry {}",
                    path.display()
                );
                continue;
            }
            match read_record(&path, &active) {
                Ok(record) => records.push(record),
                Err(error) => eprintln!(
                    "warning: ignoring incomplete or invalid Traffic Record {}: {error:#}",
                    path.display()
                ),
            }
        }
        records.sort_by(|left, right| {
            right
                .request
                .started_at
                .cmp(&left.request.started_at)
                .then_with(|| right.request.id.cmp(&left.request.id))
        });
        Ok(records)
    }

    pub fn find(&self, id: &str) -> Result<StoredRecord> {
        validate_id(id)?;
        self.scan()?
            .into_iter()
            .find(|record| record.request.id == id)
            .with_context(|| format!("Traffic Record not found: {id}"))
    }

    pub fn open_body(&self, id: &str, response: bool, offset: u64) -> Result<(fs::File, u64)> {
        let record = self.find(id)?;
        let path = record.directory.join(if response {
            RESPONSE_BODY
        } else {
            REQUEST_BODY
        });
        validate_regular_file(&path, "Traffic body")?;
        let mut file = crate::tenant::open_real_file(&path, "Traffic body")?;
        let length = file.metadata()?.len();
        if offset > length {
            bail!("body offset {offset} exceeds current length {length}");
        }
        file.seek(SeekFrom::Start(offset))?;
        Ok((file, length))
    }

    pub fn delete_ids(&self, ids: &[String]) -> Result<usize> {
        if ids.is_empty() {
            bail!("at least one Traffic Record id is required");
        }
        let unique: HashSet<_> = ids.iter().collect();
        if unique.len() != ids.len() {
            bail!("Traffic Record ids must not be repeated");
        }
        let active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if ids.iter().any(|id| active.contains(id)) {
            bail!("active Traffic Records cannot be deleted");
        }
        let records = self.scan()?;
        let mut selected = Vec::new();
        for id in ids {
            validate_id(id)?;
            let record = records
                .iter()
                .find(|record| &record.request.id == id)
                .with_context(|| format!("Traffic Record not found: {id}"))?;
            if record.active {
                bail!("active Traffic Records cannot be deleted");
            }
            validate_record_ancestor(&self.root, &record.directory)?;
            selected.push(record.directory.clone());
        }
        for path in &selected {
            remove_controlled_record_dir(path)?;
        }
        crate::tenant::sync_dir(&self.root)?;
        Ok(selected.len())
    }

    pub fn delete_all(&self, expected: usize) -> Result<usize> {
        let records: Vec<_> = self
            .scan()?
            .into_iter()
            .filter(|record| !record.active)
            .collect();
        if records.len() != expected {
            bail!(
                "deletable Traffic Record count changed (expected {expected}, now {})",
                records.len()
            );
        }
        for record in &records {
            validate_record_ancestor(&self.root, &record.directory)?;
        }
        for record in &records {
            remove_controlled_record_dir(&record.directory)?;
        }
        crate::tenant::sync_dir(&self.root)?;
        Ok(records.len())
    }
}

fn read_record(path: &Path, active: &HashSet<String>) -> Result<StoredRecord> {
    let request: RequestMetadata = read_json(&path.join(REQUEST_JSON), "Traffic request metadata")?;
    if request.format_version != FORMAT_VERSION {
        bail!(
            "unsupported Traffic format version {}",
            request.format_version
        );
    }
    validate_id(&request.id)?;
    validate_record_directory_name(path, &request.id)?;
    let response: Option<ResponseMetadata> =
        optional_json(&path.join(RESPONSE_JSON), "Traffic response metadata")?;
    if response
        .as_ref()
        .is_some_and(|metadata| metadata.format_version != FORMAT_VERSION)
    {
        bail!("unsupported Traffic response format version");
    }
    let result: Option<ResultMetadata> =
        optional_json(&path.join(RESULT_JSON), "Traffic result metadata")?;
    if result
        .as_ref()
        .is_some_and(|metadata| metadata.format_version != FORMAT_VERSION)
    {
        bail!("unsupported Traffic result format version");
    }
    let request_body_bytes = regular_file_length(&path.join(REQUEST_BODY), "Traffic request body")?;
    let response_body_bytes =
        regular_file_length(&path.join(RESPONSE_BODY), "Traffic response body")?;
    Ok(StoredRecord {
        directory: path.to_path_buf(),
        active: result.is_none() && active.contains(&request.id),
        request,
        response,
        result,
        request_body_bytes,
        response_body_bytes,
    })
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, kind: &str) -> Result<T> {
    let file = crate::tenant::open_real_file(path, kind)?;
    serde_json::from_reader(file).with_context(|| format!("parse {kind} {}", path.display()))
}

fn optional_json<T: serde::de::DeserializeOwned>(path: &Path, kind: &str) -> Result<Option<T>> {
    if !crate::tenant::real_file_exists(path, kind)? {
        return Ok(None);
    }
    read_json(path, kind).map(Some)
}

fn regular_file_length(path: &Path, kind: &str) -> Result<u64> {
    validate_regular_file(path, kind)?;
    Ok(fs::symlink_metadata(path)?.len())
}

fn validate_regular_file(path: &Path, kind: &str) -> Result<()> {
    if !crate::tenant::real_file_exists(path, kind)? {
        bail!("{kind} does not exist: {}", path.display());
    }
    Ok(())
}

fn validate_record_ancestor(root: &Path, directory: &Path) -> Result<()> {
    if directory.parent() != Some(root) {
        bail!("Traffic Record is not a direct child of the Traffic collection");
    }
    if !crate::tenant::real_dir_exists(root, "Traffic Record collection")?
        || !crate::tenant::real_dir_exists(directory, "Traffic Record")?
    {
        bail!("Traffic Record disappeared: {}", directory.display());
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    let parsed = Uuid::parse_str(id).with_context(|| format!("invalid Traffic Record id: {id}"))?;
    if parsed.get_version_num() != 7 {
        bail!("Traffic Record id is not UUID v7: {id}");
    }
    Ok(())
}

fn validate_record_directory_name(path: &Path, id: &str) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("Traffic Record directory name is not valid UTF-8")?;
    let suffix = format!("-{id}");
    let prefix = name
        .strip_suffix(&suffix)
        .context("Traffic Record directory name does not match its UUID")?;
    let (timestamp, host) = prefix
        .split_once('-')
        .context("Traffic Record directory name has no host slug")?;
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
        bail!("Traffic Record directory name is not structurally valid");
    }
    Ok(())
}

fn create_private_file(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).read(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .with_context(|| format!("create private Traffic file {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

fn atomic_write_json(path: &Path, name: &str, value: &impl Serialize) -> Result<()> {
    let temporary = path.join(format!(".{name}.{}.tmp", Uuid::new_v4()));
    let final_path = path.join(name);
    let result = (|| -> Result<()> {
        let mut file = create_private_file(&temporary)?;
        serde_json::to_writer_pretty(&mut file, value)
            .with_context(|| format!("serialize Traffic metadata {}", final_path.display()))?;
        file.write_all(b"\n")?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temporary, &final_path).with_context(|| {
            format!(
                "publish Traffic metadata {} as {}",
                temporary.display(),
                final_path.display()
            )
        })?;
        crate::tenant::sync_dir(path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn remove_controlled_record_dir(path: &Path) -> Result<()> {
    if !crate::tenant::real_dir_exists(path, "Traffic Record")? {
        bail!("Traffic Record disappeared: {}", path.display());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.file_type().is_file() {
            bail!(
                "refusing to delete Traffic Record with unsafe entry: {}",
                entry.path().display()
            );
        }
        files.push(entry.path());
    }
    for file in files {
        fs::remove_file(&file)
            .with_context(|| format!("delete Traffic file {}", file.display()))?;
    }
    fs::remove_dir(path).with_context(|| format!("delete Traffic Record {}", path.display()))
}

fn restrict_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("chmod 0700 {}", path.display()))?;
    }
    Ok(())
}

pub(super) fn utc_now() -> String {
    let format = time::format_description::parse(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:9]Z",
    )
    .expect("static Traffic timestamp format is valid");
    OffsetDateTime::now_utc()
        .format(&format)
        .unwrap_or_else(|_| "1970-01-01T00:00:00.000000000Z".to_string())
}

fn utc_basic_now() -> String {
    let format = time::format_description::parse(
        "[year][month][day]T[hour][minute][second].[subsecond digits:3]Z",
    )
    .expect("static UTC filename format is valid");
    OffsetDateTime::now_utc()
        .format(&format)
        .unwrap_or_else(|_| "19700101T000000.000Z".to_string())
}

fn sanitize_host(host: &str) -> String {
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

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn finished_record(store: &TrafficStore, incoming_uri: &str) -> String {
        let (record, _) = store
            .begin("GET", incoming_uri, None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let id = record.id.clone();
        store
            .finish(
                &record,
                std::time::Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Rejected,
                None,
            )
            .unwrap();
        id
    }

    #[test]
    fn host_slug_and_flat_record_layout_are_safe() {
        assert_eq!(sanitize_host("API.Example.com:443"), "api.example.com-443");
        assert_eq!(sanitize_host("////"), "invalid");
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, request) = store
            .begin(
                "POST",
                "/https://example.com/v1",
                Some("https://example.com/v1"),
                "HTTP/1.1",
                Vec::new(),
                Some("example.com"),
            )
            .unwrap();
        assert_eq!(record.directory.parent(), Some(store.root()));
        assert_eq!(request.format_version, 1);
        assert!(record
            .directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("example.com"));
        assert_eq!(
            fs::metadata(store.root()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(record.directory.join(REQUEST_BODY))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn missing_terminal_metadata_is_interrupted_unless_currently_active() {
        let temp = tempfile::tempdir().unwrap();
        let first = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = first
            .begin("GET", "/bad", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        assert!(first.find(&record.id).unwrap().active);
        let restarted = TrafficStore::open(temp.path()).unwrap();
        assert!(!restarted.find(&record.id).unwrap().active);
        assert!(restarted.find(&record.id).unwrap().result.is_none());
    }

    #[test]
    fn collection_ignores_unknown_and_misnamed_direct_children() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        fs::write(store.root().join("unknown-file"), b"leave me alone").unwrap();
        fs::create_dir(store.root().join("unknown-directory")).unwrap();
        let (record, _) = store
            .begin("GET", "/bad", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let renamed = store.root().join(format!("wrong-name-{}", record.id));
        fs::rename(&record.directory, renamed).unwrap();
        assert!(store.scan().unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn opening_and_scanning_never_follow_symlinked_traffic_paths() {
        use std::os::unix::fs::symlink;

        let linked_root = tempfile::tempdir().unwrap();
        let outside_collection = tempfile::tempdir().unwrap();
        fs::write(outside_collection.path().join("keep"), b"outside").unwrap();
        symlink(
            outside_collection.path(),
            linked_root.path().join("traffic"),
        )
        .unwrap();
        let error = TrafficStore::open(linked_root.path())
            .err()
            .expect("a symlinked collection must be rejected")
            .to_string();
        assert!(error.contains("not a real directory"), "{error}");
        assert_eq!(
            fs::read(outside_collection.path().join("keep")).unwrap(),
            b"outside"
        );

        let root = tempfile::tempdir().unwrap();
        let outside_body = tempfile::tempdir().unwrap();
        let target = outside_body.path().join("request.body");
        fs::write(&target, b"secret").unwrap();
        let store = TrafficStore::open(root.path()).unwrap();
        let (record, _) = store
            .begin("POST", "/unsafe", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let body = record.directory.join(REQUEST_BODY);
        fs::remove_file(&body).unwrap();
        symlink(&target, &body).unwrap();

        assert!(store.scan().unwrap().is_empty());
        assert_eq!(fs::read(target).unwrap(), b"secret");
    }

    #[cfg(unix)]
    #[test]
    fn deletion_rejects_symlinked_record_entries_without_touching_targets() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("secret");
        fs::write(&target, b"keep").unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let (record, _) = store
            .begin("GET", "/bad", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        store
            .finish(
                &record,
                std::time::Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Rejected,
                None,
            )
            .unwrap();
        symlink(&target, record.directory.join("unsafe-link")).unwrap();
        assert!(store.delete_ids(std::slice::from_ref(&record.id)).is_err());
        assert_eq!(fs::read(target).unwrap(), b"keep");
        assert!(record.directory.exists());
    }

    #[test]
    fn delete_all_rechecks_the_non_active_count_and_preserves_active_records() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        for _ in 0..2 {
            finished_record(&store, "/bad");
        }
        let (active, _) = store
            .begin("GET", "/active", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        assert!(store.delete_all(1).is_err());
        assert_eq!(store.scan().unwrap().len(), 3);
        assert_eq!(store.delete_all(2).unwrap(), 2);
        let remaining = store.scan().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].request.id, active.id);
        assert!(remaining[0].active);
    }

    #[test]
    fn delete_ids_requires_a_unique_valid_non_active_selection_before_removing_anything() {
        let temp = tempfile::tempdir().unwrap();
        let store = TrafficStore::open(temp.path()).unwrap();
        let first = finished_record(&store, "/first");
        let second = finished_record(&store, "/second");
        let (active, _) = store
            .begin("GET", "/active", None, "HTTP/1.1", Vec::new(), None)
            .unwrap();
        let missing = Uuid::now_v7().to_string();

        for (ids, expected) in [
            (Vec::new(), "at least one"),
            (vec![first.clone(), first.clone()], "must not be repeated"),
            (
                vec![
                    first.clone(),
                    "550e8400-e29b-41d4-a716-446655440000".to_string(),
                ],
                "not UUID v7",
            ),
            (vec![first.clone(), missing], "not found"),
            (vec![first.clone(), active.id.clone()], "active Traffic"),
        ] {
            let error = store.delete_ids(&ids).unwrap_err().to_string();
            assert!(error.contains(expected), "{ids:?}: {error}");
            assert!(
                store.find(&first).is_ok(),
                "{ids:?} removed the first record"
            );
            assert!(
                store.find(&second).is_ok(),
                "{ids:?} removed the second record"
            );
        }

        assert_eq!(store.delete_ids(&[first, second]).unwrap(), 2);
        let remaining = store.scan().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].request.id, active.id);
        assert!(remaining[0].active);
    }
}
