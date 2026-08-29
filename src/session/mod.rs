//! Browse saved Sessions directly from a Tenant Home or Host Home without
//! starting a container. Discovery, id resolution, listing, and deletion are shared;
//! [`SessionBackend`] isolates the two Coding Agents' Transcript formats.
//! Strict discovery protects Console detail and deletion from partial views,
//! while listing can report traversal errors alongside readable Sessions.

pub(crate) mod claude;
pub(crate) mod codex;

use crate::agent::AgentKind;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
#[cfg(unix)]
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead, Read};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

// Transcripts stream line by line, but a container-written JSONL record still
// needs a bound before it is buffered for parsing.
const MAX_TRANSCRIPT_LINE_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const UUID_TEXT_LEN: usize = 36;
const UUID_SUFFIX_LEN: usize = 12;

fn terminal_safe(value: &str) -> String {
    terminal_safe_with(value, |_| false)
}

fn terminal_safe_with(value: &str, keep_control: impl Fn(char) -> bool) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() && !keep_control(character) {
            output.extend(character.escape_default());
        } else {
            output.push(character);
        }
    }
    output
}

fn safe_path(path: &Path) -> String {
    terminal_safe(&path.to_string_lossy())
}

/// Resolve a Transcript directory only through real directory entries beneath
/// the selected Home. The Home is writable by a Coding Agent, so following a
/// `.claude`/`.codex` ancestor symlink it planted could make Console deletion
/// remove Transcripts outside the Tenant.
pub(crate) fn checked_session_dir(home: &Path, components: &[&str]) -> Result<Option<PathBuf>> {
    let mut path = home.to_path_buf();
    if !crate::foundation::safe_fs::real_dir_exists(&path, "tenant home")? {
        return Ok(None);
    }
    for component in components {
        path.push(component);
        if !crate::foundation::safe_fs::real_dir_exists(&path, "session directory")? {
            return Ok(None);
        }
    }
    Ok(Some(path))
}

/// Transcript discovery for Console Session listing: usable files plus non-fatal walk
/// errors that should be reported without hiding every readable transcript.
#[derive(Default)]
pub(crate) struct SessionDiscovery {
    /// Transcript files that were discovered safely.
    pub files: Vec<PathBuf>,
    /// Non-fatal traversal failures to report alongside partial list results.
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct SessionDiscoverySummary {
    pub(crate) count: usize,
    pub(crate) warnings: Vec<String>,
    pub(crate) partial: bool,
}

/// Whether a walked entry is a Transcript we want: a regular `.jsonl` file
/// whose name passes `keep`. Do not follow a Transcript-shaped symlink created
/// inside the selected Home: host-side Session access must stay beneath
/// the selected Home. Shared by the strict and tolerant walks so they cannot
/// drift on which files count.
fn is_wanted_transcript(entry: &walkdir::DirEntry, keep: &impl Fn(&str) -> bool) -> bool {
    entry.file_type().is_file() && has_wanted_transcript_name(entry.path(), keep)
}

fn has_wanted_transcript_name(path: &Path, keep: &impl Fn(&str) -> bool) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "jsonl")
        && path
            .file_name()
            .is_some_and(|name| keep(&name.to_string_lossy()))
}

/// Collect every `.jsonl` transcript under `base` (recursively), keeping only
/// those whose file name passes `keep`. Empty if `base` isn't a directory. Shared
/// by both backends' `files()`; they differ only in the base dir and the filter
/// (Claude keeps all, Codex keeps `rollout-` names).
pub(crate) fn walk_jsonl(base: &Path, keep: impl Fn(&str) -> bool) -> Result<Vec<PathBuf>> {
    if !session_dir_exists(base)? {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(base) {
        let entry = entry.map_err(|error| {
            anyhow::anyhow!(
                "{}",
                terminal_safe(&format!(
                    "walk session directory {}: {error}",
                    safe_path(base)
                ))
            )
        })?;
        if is_wanted_transcript(&entry, &keep) {
            out.push(entry.path().to_path_buf());
        }
    }
    Ok(out)
}

/// Tolerant counterpart to [`walk_jsonl`]: return readable transcripts plus
/// child traversal errors. An unsafe or unreadable `base` itself still fails.
pub(crate) fn walk_jsonl_tolerant(
    base: &Path,
    keep: impl Fn(&str) -> bool,
) -> Result<SessionDiscovery> {
    if !session_dir_exists(base)? {
        return Ok(SessionDiscovery::default());
    }
    let mut out = SessionDiscovery::default();
    for entry in walkdir::WalkDir::new(base) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                out.errors.push(terminal_safe(&format!(
                    "walk session directory {}: {error}",
                    safe_path(base)
                )));
                continue;
            }
        };
        if is_wanted_transcript(&entry, &keep) {
            out.files.push(entry.path().to_path_buf());
        }
    }
    Ok(out)
}

fn session_dir_exists(base: &Path) -> Result<bool> {
    match fs::symlink_metadata(base) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(true),
        Ok(_) => bail!("session path is not a directory: {}", safe_path(base)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("inspect session directory {}", safe_path(base)))
        }
    }
}

fn try_for_each_json_line(
    home: &Path,
    path: &Path,
    visit: impl FnMut(&Value) -> Result<bool>,
) -> Result<usize> {
    try_for_each_json_line_with_limit(home, path, MAX_TRANSCRIPT_LINE_BYTES, visit)
}

/// Stream parsed JSONL records to `visit`, returning the number of malformed
/// records skipped. A line that is not valid UTF-8 JSON counts as a malformed
/// record; open, size-limit, and I/O read failures remain errors.
///
/// A Tenant's Transcripts can be hundreds of megabytes and Session listing
/// visits every one, so neither a complete file nor all parsed records are held
/// in memory.
fn try_for_each_json_line_with_limit(
    home: &Path,
    path: &Path,
    max_line_bytes: u64,
    mut visit: impl FnMut(&Value) -> Result<bool>,
) -> Result<usize> {
    let file = open_session_transcript(home, path)?;
    let mut reader = io::BufReader::new(file);
    let mut line = Vec::new();
    let mut line_number = 0_u64;
    let mut malformed_lines = 0;
    loop {
        line.clear();
        let read = (&mut reader)
            // A JSONL record may be followed by either LF or CRLF. Read room
            // for both delimiters so the byte limit applies to the JSON
            // record itself rather than rejecting an exact-size record just
            // because it has a conventional trailing newline.
            .take(max_line_bytes.saturating_add(2))
            .read_until(b'\n', &mut line);
        match read {
            Ok(0) => return Ok(malformed_lines),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("read session transcript {}", safe_path(path)));
            }
            Ok(_) => {
                let record = line.strip_suffix(b"\n").unwrap_or(&line);
                let record = record.strip_suffix(b"\r").unwrap_or(record);
                if record.len() as u64 > max_line_bytes {
                    bail!(
                        "session transcript line {} exceeds the {} byte limit: {}",
                        line_number + 1,
                        max_line_bytes,
                        safe_path(path)
                    );
                }
                line_number += 1;
            }
        }
        match serde_json::from_slice::<Value>(&line) {
            Ok(value) if !visit(&value)? => return Ok(malformed_lines),
            Ok(_) => {}
            Err(_) => malformed_lines += 1,
        }
    }
}

#[cfg(test)]
fn test_transcript_home(path: &Path, components: &[&str]) -> Result<PathBuf> {
    if components.is_empty() {
        return path
            .parent()
            .map(Path::to_path_buf)
            .with_context(|| format!("session transcript has no parent: {}", path.display()));
    }

    let session_suffix: PathBuf = components.iter().collect();
    let Some(session_dir) = path.parent().and_then(|parent| {
        parent
            .ancestors()
            .find(|dir| dir.ends_with(&session_suffix))
    }) else {
        bail!(
            "session transcript is outside the expected session tree: {}",
            path.display()
        );
    };
    let mut home = session_dir.to_path_buf();
    for _ in components {
        if !home.pop() {
            bail!(
                "session transcript has no tenant home ancestor: {}",
                path.display()
            );
        }
    }
    Ok(home)
}

#[cfg(unix)]
fn open_session_transcript(home: &Path, path: &Path) -> Result<fs::File> {
    let (parent, file_name) = open_session_parent(home, path)?;
    open_session_transcript_at(&parent, &file_name, path)
}

#[cfg(unix)]
fn open_session_transcript_at(
    parent: &std::os::fd::OwnedFd,
    file_name: &std::ffi::OsStr,
    path: &Path,
) -> Result<fs::File> {
    use std::os::fd::{AsRawFd, FromRawFd};

    let file_name = os_str_c_string(file_name)?;
    // SAFETY: `parent` owns a valid directory descriptor and `file_name` is a
    // live NUL-terminated string. `openat` retains neither pointer, and its
    // return value is checked before it is treated as an owned descriptor.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            file_name.as_ptr(),
            // A transcript can be replaced after discovery. O_NONBLOCK is a
            // no-op for regular files but prevents a FIFO replacement from
            // hanging this host-side read before the descriptor type check.
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ELOOP) {
            bail!(
                "session transcript is not a regular file: {}",
                safe_path(path)
            );
        }
        return Err(error).with_context(|| format!("open session transcript {}", safe_path(path)));
    }
    // SAFETY: `fd` is nonnegative and newly returned by `openat`; ownership is
    // transferred exactly once to `File`, which will close it on drop.
    let file = unsafe { fs::File::from_raw_fd(fd) };
    let metadata = file
        .metadata()
        .with_context(|| format!("inspect session transcript {}", safe_path(path)))?;
    if !metadata.file_type().is_file() {
        bail!(
            "session transcript is not a regular file: {}",
            safe_path(path)
        );
    }
    Ok(file)
}

#[cfg(not(unix))]
fn open_session_transcript(home: &Path, path: &Path) -> Result<fs::File> {
    validate_session_ancestors(home, path)?;
    crate::foundation::safe_fs::open_real_file(path, "session transcript")
}

#[cfg(unix)]
fn remove_session_transcript(home: &Path, path: &Path) -> Result<()> {
    use std::os::fd::AsRawFd;

    let (parent, file_name) = open_session_parent(home, path)?;
    // Require a regular final entry before unlinking it. `unlinkat` is anchored
    // to the already-open real parent directory, so an agent cannot redirect
    // this deletion by swapping an ancestor for a symlink after discovery.
    drop(open_session_transcript_at(&parent, &file_name, path)?);
    let file_name = os_str_c_string(&file_name)?;
    // SAFETY: `parent` owns a valid directory descriptor and `file_name`
    // remains a live NUL-terminated string for the duration of the call.
    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), file_name.as_ptr(), 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error()).with_context(|| format!("delete {}", safe_path(path)))
    }
}

#[cfg(not(unix))]
fn remove_session_transcript(home: &Path, path: &Path) -> Result<()> {
    validate_session_ancestors(home, path)?;
    crate::foundation::safe_fs::open_real_file(path, "session transcript")?;
    fs::remove_file(path).with_context(|| format!("delete {}", safe_path(path)))
}

#[cfg(unix)]
fn open_session_parent(home: &Path, path: &Path) -> Result<(std::os::fd::OwnedFd, OsString)> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;

    let relative = path.strip_prefix(home).with_context(|| {
        format!(
            "session transcript {} is outside tenant home {}",
            safe_path(path),
            safe_path(home)
        )
    })?;
    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => components.push(name.to_os_string()),
            _ => bail!(
                "session transcript path is not a normalized child of the tenant home: {}",
                safe_path(path)
            ),
        }
    }
    let file_name = components
        .pop()
        .with_context(|| format!("session transcript path is empty: {}", safe_path(path)))?;

    let home_path = std::ffi::CString::new(home.as_os_str().as_bytes())
        .with_context(|| format!("tenant home contains a NUL byte: {}", safe_path(home)))?;
    // SAFETY: `home_path` is NUL-terminated and live for the call. `open`
    // retains no pointer, and its return value is checked before ownership.
    let home_fd = unsafe {
        libc::open(
            home_path.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    if home_fd < 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("open tenant home {}", safe_path(home)));
    }
    // SAFETY: `home_fd` is nonnegative and newly returned by `open`; ownership
    // is transferred exactly once to `OwnedFd`.
    let mut parent = unsafe { OwnedFd::from_raw_fd(home_fd) };

    for component in components {
        let component_c = os_str_c_string(&component)?;
        // SAFETY: `parent` owns a valid directory descriptor and `component_c`
        // is NUL-terminated and live for the call. The result is checked before
        // it is treated as an owned descriptor.
        let next_fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                component_c.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            )
        };
        if next_fd < 0 {
            return Err(io::Error::last_os_error())
                .with_context(|| format!("open session path {}", safe_path(path)));
        }
        // SAFETY: `next_fd` is nonnegative and newly returned by `openat`;
        // ownership is transferred exactly once, replacing and closing the
        // previous parent descriptor.
        parent = unsafe { OwnedFd::from_raw_fd(next_fd) };
    }

    Ok((parent, file_name))
}

#[cfg(unix)]
fn os_str_c_string(value: &std::ffi::OsStr) -> Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;

    std::ffi::CString::new(value.as_bytes()).context("session path contains a NUL byte")
}

#[cfg(not(unix))]
fn validate_session_ancestors(home: &Path, path: &Path) -> Result<()> {
    let relative = path.strip_prefix(home).with_context(|| {
        format!(
            "session transcript {} is outside tenant home {}",
            safe_path(path),
            safe_path(home)
        )
    })?;
    let mut current = home.to_path_buf();
    crate::foundation::safe_fs::real_dir_exists(&current, "tenant home")?;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            bail!(
                "session transcript path is not a normalized child of the tenant home: {}",
                safe_path(path)
            );
        };
        current.push(name);
        if components.peek().is_some() {
            crate::foundation::safe_fs::real_dir_exists(&current, "session directory")?;
        }
    }
    Ok(())
}

/// A line's top-level timestamp, shared by both transcript formats, or empty.
pub(crate) fn ts_of(value: &Value) -> String {
    value
        .get("timestamp")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Classification of one parsed transcript record for list titles and parser tests.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PromptRecord {
    /// A prompt that the user actually typed.
    Typed(String),
    /// A readable user record whose content also contains unsupported parts.
    TypedWithUnsupported(String),
    /// A recognized non-prompt record, including injected and tool records.
    NotTyped,
    /// A user-like record whose shape is unsupported or malformed.
    UnsupportedUserLike,
}

impl PromptRecord {
    pub(crate) fn from_text_parts(parts: &[String], unsupported: bool) -> Self {
        if parts.is_empty() {
            if unsupported {
                Self::UnsupportedUserLike
            } else {
                Self::NotTyped
            }
        } else {
            let text = parts.join("\n");
            if unsupported {
                Self::TypedWithUnsupported(text)
            } else {
                Self::Typed(text)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TranscriptDiagnostics {
    malformed_lines: usize,
    unsupported_user_records: usize,
}

impl TranscriptDiagnostics {
    fn observe_prompt_record(&mut self, record: PromptRecord) -> Option<String> {
        match record {
            PromptRecord::Typed(text) => Some(text),
            PromptRecord::TypedWithUnsupported(text) => {
                self.unsupported_user_records += 1;
                Some(text)
            }
            PromptRecord::UnsupportedUserLike => {
                self.unsupported_user_records += 1;
                None
            }
            PromptRecord::NotTyped => None,
        }
    }
}

/// One Session's list-row data.
///
/// Every Transcript yields a summary, so Sessions with no readable message remain
/// visible and deletable.
pub(crate) struct SessionSummary {
    /// Full session id (the row shows the final UUID group for canonical UUIDs).
    pub id: String,
    /// Session start timestamp (ISO-8601), or empty if none was found.
    pub start_ts: String,
    /// The agent-generated title when available, otherwise the first readable
    /// user message, or empty for a tool/injected-only session.
    pub title: String,
    pub latest_message: String,
    pub message_count: usize,
    pub tool_count: usize,
    pub native_facts: SessionNativeFacts,
    diagnostics: TranscriptDiagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub(crate) enum ConversationRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ConversationMessage {
    pub(crate) entry_ids: Vec<String>,
    pub(crate) role: ConversationRole,
    pub(crate) timestamp: String,
    pub(crate) text: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub(crate) enum ToolActivityStatus {
    Started,
    Completed,
    Failed,
    Incomplete,
    Unknown,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ToolActivity {
    pub(crate) entry_ids: Vec<String>,
    pub(crate) call_id: Option<String>,
    pub(crate) timestamp: String,
    pub(crate) name: String,
    pub(crate) status: ToolActivityStatus,
    pub(crate) summary: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TranscriptEvidenceSummary {
    pub(crate) entry_id: String,
    pub(crate) line: u64,
    pub(crate) timestamp: String,
    pub(crate) native_type: String,
    pub(crate) role: Option<String>,
    pub(crate) content_types: Vec<String>,
    pub(crate) status: String,
    pub(crate) preview: String,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct SessionDetailStats {
    pub(crate) start_ts: String,
    pub(crate) last_event_ts: String,
    pub(crate) observed_duration_ms: Option<i64>,
    pub(crate) message_count: usize,
    pub(crate) tool_count: usize,
    pub(crate) entry_count: usize,
    pub(crate) malformed_count: usize,
    pub(crate) unsupported_count: usize,
    pub(crate) hidden_internal_count: usize,
    pub(crate) file_size: u64,
    pub(crate) snapshot: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct SessionDetailMeta {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) start_ts: String,
    pub(crate) transcript_path: String,
    pub(crate) cwd: Option<String>,
    pub(crate) model_provider: Option<String>,
    pub(crate) cli_version: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SessionNativeFacts {
    pub(crate) cwd: Option<String>,
    pub(crate) model_provider: Option<String>,
    pub(crate) cli_version: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum DetailRecord {
    Message(ConversationMessage),
    Tool(ToolActivity),
    Evidence(TranscriptEvidenceSummary),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) enum EvidenceEncoding {
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "base64")]
    Base64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct TranscriptEvidence {
    pub(crate) entry_id: String,
    pub(crate) encoding: EvidenceEncoding,
    pub(crate) content: String,
    pub(crate) snapshot: String,
}

pub(crate) fn bounded_preview(value: &str) -> String {
    const MAX: usize = 240;
    let safe = terminal_safe(value);
    safe.chars().take(MAX).collect()
}

fn content_types(value: &Value) -> Vec<String> {
    value
        .pointer("/message/content")
        .or_else(|| value.pointer("/payload/content"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("type").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn role_of(value: &Value) -> Option<String> {
    value
        .pointer("/message/role")
        .or_else(|| value.pointer("/payload/role"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn text_at(value: &Value, pointer: &str) -> Option<String> {
    match value.pointer(pointer) {
        Some(Value::String(text)) if !text.is_empty() => Some(text.clone()),
        Some(Value::Array(items)) => {
            let parts = items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}

pub(crate) fn evidence_for(
    value: &Value,
    entry_id: &str,
    line: u64,
    status: &str,
) -> TranscriptEvidenceSummary {
    TranscriptEvidenceSummary {
        entry_id: entry_id.to_string(),
        line,
        timestamp: ts_of(value),
        native_type: value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        role: role_of(value),
        content_types: content_types(value),
        status: status.to_string(),
        preview: text_at(value, "/message/content")
            .or_else(|| text_at(value, "/payload/content"))
            .or_else(|| {
                value
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .map(|text| bounded_preview(&text))
            .unwrap_or_default(),
    }
}

fn snapshot_for_metadata(metadata: &fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    format!("{}:{modified}", metadata.len())
}

fn observed_duration_ms(start: &str, end: &str) -> Option<i64> {
    let start = OffsetDateTime::parse(start, &Rfc3339).ok()?;
    let end = OffsetDateTime::parse(end, &Rfc3339).ok()?;
    let duration = end - start;
    (duration.is_positive() || duration.is_zero()).then(|| duration.whole_milliseconds() as i64)
}

fn detail_entry_id(line: u64) -> String {
    format!("line-{line}")
}

/// Test-only compatibility projection for backend parser tests.
#[cfg(test)]
#[derive(Clone, Debug, serde::Serialize)]
pub(crate) struct Prompt {
    /// The turn's timestamp (ISO-8601), or empty.
    pub timestamp: String,
    /// The full prompt text (all supported text content joined; injected
    /// wrappers already filtered by the backend).
    pub text: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct SessionListRow {
    pub(crate) id: String,
    pub(crate) display_id: String,
    pub(crate) start_ts: String,
    pub(crate) title: String,
    pub(crate) latest_message: String,
    pub(crate) message_count: usize,
    pub(crate) tool_count: usize,
    pub(crate) warnings: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct SessionListData {
    pub(crate) sessions: Vec<SessionListRow>,
    pub(crate) warnings: Vec<String>,
    pub(crate) partial: bool,
}

/// A Coding Agent's on-disk Transcript format. The two implementations
/// (`session_claude::Claude`, `session_codex::Codex`) diverge only in the
/// required methods below — *where* the transcript tree lives, which file
/// names count, *where* each field lives on a line, and which lines count as
/// a real prompt. The discovery walks and the summary/get loops that consume
/// those answers ([`files`](Self::files) / [`list_files`](Self::list_files) /
/// [`summarize_in`](Self::summarize_in) /
/// [`for_each_prompt_in`](Self::for_each_prompt_in)) are written once here as
/// provided methods, so the two backends can't drift out of sync.
pub(crate) trait SessionBackend {
    /// Path components of the transcript tree beneath the tenant home
    /// (e.g. `[".claude", "projects"]`), resolved only through real directory
    /// entries so agent-created symlinks are never followed.
    fn session_dir_components(&self) -> &'static [&'static str];

    /// Whether a `.jsonl` file name is a transcript. Claude keeps all; Codex
    /// keeps only `rollout-` names. Shared by [`files`](Self::files) and
    /// [`list_files`](Self::list_files), so `list` can never show a row that
    /// `get`/`delete` then refuse to resolve.
    fn keep_transcript_name(&self, name: &str) -> bool;

    /// All transcript files under this tenant home (empty if none yet). The
    /// strict walk: `get`/`delete` use it, and a destructive or single-target
    /// action must not act on a partial view of the tree.
    fn files(&self, home: &Path) -> Result<Vec<PathBuf>> {
        let Some(base) = checked_session_dir(home, self.session_dir_components())? else {
            return Ok(Vec::new());
        };
        walk_jsonl(&base, |name| self.keep_transcript_name(name))
    }

    /// Transcript files for Console Session listing: the tolerant walk, so one bad
    /// child path does not hide every readable session.
    fn list_files(&self, home: &Path) -> Result<SessionDiscovery> {
        let Some(base) = checked_session_dir(home, self.session_dir_components())? else {
            return Ok(SessionDiscovery::default());
        };
        walk_jsonl_tolerant(&base, |name| self.keep_transcript_name(name))
    }

    /// The session id for a transcript path.
    fn id_of(&self, path: &Path) -> String;

    /// Classify one line for the list title and compatibility parser, filtering injected/wrapper
    /// turns while distinguishing recognized non-prompts from unsupported
    /// user-like records. This is the heart of the divergence: Claude keys off
    /// `promptSource:typed`, Codex off a wrapper-filtered `response_item` user
    /// message.
    fn prompt_record(&self, value: &Value) -> PromptRecord;

    /// Project one native Transcript Entry into the Console's shared detail
    /// vocabulary. Coding Agent formats intentionally keep this mapping local.
    fn detail_records(&self, value: &Value, entry_id: &str, line: u64) -> Vec<DetailRecord> {
        vec![DetailRecord::Evidence(evidence_for(
            value,
            entry_id,
            line,
            "unsupported",
        ))]
    }

    fn native_facts(&self, _value: &Value, _facts: &mut SessionNativeFacts) {}

    /// The session start timestamp from one parsed line; the first `Some` is
    /// retained. Claude answers for any line bearing a non-empty top-level
    /// `timestamp`; Codex answers for a `session_meta` timestamp.
    fn start_ts_of(&self, value: &Value) -> Option<String>;

    /// Lower-confidence timestamp candidate used only when
    /// [`start_ts_of`](Self::start_ts_of) never finds one.
    fn fallback_start_ts_of(&self, _value: &Value) -> Option<String> {
        None
    }

    /// A `list` row title candidate from one parsed line. The *last* non-empty
    /// candidate wins; a session with none falls back to its first readable
    /// user message. Default: no candidates (Codex has no ai-title); Claude overrides
    /// to surface `ai-title` lines.
    fn title_of(&self, _value: &Value) -> Option<String> {
        None
    }

    /// Summarize one transcript for `list`. Every transcript summarizes — a
    /// session with no readable message just gets an empty title (unless a backend's
    /// `title_of` finds something else, like Claude's `ai-title`), so tool/
    /// injected-only shells still list and can be cleared. One streaming pass
    /// with O(1) state; the Coding Agent-specific answers come from the methods
    /// above.
    /// `home` anchors no-follow traversal of every path component.
    fn summarize_in(&self, home: &Path, path: &Path) -> Result<SessionSummary> {
        let mut start_ts: Option<String> = None;
        let mut fallback_start_ts: Option<String> = None;
        let mut first_typed: Option<String> = None;
        let mut title: Option<String> = None;
        let mut latest_message = String::new();
        let mut message_count = 0;
        let mut tool_count = 0;
        let mut native_facts = SessionNativeFacts::default();
        let mut diagnostics = TranscriptDiagnostics::default();
        diagnostics.malformed_lines = try_for_each_json_line(home, path, |value| {
            if start_ts.is_none() {
                start_ts = self.start_ts_of(value);
            }
            if fallback_start_ts.is_none() {
                fallback_start_ts = self.fallback_start_ts_of(value);
            }
            let typed = diagnostics.observe_prompt_record(self.prompt_record(value));
            if first_typed.is_none() {
                first_typed = typed;
            }
            if let Some(candidate) = self.title_of(value)
                && !candidate.is_empty()
            {
                title = Some(candidate);
            }
            self.native_facts(value, &mut native_facts);
            for record in self.detail_records(value, "summary", 0) {
                match record {
                    DetailRecord::Message(message) => {
                        latest_message = message.text;
                        message_count += 1;
                    }
                    DetailRecord::Tool(tool) if tool.status == ToolActivityStatus::Started => {
                        tool_count += 1;
                    }
                    DetailRecord::Tool(_) | DetailRecord::Evidence(_) => {}
                }
            }
            Ok(true)
        })?;
        Ok(SessionSummary {
            id: self.id_of(path),
            start_ts: start_ts.or(fallback_start_ts).unwrap_or_default(),
            title: title.or(first_typed).unwrap_or_default(),
            latest_message,
            message_count,
            tool_count,
            native_facts,
            diagnostics,
        })
    }

    /// Test helper that derives the fixture home from the backend's tree.
    #[cfg(test)]
    fn summarize(&self, path: &Path) -> Result<SessionSummary>
    where
        Self: Sized,
    {
        let home = test_transcript_home(path, self.session_dir_components())?;
        self.summarize_in(&home, path)
    }

    /// Collect typed user records for parser tests.
    #[cfg(test)]
    fn prompts_in(&self, home: &Path, path: &Path) -> Result<Vec<Prompt>> {
        let mut out = Vec::new();
        self.for_each_prompt_in(home, path, &mut |prompt| {
            out.push(prompt);
            Ok(true)
        })?;
        Ok(out)
    }

    /// Visit typed user records for parser tests.
    #[cfg(test)]
    fn for_each_prompt_in(
        &self,
        home: &Path,
        path: &Path,
        visit: &mut dyn FnMut(Prompt) -> Result<bool>,
    ) -> Result<(usize, TranscriptDiagnostics)> {
        let mut count = 0;
        let mut diagnostics = TranscriptDiagnostics::default();
        diagnostics.malformed_lines = try_for_each_json_line(home, path, |value| {
            if let Some(text) = diagnostics.observe_prompt_record(self.prompt_record(value)) {
                count += 1;
                return visit(Prompt {
                    timestamp: ts_of(value),
                    text,
                });
            }
            Ok(true)
        })?;
        Ok((count, diagnostics))
    }

    /// Test helper that derives the fixture home from the backend's tree.
    #[cfg(test)]
    fn prompts(&self, path: &Path) -> Result<Vec<Prompt>>
    where
        Self: Sized,
    {
        let home = test_transcript_home(path, self.session_dir_components())?;
        self.prompts_in(&home, path)
    }
}

/// Resolve `AgentKind` to its backend. The one bridge between the enum and the
/// session trait objects.
pub(crate) fn backend_for(agent: AgentKind) -> Box<dyn SessionBackend> {
    match agent {
        AgentKind::Claude => Box::new(crate::session::claude::Claude),
        AgentKind::Codex => Box::new(crate::session::codex::Codex),
    }
}

/// Resolve a full id or unique suffix to exactly one transcript path. A single
/// exact id wins even when it is a suffix of other ids (otherwise that session
/// could never be addressed at all), but duplicate exact ids remain ambiguous
/// rather than selecting whichever directory the filesystem happened to visit
/// first. Zero matches or ambiguous candidates fail with a message.
fn resolve(backend: &dyn SessionBackend, home: &Path, query: &str) -> Result<PathBuf> {
    resolve_in(backend, &backend.files(home)?, query)
}

/// Resolve `query` against an already-discovered file list, so callers with many
/// ids (`delete a b c`) can walk the transcript tree once instead of per id.
fn resolve_in(backend: &dyn SessionBackend, files: &[PathBuf], query: &str) -> Result<PathBuf> {
    if query.is_empty() {
        bail!("need a session id (or unique suffix)");
    }
    let mut exact_matches: Vec<PathBuf> = Vec::new();
    let mut suffix_matches: Vec<PathBuf> = Vec::new();
    for file in files {
        let id = backend.id_of(file);
        if id == query {
            exact_matches.push(file.clone());
        } else if id.ends_with(query) {
            suffix_matches.push(file.clone());
        }
    }
    let candidates = if exact_matches.is_empty() {
        suffix_matches
    } else {
        exact_matches
    };
    match candidates.len() {
        0 => bail!("no session matches: {}", terminal_safe(query)),
        1 => Ok(candidates.into_iter().next().unwrap()),
        n => {
            let mut message = format!(
                "ambiguous id '{}' matches {n} sessions:",
                terminal_safe(query)
            );
            for candidate in &candidates {
                write!(
                    &mut message,
                    "\n     {}  {}",
                    terminal_safe(&backend.id_of(candidate)),
                    safe_path(candidate)
                )
                .expect("writing to a String cannot fail");
            }
            bail!(message)
        }
    }
}

const LIST_TITLE_MAX_CHARS: usize = 64;

pub(crate) fn is_canonical_uuid(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == UUID_TEXT_LEN
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn list_id(id: &str) -> String {
    if is_canonical_uuid(id) {
        id[id.len() - UUID_SUFFIX_LEN..].to_string()
    } else {
        terminal_safe(id)
    }
}

pub(crate) fn list_data(backend: &dyn SessionBackend, home: &Path) -> Result<SessionListData> {
    let discovery = backend.list_files(home)?;
    let mut warnings = discovery
        .errors
        .into_iter()
        .map(|error| terminal_safe(&error))
        .collect::<Vec<_>>();
    let mut sessions = Vec::new();
    for file in discovery.files {
        match backend.summarize_in(home, &file) {
            Ok(summary) => {
                let mut row_warnings = Vec::new();
                if summary.diagnostics.malformed_lines != 0 {
                    row_warnings.push(format!(
                        "skipped {} malformed JSONL record(s)",
                        summary.diagnostics.malformed_lines
                    ));
                }
                if summary.diagnostics.unsupported_user_records != 0 {
                    row_warnings.push(format!(
                        "skipped {} malformed or unsupported user-like record(s)",
                        summary.diagnostics.unsupported_user_records
                    ));
                }
                sessions.push(SessionListRow {
                    display_id: list_id(&summary.id),
                    id: summary.id,
                    start_ts: summary.start_ts,
                    title: list_title(&summary.title),
                    latest_message: list_title(&summary.latest_message),
                    message_count: summary.message_count,
                    tool_count: summary.tool_count,
                    warnings: row_warnings,
                });
            }
            Err(error) => warnings.push(format!("{}: {error:#}", safe_path(&file))),
        }
    }
    sessions.sort_by(|left, right| right.start_ts.cmp(&left.start_ts));
    let partial = !warnings.is_empty() || sessions.iter().any(|row| !row.warnings.is_empty());
    Ok(SessionListData {
        sessions,
        warnings,
        partial,
    })
}

#[cfg(test)]
pub(crate) fn detail_records_for_test(
    backend: &dyn SessionBackend,
    home: &Path,
    query: &str,
) -> Result<Vec<DetailRecord>> {
    let mut records = Vec::new();
    stream_detail_data(backend, home, query, &mut |_| Ok(true), &mut |record| {
        records.push(record);
        Ok(true)
    })?;
    Ok(records)
}

/// Count safely discoverable Transcripts without opening or parsing them.
pub(crate) fn discovery_summary(
    backend: &dyn SessionBackend,
    home: &Path,
) -> Result<SessionDiscoverySummary> {
    let discovery = backend.list_files(home)?;
    let warnings = discovery
        .errors
        .into_iter()
        .map(|error| terminal_safe(&error))
        .collect::<Vec<_>>();
    Ok(SessionDiscoverySummary {
        count: discovery.files.len(),
        partial: !warnings.is_empty(),
        warnings,
    })
}

fn detail_file_path(backend: &dyn SessionBackend, home: &Path, query: &str) -> Result<PathBuf> {
    resolve(backend, home, query)
}

fn detail_meta(
    backend: &dyn SessionBackend,
    home: &Path,
    path: &Path,
    id: &str,
) -> Result<SessionDetailMeta> {
    let summary = backend.summarize_in(home, path)?;
    Ok(SessionDetailMeta {
        id: id.to_string(),
        title: summary.title,
        start_ts: summary.start_ts,
        transcript_path: path
            .strip_prefix(home)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string(),
        cwd: summary.native_facts.cwd,
        model_provider: summary.native_facts.model_provider,
        cli_version: summary.native_facts.cli_version,
    })
}

pub(crate) fn stream_detail_data(
    backend: &dyn SessionBackend,
    home: &Path,
    query: &str,
    begin: &mut dyn FnMut(&SessionDetailMeta) -> Result<bool>,
    visit: &mut dyn FnMut(DetailRecord) -> Result<bool>,
) -> Result<(SessionDetailMeta, SessionDetailStats, Vec<String>)> {
    let path = detail_file_path(backend, home, query)?;
    let id = backend.id_of(&path);
    let meta = detail_meta(backend, home, &path, &id)?;
    if !begin(&meta)? {
        return Ok((meta, SessionDetailStats::default(), Vec::new()));
    }
    let file = open_session_transcript(home, &path)?;
    let metadata = file
        .metadata()
        .with_context(|| format!("inspect session transcript {}", safe_path(&path)))?;
    let snapshot = snapshot_for_metadata(&metadata);
    let file_size = metadata.len();
    let mut stats = SessionDetailStats {
        start_ts: meta.start_ts.clone(),
        file_size,
        snapshot,
        ..SessionDetailStats::default()
    };
    let mut warnings = Vec::new();
    let mut reader = io::BufReader::new(file);
    let mut line = Vec::new();
    let mut line_number = 0_u64;
    let mut pending_tools = HashMap::<String, ToolActivity>::new();
    loop {
        line.clear();
        let read = (&mut reader)
            .take(MAX_TRANSCRIPT_LINE_BYTES.saturating_add(2))
            .read_until(b'\n', &mut line)
            .with_context(|| format!("read session transcript {}", safe_path(&path)))?;
        if read == 0 {
            break;
        }
        line_number += 1;
        let record_id = detail_entry_id(line_number);
        let record = line.strip_suffix(b"\n").unwrap_or(&line);
        let record = record.strip_suffix(b"\r").unwrap_or(record);
        if record.len() as u64 > MAX_TRANSCRIPT_LINE_BYTES {
            bail!(
                "session transcript line {} exceeds the {} byte limit: {}",
                line_number,
                MAX_TRANSCRIPT_LINE_BYTES,
                safe_path(&path)
            );
        }
        stats.entry_count += 1;
        let value = match serde_json::from_slice::<Value>(record) {
            Ok(value) => value,
            Err(error) => {
                stats.malformed_count += 1;
                warnings.push(format!("line {line_number}: malformed JSONL ({error})"));
                if !visit(DetailRecord::Evidence(TranscriptEvidenceSummary {
                    entry_id: record_id,
                    line: line_number,
                    timestamp: String::new(),
                    native_type: "malformed".to_string(),
                    role: None,
                    content_types: Vec::new(),
                    status: "malformed".to_string(),
                    preview: bounded_preview(&String::from_utf8_lossy(record)),
                }))? {
                    return Ok((meta, stats, warnings));
                }
                continue;
            }
        };
        let timestamp = ts_of(&value);
        if !timestamp.is_empty() {
            stats.last_event_ts = timestamp;
        }
        for projected in backend.detail_records(&value, &record_id, line_number) {
            if let DetailRecord::Tool(tool) = &projected
                && let Some(call_id) = &tool.call_id
            {
                if tool.status == ToolActivityStatus::Started {
                    pending_tools.insert(call_id.clone(), tool.clone());
                } else {
                    pending_tools.remove(call_id);
                }
            }
            match &projected {
                DetailRecord::Message(_) => stats.message_count += 1,
                DetailRecord::Tool(tool) if tool.status == ToolActivityStatus::Started => {
                    stats.tool_count += 1;
                }
                DetailRecord::Tool(_) => {}
                DetailRecord::Evidence(evidence) => {
                    if evidence.status == "hidden_internal" {
                        stats.hidden_internal_count += 1;
                    } else if evidence.status == "unsupported" {
                        stats.unsupported_count += 1;
                    }
                }
            }
            if !visit(projected)? {
                return Ok((meta, stats, warnings));
            }
        }
    }
    for mut tool in pending_tools.into_values() {
        tool.status = ToolActivityStatus::Incomplete;
        if !visit(DetailRecord::Tool(tool))? {
            return Ok((meta, stats, warnings));
        }
    }
    if stats.unsupported_count != 0 {
        warnings.push(format!(
            "encountered {} unsupported Transcript Entry projection(s)",
            stats.unsupported_count
        ));
    }
    stats.observed_duration_ms = observed_duration_ms(&stats.start_ts, &stats.last_event_ts);
    Ok((meta, stats, warnings))
}

pub(crate) fn read_evidence(
    backend: &dyn SessionBackend,
    home: &Path,
    query: &str,
    entry: &str,
    snapshot: &str,
) -> Result<TranscriptEvidence> {
    let path = detail_file_path(backend, home, query)?;
    let file = open_session_transcript(home, &path)?;
    let current_snapshot = snapshot_for_metadata(
        &file
            .metadata()
            .with_context(|| format!("inspect session transcript {}", safe_path(&path)))?,
    );
    if current_snapshot != snapshot {
        return Err(crate::application_error::application_error(
            crate::application_error::ApplicationErrorKind::Conflict,
            "Session Transcript changed since it was inspected; refresh the detail view",
        ));
    }
    let line_number = entry
        .strip_prefix("line-")
        .and_then(|value| value.parse::<u64>().ok())
        .with_context(|| format!("invalid Transcript Entry id: {entry}"))?;
    let mut reader = io::BufReader::new(file);
    let mut raw = Vec::new();
    let mut current_line = 0_u64;
    loop {
        raw.clear();
        let read = (&mut reader)
            .take(MAX_TRANSCRIPT_LINE_BYTES.saturating_add(2))
            .read_until(b'\n', &mut raw)
            .with_context(|| format!("read Transcript Entry {entry}"))?;
        if read == 0 {
            break;
        }
        current_line += 1;
        if current_line != line_number {
            continue;
        }
        let record = raw.strip_suffix(b"\n").unwrap_or(&raw);
        let record = record.strip_suffix(b"\r").unwrap_or(record);
        if record.len() as u64 > MAX_TRANSCRIPT_LINE_BYTES {
            bail!(
                "Transcript Entry exceeds the {} byte limit",
                MAX_TRANSCRIPT_LINE_BYTES
            );
        }
        let (encoding, content) = match std::str::from_utf8(record) {
            Ok(value) => (EvidenceEncoding::Utf8, value.to_string()),
            Err(_) => (
                EvidenceEncoding::Base64,
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, record),
            ),
        };
        if let Ok(value) = serde_json::from_slice::<Value>(record)
            && backend
                .detail_records(&value, entry, line_number)
                .iter()
                .any(|item| {
                    matches!(item, DetailRecord::Evidence(evidence) if evidence.status == "hidden_internal")
                })
        {
            bail!("internal reasoning is not available as Transcript evidence");
        }
        return Ok(TranscriptEvidence {
            entry_id: entry.to_string(),
            encoding,
            content,
            snapshot: current_snapshot,
        });
    }
    Err(crate::application_error::application_error(
        crate::application_error::ApplicationErrorKind::NotFound,
        format!("Transcript Entry does not exist: {entry}"),
    ))
}

pub(crate) fn delete_sessions(
    backend: &dyn SessionBackend,
    home: &Path,
    ids: &[String],
    all: bool,
) -> Result<usize> {
    let targets = delete_targets(backend, home, ids, all)?;
    let count = targets.len();
    for path in targets {
        remove_session_transcript(home, &path)?;
    }
    Ok(count)
}

fn delete_targets(
    backend: &dyn SessionBackend,
    home: &Path,
    ids: &[String],
    all: bool,
) -> Result<Vec<PathBuf>> {
    if all && !ids.is_empty() {
        bail!("--all cannot be combined with session ids");
    }
    if !all && ids.is_empty() {
        bail!("provide at least one session id or use --all");
    }

    if all {
        // Match `list`: bulk deletion includes tool/injected-only transcripts
        // that have no readable message.
        let mut targets = backend.files(home)?;
        targets.sort_by_key(|path| backend.id_of(path));
        return Ok(targets);
    }

    // Resolve every id against one strict snapshot so one command cannot act on
    // different views of a changing transcript tree.
    let files = backend.files(home)?;
    let mut targets = Vec::new();
    for id in ids {
        let path = resolve_in(backend, &files, id)?;
        if !targets.contains(&path) {
            targets.push(path);
        }
    }
    Ok(targets)
}

/// Collapse runs of control characters and non-plain-space whitespace to a
/// single space (titles are one-liners in the listing). Keep ordinary spaces as
/// authored so readable prompt snippets do not get over-normalized.
fn collapse_ws(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_run = false;
    for character in text.chars() {
        if character.is_control() || (character.is_whitespace() && character != ' ') {
            if !in_run {
                out.push(' ');
                in_run = true;
            }
        } else {
            out.push(character);
            in_run = false;
        }
    }
    out
}

fn list_title(text: &str) -> String {
    collapse_ws(text)
        .chars()
        .take(LIST_TITLE_MAX_CHARS)
        .collect()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
