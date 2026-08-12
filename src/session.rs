//! Browse saved Sessions directly from a Tenant Home without starting a
//! container. Discovery, id resolution, listing, and deletion are shared;
//! [`SessionBackend`] isolates the two Coding Agents' Transcript formats.
//! Strict discovery protects `get` and `delete` from partial views, while
//! `list` can report traversal errors alongside readable Sessions.

use crate::agent::AgentKind;
use crate::cli::SessionCommand;
use anyhow::{Context, Result, bail};
use serde_json::Value;
#[cfg(unix)]
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};

const MAX_TRANSCRIPT_LINE_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const UUID_TEXT_LEN: usize = 36;
const UUID_SUFFIX_LEN: usize = 12;
const LIST_ID_MIN_WIDTH: usize = UUID_SUFFIX_LEN;

fn terminal_safe(value: &str) -> String {
    terminal_safe_with(value, |_| false)
}

fn terminal_safe_prompt(value: &str) -> String {
    terminal_safe_with(value, |character| matches!(character, '\n' | '\t'))
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
/// the Tenant Home. The Home is writable by the container, so following a
/// `.claude`/`.codex` ancestor symlink planted by a Coding Agent could make
/// host-side `session delete` remove Transcripts outside the Tenant.
pub(crate) fn checked_session_dir(home: &Path, components: &[&str]) -> Result<Option<PathBuf>> {
    let mut path = home.to_path_buf();
    if !crate::tenant::real_dir_exists(&path, "tenant home")? {
        return Ok(None);
    }
    for component in components {
        path.push(component);
        if !crate::tenant::real_dir_exists(&path, "session directory")? {
            return Ok(None);
        }
    }
    Ok(Some(path))
}

/// Transcript discovery for `session list`: usable files plus non-fatal walk
/// errors that should be reported without hiding every readable transcript.
#[derive(Default)]
pub(crate) struct SessionDiscovery {
    /// Transcript files that were discovered safely.
    pub files: Vec<PathBuf>,
    /// Non-fatal traversal failures to report alongside partial list results.
    pub errors: Vec<String>,
}

/// Whether a walked entry is a Transcript we want: a regular `.jsonl` file
/// whose name passes `keep`. Do not follow a Transcript-shaped symlink created
/// inside the mounted Tenant Home: host-side Session access must stay beneath
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

#[cfg(test)]
fn for_each_json_line_with_limit(
    home: &Path,
    path: &Path,
    max_line_bytes: u64,
    mut visit: impl FnMut(&Value),
) -> Result<()> {
    try_for_each_json_line_with_limit(home, path, max_line_bytes, |value| {
        visit(value);
        Ok(true)
    })
    .map(|_| ())
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
/// A Tenant's Transcripts can be hundreds of megabytes and `session list`
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
    crate::tenant::open_real_file(path, "session transcript")
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
    crate::tenant::open_real_file(path, "session transcript")?;
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
    crate::tenant::real_dir_exists(&current, "tenant home")?;
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
            crate::tenant::real_dir_exists(&current, "session directory")?;
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

/// Classification of one parsed transcript record for the typed-prompt view.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PromptRecord {
    /// A prompt that the user actually typed.
    Typed(String),
    /// A readable typed prompt whose record also contains unsupported content.
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

    fn has_warnings(self) -> bool {
        self.malformed_lines != 0 || self.unsupported_user_records != 0
    }
}

/// One Session's list-row data.
///
/// Every Transcript yields a summary, so Sessions with no typed prompt remain
/// visible and deletable.
pub(crate) struct SessionSummary {
    /// Full session id (the row shows the final UUID group for canonical UUIDs).
    pub id: String,
    /// Session start timestamp (ISO-8601), or empty if none was found.
    pub start_ts: String,
    /// The agent-generated title when available, otherwise the first typed
    /// prompt, or empty for a tool/injected-only session.
    pub title: String,
    diagnostics: TranscriptDiagnostics,
}

/// One typed prompt from a session, for `get`.
pub(crate) struct Prompt {
    /// The turn's timestamp (ISO-8601), or empty.
    pub timestamp: String,
    /// The full prompt text (all supported text content joined; injected
    /// wrappers already filtered by the backend).
    pub text: String,
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

    /// Transcript files for `session list`: the tolerant walk, so one bad
    /// child path does not hide every readable session.
    fn list_files(&self, home: &Path) -> Result<SessionDiscovery> {
        let Some(base) = checked_session_dir(home, self.session_dir_components())? else {
            return Ok(SessionDiscovery::default());
        };
        walk_jsonl_tolerant(&base, |name| self.keep_transcript_name(name))
    }

    /// The session id for a transcript path.
    fn id_of(&self, path: &Path) -> String;

    /// Classify one line for the typed-prompt view, filtering injected/wrapper
    /// turns while distinguishing recognized non-prompts from unsupported
    /// user-like records. This is the heart of the divergence: Claude keys off
    /// `promptSource:typed`, Codex off a wrapper-filtered `response_item` user
    /// message.
    fn prompt_record(&self, value: &Value) -> PromptRecord;

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
    /// candidate wins; a session with none falls back to its first typed
    /// prompt. Default: no candidates (Codex has no ai-title); Claude overrides
    /// to surface `ai-title` lines.
    fn title_of(&self, _value: &Value) -> Option<String> {
        None
    }

    /// Summarize one transcript for `list`. Every transcript summarizes — a
    /// session with no typed prompt just gets an empty title (unless a backend's
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
            Ok(true)
        })?;
        Ok(SessionSummary {
            id: self.id_of(path),
            start_ts: start_ts.or(fallback_start_ts).unwrap_or_default(),
            title: title.or(first_typed).unwrap_or_default(),
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

    /// Collect every typed prompt in one transcript, in order.
    ///
    /// Call [`Self::for_each_prompt_in`] when prompts should be processed with
    /// bounded memory, as the CLI `get` path does.
    #[cfg(test)]
    fn prompts_in(&self, home: &Path, path: &Path) -> Result<Vec<Prompt>> {
        let mut out = Vec::new();
        self.for_each_prompt_in(home, path, &mut |prompt| {
            out.push(prompt);
            Ok(true)
        })?;
        Ok(out)
    }

    /// Visit typed prompts in order without retaining the full transcript's
    /// prompt text. Returning `false` stops the read cleanly, which lets CLI
    /// output stop immediately after a downstream pipe closes.
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
        AgentKind::Claude => Box::new(crate::session_claude::Claude),
        AgentKind::Codex => Box::new(crate::session_codex::Codex),
    }
}

/// Dispatch a host-side session action.
///
/// The return value is the command exit code; `list` returns 1 when it can show
/// only a partial result.
pub(crate) fn dispatch(
    agent: AgentKind,
    home: &Path,
    command: Option<&SessionCommand>,
) -> Result<i32> {
    let backend = backend_for(agent);
    match command {
        None | Some(SessionCommand::List) => list(backend.as_ref(), home),
        Some(SessionCommand::Get { id }) => get(backend.as_ref(), home, id),
        Some(SessionCommand::Delete { ids, all, yes }) => {
            delete(backend.as_ref(), home, ids, *all, *yes)
        }
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

/// List this Tenant's Sessions, newest first: `shortid  date  title`.
///
/// Every Transcript lists (tool/injected-only shells show an empty title), so
/// nothing is hidden from `list` or `delete --all`. Columns are
/// `%-12s  %-16s  %s` for UUID ids; non-UUID ids are shown in full. Titles are
/// collapsed to one line and capped at 64 chars.
fn list(backend: &dyn SessionBackend, home: &Path) -> Result<i32> {
    list_with_printer(backend, home, crate::print_line)
}

fn list_with_printer(
    backend: &dyn SessionBackend,
    home: &Path,
    mut print: impl FnMut(&str) -> Result<bool>,
) -> Result<i32> {
    struct Row {
        start_ts: String,
        id: String,
        title: String,
    }

    let mut rows = Vec::new();
    let discovery = backend.list_files(home)?;
    let mut failed = !discovery.errors.is_empty();
    for error in discovery.errors {
        eprintln!("!! {}", terminal_safe(&error));
    }
    for file in discovery.files {
        match backend.summarize_in(home, &file) {
            Ok(summary) => {
                if report_transcript_diagnostics(&file, summary.diagnostics) {
                    failed = true;
                }
                let title = list_title(&summary.title);
                rows.push(Row {
                    start_ts: summary.start_ts,
                    id: summary.id,
                    title,
                });
            }
            Err(error) => {
                eprintln!(
                    "!! {}: {}",
                    safe_path(&file),
                    terminal_safe(&format!("{error:#}"))
                );
                failed = true;
            }
        }
    }
    if rows.is_empty() {
        if !failed {
            eprintln!(">> no sessions in this tenant");
        }
        return Ok(i32::from(failed));
    }
    // Newest first: ISO-8601 sorts lexically, so a plain string sort works.
    rows.sort_by(|a, b| b.start_ts.cmp(&a.start_ts));

    for Row {
        start_ts,
        id,
        title,
    } in rows
    {
        // Canonical UUIDs are safe ASCII; arbitrary ids come from transcript
        // file names inside the container-writable tenant home and are escaped.
        let short_id = list_id(&id);
        let timestamp = fmt_ts(&start_ts);
        if !print(&format!(
            "{short_id:<LIST_ID_MIN_WIDTH$}  {timestamp:<16}  {title}"
        ))? {
            break; // reader hung up; nothing left to show
        }
    }
    Ok(i32::from(failed))
}

/// Print your typed prompts from one session, numbered + timestamped, full text
/// (for copy-paste).
fn get(backend: &dyn SessionBackend, home: &Path, id: &str) -> Result<i32> {
    get_with_printer(backend, home, id, crate::print_line)
}

fn get_with_printer(
    backend: &dyn SessionBackend,
    home: &Path,
    id: &str,
    mut print: impl FnMut(&str) -> Result<bool>,
) -> Result<i32> {
    let path = resolve(backend, home, id)?;
    let sid = backend.id_of(&path);
    eprintln!(">> session {}", terminal_safe(&sid));
    let mut index = 0;
    let (prompt_count, diagnostics) = backend.for_each_prompt_in(home, &path, &mut |prompt| {
        index += 1;
        let timestamp = fmt_ts(&prompt.timestamp);
        let text = terminal_safe_prompt(&prompt.text);
        print(&format!("\n[{index}] {timestamp}\n{text}"))
    })?;
    if prompt_count == 0 {
        print("(no typed prompts in this session)")?;
    }
    Ok(i32::from(report_transcript_diagnostics(&path, diagnostics)))
}

fn report_transcript_diagnostics(path: &Path, diagnostics: TranscriptDiagnostics) -> bool {
    if diagnostics.malformed_lines != 0 {
        eprintln!(
            "!! {}: skipped {} malformed JSONL record(s)",
            safe_path(path),
            diagnostics.malformed_lines
        );
    }
    if diagnostics.unsupported_user_records != 0 {
        eprintln!(
            "!! {}: skipped {} malformed or unsupported user-like record(s)",
            safe_path(path),
            diagnostics.unsupported_user_records
        );
    }
    diagnostics.has_warnings()
}

/// Delete explicitly selected transcripts, asking once per target unless `yes`
/// is set. `--all` selects every transcript; an empty selection is an error.
fn delete(
    backend: &dyn SessionBackend,
    home: &Path,
    ids: &[String],
    all: bool,
    yes: bool,
) -> Result<i32> {
    let targets = delete_targets(backend, home, ids, all)?;
    if targets.is_empty() {
        eprintln!(">> no sessions in this tenant");
        return Ok(0);
    }

    let stdin = io::stdin();
    if !yes && !stdin.is_terminal() {
        bail!("refusing to delete sessions without --yes in a non-interactive shell");
    }
    let mut input = stdin.lock();
    delete_targets_with_input(backend, home, targets, yes, &mut input)
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
        // that have no typed prompt.
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

fn delete_targets_with_input(
    backend: &dyn SessionBackend,
    home: &Path,
    targets: Vec<PathBuf>,
    yes: bool,
    input: &mut dyn BufRead,
) -> Result<i32> {
    for path in targets {
        let sid = backend.id_of(&path);
        let delete = yes || confirm_delete(&sid, input)?;
        if delete {
            remove_session_transcript(home, &path)?;
            eprintln!(">> deleted {}", terminal_safe(&sid));
        } else {
            eprintln!(">> kept {}", terminal_safe(&sid));
        }
    }
    Ok(0)
}

fn confirm_delete(sid: &str, input: &mut dyn BufRead) -> Result<bool> {
    eprint!("delete session {}? [y/N] ", terminal_safe(sid));
    io::stderr()
        .flush()
        .context("flush session delete prompt")?;
    let mut answer = String::new();
    input
        .read_line(&mut answer)
        .context("read session delete confirmation")?;
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Format an ISO-8601 timestamp as `YYYY-MM-DD HH:MM` for display, or empty if
/// the timestamp is empty. Positional slicing, not real date parsing — the stored
/// value is already ISO-8601.
fn fmt_ts(ts: &str) -> String {
    if ts.is_empty() {
        return String::new();
    }
    let date: String = ts.chars().take(10).collect();
    let time: String = ts.chars().skip(11).take(5).collect();
    terminal_safe(format!("{date} {time}").trim_end())
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
#[path = "session_tests.rs"]
mod tests;
