//! Browse saved chat transcripts directly from a profile home without starting
//! a container. Discovery, id resolution, listing, and deletion are shared;
//! [`SessionBackend`] isolates the two agents' on-disk formats. Strict discovery
//! protects `get` and `delete` from partial views, while `list` can report
//! traversal errors alongside readable sessions.

use crate::agent::AgentKind;
use anyhow::{bail, Context, Result};
use serde_json::Value;
#[cfg(unix)]
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};

const MAX_TRANSCRIPT_LINE_BYTES: u64 = 64 * 1024 * 1024;

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

/// Resolve a transcript directory only through real directory entries beneath
/// the profile home. The home is writable by the container, so following an
/// agent-planted `.claude`/`.codex` ancestor link here could make host-side
/// `session delete` remove transcripts outside the profile.
pub(crate) fn checked_session_dir(home: &Path, components: &[&str]) -> Result<Option<PathBuf>> {
    let mut path = home.to_path_buf();
    if !crate::profile::real_dir_exists(&path, "profile home")? {
        return Ok(None);
    }
    for component in components {
        path.push(component);
        if !crate::profile::real_dir_exists(&path, "session directory")? {
            return Ok(None);
        }
    }
    Ok(Some(path))
}

/// Transcript discovery for `session list`: usable files plus non-fatal walk
/// errors that should be reported without hiding every readable transcript.
pub struct SessionDiscovery {
    /// Transcript files that were discovered safely.
    pub files: Vec<PathBuf>,
    /// Non-fatal traversal failures to report alongside partial list results.
    pub errors: Vec<String>,
}

impl SessionDiscovery {
    fn from_files(files: Vec<PathBuf>) -> Self {
        SessionDiscovery {
            files,
            errors: Vec::new(),
        }
    }
}

/// Whether a walked entry is a transcript file we want: a regular `.jsonl` file
/// whose name passes `keep`. Do not follow a transcript-shaped symlink created
/// inside the mounted profile home — host-side session browsing must stay inside
/// the container's transcript tree rather than becoming a path out of the sandbox
/// boundary. Shared by the strict and tolerant walks so they can't drift on which
/// files count.
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
    match std::fs::symlink_metadata(base) {
        Ok(meta) if meta.file_type().is_dir() => {}
        Ok(_) => bail!("session path is not a directory: {}", safe_path(base)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(e)
                .with_context(|| format!("inspect session directory {}", safe_path(base)));
        }
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
    match std::fs::symlink_metadata(base) {
        Ok(meta) if meta.file_type().is_dir() => {}
        Ok(_) => bail!("session path is not a directory: {}", safe_path(base)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SessionDiscovery::from_files(Vec::new()));
        }
        Err(e) => {
            return Err(e)
                .with_context(|| format!("inspect session directory {}", safe_path(base)));
        }
    }
    let mut out = SessionDiscovery::from_files(Vec::new());
    for entry in walkdir::WalkDir::new(base) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                out.errors.push(terminal_safe(&format!(
                    "walk session directory {}: {e}",
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

/// Read a transcript line by line, parsing each as JSON and feeding each parsed
/// line to `f`. Malformed JSON lines are skipped; open and read failures are
/// returned instead of being misreported as an empty session.
///
/// Streaming on purpose: a profile's transcripts can run to hundreds of MB and
/// `list` visits every one, so no whole file — nor its parsed lines — is ever
/// held in memory at once.
pub(crate) fn for_each_json_line(
    home: &Path,
    path: &Path,
    visit: impl FnMut(&Value),
) -> Result<()> {
    for_each_json_line_with_limit(home, path, MAX_TRANSCRIPT_LINE_BYTES, visit)
}

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
}

fn try_for_each_json_line(
    home: &Path,
    path: &Path,
    visit: impl FnMut(&Value) -> Result<bool>,
) -> Result<()> {
    try_for_each_json_line_with_limit(home, path, MAX_TRANSCRIPT_LINE_BYTES, visit)
}

fn try_for_each_json_line_with_limit(
    home: &Path,
    path: &Path,
    max_line_bytes: u64,
    mut visit: impl FnMut(&Value) -> Result<bool>,
) -> Result<()> {
    let file = open_session_transcript(home, path)?;
    let mut reader = io::BufReader::new(file);
    let mut line = String::new();
    let mut line_number = 0_u64;
    loop {
        line.clear();
        let read = (&mut reader)
            // A JSONL record may be followed by either LF or CRLF. Read room
            // for both delimiters so the byte limit applies to the JSON
            // record itself rather than rejecting an exact-size record just
            // because it has a conventional trailing newline.
            .take(max_line_bytes.saturating_add(2))
            .read_line(&mut line);
        match read {
            Ok(0) => return Ok(()),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("read session transcript {}", safe_path(path)));
            }
            Ok(_) => {
                let record = line.strip_suffix('\n').unwrap_or(&line);
                let record = record.strip_suffix('\r').unwrap_or(record);
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
        if let Ok(value) = serde_json::from_str::<Value>(&line) {
            if !visit(&value)? {
                return Ok(());
            }
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
                "session transcript has no profile home ancestor: {}",
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
    crate::profile::open_real_file(path, "session transcript")
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
    crate::profile::open_real_file(path, "session transcript")?;
    fs::remove_file(path).with_context(|| format!("delete {}", safe_path(path)))
}

#[cfg(unix)]
fn open_session_parent(home: &Path, path: &Path) -> Result<(std::os::fd::OwnedFd, OsString)> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    use std::os::unix::ffi::OsStrExt;

    let relative = path.strip_prefix(home).with_context(|| {
        format!(
            "session transcript {} is outside profile home {}",
            safe_path(path),
            safe_path(home)
        )
    })?;
    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => components.push(name.to_os_string()),
            _ => bail!(
                "session transcript path is not a normalized child of the profile home: {}",
                safe_path(path)
            ),
        }
    }
    let file_name = components
        .pop()
        .with_context(|| format!("session transcript path is empty: {}", safe_path(path)))?;

    let home_path = std::ffi::CString::new(home.as_os_str().as_bytes())
        .with_context(|| format!("profile home contains a NUL byte: {}", safe_path(home)))?;
    let home_fd = unsafe {
        libc::open(
            home_path.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    if home_fd < 0 {
        return Err(io::Error::last_os_error())
            .with_context(|| format!("open profile home {}", safe_path(home)));
    }
    let mut parent = unsafe { OwnedFd::from_raw_fd(home_fd) };

    for component in components {
        let component_c = os_str_c_string(&component)?;
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
            "session transcript {} is outside profile home {}",
            safe_path(path),
            safe_path(home)
        )
    })?;
    let mut current = home.to_path_buf();
    crate::profile::real_dir_exists(&current, "profile home")?;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            bail!(
                "session transcript path is not a normalized child of the profile home: {}",
                safe_path(path)
            );
        };
        current.push(name);
        if components.peek().is_some() {
            crate::profile::real_dir_exists(&current, "session directory")?;
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

/// One session's list-row data. Every transcript yields a summary — sessions
/// with no typed prompt (tool/injected-only shells) still list, just with an
/// empty title — so `list` and no-id `delete` can see and clear them all.
pub struct SessionSummary {
    /// Full session id (the row shows the first 8 chars).
    pub id: String,
    /// Session start timestamp (ISO-8601), or empty if none was found.
    pub start_ts: String,
    /// The agent-generated title when available, otherwise the first typed
    /// prompt, or empty for a tool/injected-only session.
    pub title: String,
}

/// One typed prompt from a session, for `get`.
pub struct Prompt {
    /// The turn's timestamp (ISO-8601), or empty.
    pub timestamp: String,
    /// The full prompt text (all content joined; injected wrappers already
    /// filtered by the backend).
    pub text: String,
}

/// The per-agent on-disk transcript format. The two impls
/// (`session_claude::Claude`, `session_codex::Codex`) diverge only in the
/// required methods below — *where* the transcript tree lives, which file
/// names count, *where* each field lives on a line, and which lines count as
/// a real prompt. The discovery walks and the summary/get loops that consume
/// those answers ([`files`](Self::files) / [`list_files`](Self::list_files) /
/// [`summarize_in`](Self::summarize_in) /
/// [`prompts_in`](Self::prompts_in)) are written once here as provided methods,
/// so the two backends can't drift out of sync.
pub trait SessionBackend {
    /// Path components of the transcript tree beneath the profile home
    /// (e.g. `[".claude", "projects"]`), resolved only through real directory
    /// entries so agent-created symlinks are never followed.
    fn session_dir_components(&self) -> &'static [&'static str];

    /// Whether a `.jsonl` file name is a transcript. Claude keeps all; Codex
    /// keeps only `rollout-` names. Shared by [`files`](Self::files) and
    /// [`list_files`](Self::list_files), so `list` can never show a row that
    /// `get`/`delete` then refuse to resolve.
    fn keep_transcript_name(&self, name: &str) -> bool;

    /// All transcript files under this profile home (empty if none yet). The
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
            return Ok(SessionDiscovery::from_files(Vec::new()));
        };
        walk_jsonl_tolerant(&base, |name| self.keep_transcript_name(name))
    }

    /// The session id for a transcript path.
    fn id_of(&self, path: &Path) -> String;

    /// `Some(text)` iff `v` is a prompt the user actually typed — with any
    /// injected/wrapper turns already filtered out. `None` for every other line.
    /// This is the heart of the divergence: Claude keys off `promptSource:typed`,
    /// Codex off a wrapper-filtered `response_item` user message.
    fn typed_text(&self, v: &Value) -> Option<String>;

    /// The session start timestamp from one parsed line; the first `Some` is
    /// retained. Claude answers for any line bearing a top-level `timestamp`;
    /// Codex answers for a `session_meta` timestamp.
    fn start_ts_of(&self, v: &Value) -> Option<String>;

    /// Lower-confidence timestamp candidate used only when
    /// [`start_ts_of`](Self::start_ts_of) never finds one.
    fn fallback_start_ts_of(&self, _v: &Value) -> Option<String> {
        None
    }

    /// A `list` row title candidate from one parsed line. The *last* non-empty
    /// candidate wins; a session with none falls back to its first typed
    /// prompt. Default: no candidates (Codex has no ai-title); Claude overrides
    /// to surface `ai-title` lines.
    fn title_of(&self, _v: &Value) -> Option<String> {
        None
    }

    /// Summarize one transcript for `list`. Every transcript summarizes — a
    /// session with no typed prompt just gets an empty title (unless a backend's
    /// `title_of` finds something else, like Claude's `ai-title`), so tool/
    /// injected-only shells still list and can be cleared. One streaming pass
    /// with O(1) state; the per-agent answers come from the methods above.
    /// `home` anchors no-follow traversal of every path component.
    fn summarize_in(&self, home: &Path, path: &Path) -> Result<SessionSummary> {
        let mut start_ts: Option<String> = None;
        let mut fallback_start_ts: Option<String> = None;
        let mut first_typed: Option<String> = None;
        let mut title: Option<String> = None;
        for_each_json_line(home, path, |value| {
            if start_ts.is_none() {
                start_ts = self.start_ts_of(value);
            }
            if fallback_start_ts.is_none() {
                fallback_start_ts = self.fallback_start_ts_of(value);
            }
            if first_typed.is_none() {
                first_typed = self.typed_text(value);
            }
            if let Some(candidate) = self.title_of(value) {
                if !candidate.is_empty() {
                    title = Some(candidate);
                }
            }
        })?;
        Ok(SessionSummary {
            id: self.id_of(path),
            start_ts: start_ts.or(fallback_start_ts).unwrap_or_default(),
            title: title.or(first_typed).unwrap_or_default(),
        })
    }

    #[cfg(test)]
    /// Test helper that derives the fixture home from the backend's tree.
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
    ) -> Result<usize> {
        let mut count = 0;
        try_for_each_json_line(home, path, |value| {
            if let Some(text) = self.typed_text(value) {
                count += 1;
                return visit(Prompt {
                    timestamp: ts_of(value),
                    text,
                });
            }
            Ok(true)
        })?;
        Ok(count)
    }

    #[cfg(test)]
    /// Test helper that derives the fixture home from the backend's tree.
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
pub fn backend_for(agent: AgentKind) -> Box<dyn SessionBackend> {
    match agent {
        AgentKind::Claude => Box::new(crate::session_claude::Claude),
        AgentKind::Codex => Box::new(crate::session_codex::Codex),
    }
}

/// Dispatch a host-side session action.
///
/// `list` accepts no ids or flags; `get` requires exactly one id; `delete`
/// accepts ids or selects every transcript when `all` is true or `ids` is
/// empty. Only `delete` accepts `yes`. The return value is the command exit
/// code; `list` returns 1 when it can show only a partial result.
pub fn dispatch(
    agent: AgentKind,
    home: &Path,
    action: &str,
    ids: &[String],
    all: bool,
    yes: bool,
) -> Result<i32> {
    let backend = backend_for(agent);
    match action {
        "list" => {
            reject_yes("list", yes)?;
            reject_all("list", all)?;
            if !ids.is_empty() {
                bail!("session list does not accept ids");
            }
            list(backend.as_ref(), home)
        }
        "get" => {
            reject_yes("get", yes)?;
            reject_all("get", all)?;
            match ids {
                [id] => get(backend.as_ref(), home, id),
                [] => bail!("need a session id (or unique prefix)"),
                _ => bail!("session get accepts exactly one id"),
            }
        }
        "delete" => delete(backend.as_ref(), home, ids, all, yes),
        other => bail!("unknown session action: {other} (use list|get|delete)"),
    }
}

fn reject_yes(action: &str, yes: bool) -> Result<()> {
    if yes {
        bail!("session {action} does not use -y/--yes");
    }
    Ok(())
}

fn reject_all(action: &str, all: bool) -> Result<()> {
    if all {
        bail!("session {action} does not use --all");
    }
    Ok(())
}

/// Resolve a full id or unique prefix to exactly one transcript path. A single
/// exact id wins even when it prefixes other ids (otherwise that session could
/// never be addressed at all), but duplicate exact ids remain ambiguous rather
/// than selecting whichever directory the filesystem happened to visit first.
/// Zero matches or ambiguous candidates fail with a message.
fn resolve(backend: &dyn SessionBackend, home: &Path, query: &str) -> Result<PathBuf> {
    resolve_in(backend, &backend.files(home)?, query)
}

/// Resolve `query` against an already-discovered file list, so callers with many
/// ids (`delete a b c`) can walk the transcript tree once instead of per id.
fn resolve_in(backend: &dyn SessionBackend, files: &[PathBuf], query: &str) -> Result<PathBuf> {
    if query.is_empty() {
        bail!("need a session id (or unique prefix)");
    }
    let mut exact_matches: Vec<PathBuf> = Vec::new();
    let mut prefix_matches: Vec<PathBuf> = Vec::new();
    for file in files {
        let id = backend.id_of(file);
        if id == query {
            exact_matches.push(file.clone());
        } else if id.starts_with(query) {
            prefix_matches.push(file.clone());
        }
    }
    let candidates = if exact_matches.is_empty() {
        prefix_matches
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
                message.push_str(&format!(
                    "\n     {}  {}",
                    terminal_safe(&backend.id_of(candidate)),
                    safe_path(candidate)
                ));
            }
            bail!(message)
        }
    }
}

const LIST_TITLE_MAX_CHARS: usize = 64;

/// List this profile's sessions, newest first: `shortid  date  title`. Every
/// transcript lists (tool/injected-only shells show an empty title) so nothing is
/// hidden from `list` or no-id `delete`. Columns are `%-8s  %-16s  %s`; titles
/// are collapsed to one line and capped at 64 chars.
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
            eprintln!(">> no sessions in this profile");
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
        // Escape terminal controls before truncating: ids come from arbitrary
        // transcript file names inside the container-writable profile home.
        let short_id: String = terminal_safe(&id).chars().take(8).collect();
        let timestamp = fmt_ts(&start_ts);
        if !print(&format!("{short_id:<8}  {timestamp:<16}  {title}"))? {
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
    let prompt_count = backend.for_each_prompt_in(home, &path, &mut |prompt| {
        index += 1;
        let timestamp = fmt_ts(&prompt.timestamp);
        let text = terminal_safe_prompt(&prompt.text);
        print(&format!("\n[{index}] {timestamp}\n{text}"))
    })?;
    if prompt_count == 0 {
        print("(no typed prompts in this session)")?;
    }
    Ok(0)
}

/// Delete transcripts, asking once per target unless `yes` is set. Passing no
/// ids or `--all` selects every transcript for this profile.
fn delete(
    backend: &dyn SessionBackend,
    home: &Path,
    ids: &[String],
    all: bool,
    yes: bool,
) -> Result<i32> {
    let targets = delete_targets(backend, home, ids, all)?;
    if targets.is_empty() {
        eprintln!(">> no sessions in this profile");
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

    if all || ids.is_empty() {
        // Match `list`: bulk deletion includes tool/injected-only transcripts
        // that have no typed prompt.
        let mut targets = backend.files(home)?;
        targets.sort_by_key(|p| backend.id_of(p));
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
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for character in s.chars() {
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

fn list_title(s: &str) -> String {
    collapse_ws(s).chars().take(LIST_TITLE_MAX_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::io::Cursor;

    struct TestBackend;

    impl SessionBackend for TestBackend {
        fn session_dir_components(&self) -> &'static [&'static str] {
            &["sessions"]
        }

        fn keep_transcript_name(&self, _name: &str) -> bool {
            true
        }

        fn id_of(&self, path: &Path) -> String {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string()
        }

        fn typed_text(&self, v: &Value) -> Option<String> {
            v.get("typed").and_then(Value::as_str).map(str::to_string)
        }

        fn start_ts_of(&self, v: &Value) -> Option<String> {
            v.get("ts").and_then(Value::as_str).map(str::to_string)
        }
    }

    fn write_session(home: &Path, id: &str) -> PathBuf {
        let path = home.join("sessions").join(format!("{id}.jsonl"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{}\n").unwrap();
        path
    }

    #[test]
    fn transcript_line_reader_rejects_oversized_lines_without_reading_the_whole_file() {
        let home = tempfile::tempdir().unwrap();
        let path = write_session(home.path(), "oversized");
        std::fs::write(&path, vec![b'x'; 33]).unwrap();

        let error = for_each_json_line_with_limit(home.path(), &path, 32, |_| {})
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("line 1 exceeds the 32 byte limit"),
            "{error}"
        );
    }

    #[test]
    fn transcript_line_limit_accepts_an_exact_size_final_line() {
        let home = tempfile::tempdir().unwrap();
        let path = write_session(home.path(), "exact");
        let line = br#"{"typed":"ok"}"#;
        std::fs::write(&path, line).unwrap();
        let mut visits = 0;

        for_each_json_line_with_limit(home.path(), &path, line.len() as u64, |_| {
            visits += 1;
        })
        .unwrap();

        assert_eq!(visits, 1);
    }

    #[test]
    fn transcript_line_limit_excludes_jsonl_delimiters() {
        let home = tempfile::tempdir().unwrap();
        let path = write_session(home.path(), "exact-with-newline");
        let record = r#"{"typed":"ok"}"#;

        for delimiter in ["\n", "\r\n"] {
            std::fs::write(&path, format!("{record}{delimiter}")).unwrap();
            let mut visits = 0;

            for_each_json_line_with_limit(home.path(), &path, record.len() as u64, |_| visits += 1)
                .unwrap();

            assert_eq!(visits, 1, "delimiter {delimiter:?}");
        }
    }

    #[test]
    fn session_display_escapes_terminal_controls_from_container_owned_data() {
        assert_eq!(terminal_safe("普通\x1b[2J\n"), "普通\\u{1b}[2J\\n");
        assert_eq!(
            terminal_safe_prompt("first\n\tsecond\x1b[2J"),
            "first\n\tsecond\\u{1b}[2J"
        );

        let home = tempfile::tempdir().unwrap();
        write_session(home.path(), "\x1b[2Jmalicious");
        let mut lines = Vec::new();
        let code = list_with_printer(&TestBackend, home.path(), |line| {
            lines.push(line.to_string());
            Ok(true)
        })
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(lines.len(), 1);
        assert!(!lines[0].contains('\x1b'), "{:?}", lines[0]);
        assert!(lines[0].contains("\\u{1b}"), "{:?}", lines[0]);
    }

    struct ExplicitFilesBackend {
        files: Vec<PathBuf>,
        list_errors: Vec<String>,
        files_error: Option<String>,
    }

    impl ExplicitFilesBackend {
        fn new(files: Vec<PathBuf>) -> Self {
            ExplicitFilesBackend {
                files,
                list_errors: Vec::new(),
                files_error: None,
            }
        }

        fn with_list_errors(files: Vec<PathBuf>, list_errors: Vec<String>) -> Self {
            ExplicitFilesBackend {
                files,
                list_errors,
                files_error: None,
            }
        }

        fn with_files_error(message: &str) -> Self {
            ExplicitFilesBackend {
                files: Vec::new(),
                list_errors: Vec::new(),
                files_error: Some(message.to_string()),
            }
        }
    }

    impl SessionBackend for ExplicitFilesBackend {
        // Never reached: this backend overrides both discovery walks with its
        // explicit lists, which is the point — the shared list/get/delete
        // logic under test takes whatever discovery hands it.
        fn session_dir_components(&self) -> &'static [&'static str] {
            &[]
        }

        fn keep_transcript_name(&self, _name: &str) -> bool {
            true
        }

        fn files(&self, _home: &Path) -> Result<Vec<PathBuf>> {
            if let Some(message) = &self.files_error {
                bail!("{message}");
            }
            Ok(self.files.clone())
        }

        fn list_files(&self, _home: &Path) -> Result<SessionDiscovery> {
            Ok(SessionDiscovery {
                files: self.files.clone(),
                errors: self.list_errors.clone(),
            })
        }

        fn id_of(&self, path: &Path) -> String {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string()
        }

        fn typed_text(&self, v: &Value) -> Option<String> {
            v.get("typed").and_then(Value::as_str).map(str::to_string)
        }

        fn start_ts_of(&self, v: &Value) -> Option<String> {
            v.get("ts").and_then(Value::as_str).map(str::to_string)
        }
    }

    #[test]
    fn fmt_ts_positional() {
        assert_eq!(fmt_ts("2026-07-14T02:16:33.123Z"), "2026-07-14 02:16");
        assert_eq!(fmt_ts(""), "");
    }

    #[test]
    fn collapse_ws_runs() {
        assert_eq!(collapse_ws("a\n\nb\tc"), "a b c");
        assert_eq!(collapse_ws("a\rb\u{7f}c\u{00a0}d"), "a b c d");
        assert_eq!(collapse_ws("a  b"), "a  b");
        assert_eq!(collapse_ws("plain"), "plain");
    }

    #[test]
    fn list_title_collapses_and_truncates_to_64_chars() {
        assert_eq!(list_title("a\n\nb\tc"), "a b c");
        let long: String = "0123456789".repeat(7); // 70 chars
        assert_eq!(list_title(&long), long[..64].to_string());
        assert_eq!(list_title(&long).chars().count(), 64);

        let multibyte = "é".repeat(70);
        assert_eq!(list_title(&multibyte), "é".repeat(64));
        assert_eq!(list_title(&multibyte).chars().count(), 64);
    }

    #[test]
    fn list_shortens_multibyte_session_ids_by_chars() {
        let dir = tempfile::tempdir().unwrap();
        let id = "é".repeat(10);
        let path = dir.path().join(format!("{id}.jsonl"));
        std::fs::write(&path, "{\"typed\":\"bonjour\"}\n").unwrap();
        let backend = ExplicitFilesBackend::new(vec![path]);
        let mut lines = Vec::new();

        let code = list_with_printer(&backend, dir.path(), |line| {
            lines.push(line.to_string());
            Ok(true)
        })
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(lines.len(), 1);
        let mut expected_prefix = "é".repeat(8);
        expected_prefix.push_str("  ");
        assert!(
            lines[0].starts_with(&expected_prefix),
            "short ids must be truncated on char boundaries, not byte boundaries: {lines:?}"
        );
    }

    #[test]
    fn non_utf8_transcript_is_reported_by_list_and_fails_the_read_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("33333333.jsonl");
        // Valid line, then a lone continuation byte: read_line errors on it.
        std::fs::write(&path, b"{\"typed\":\"ok\"}\n\xff\xfe").unwrap();
        let backend = ExplicitFilesBackend::new(vec![path.clone()]);

        let err = backend
            .prompts(&path)
            .err()
            .expect("invalid UTF-8 must not read as an empty prompt list")
            .to_string();
        assert!(err.contains("read session transcript"), "{err}");
        assert!(err.contains("33333333.jsonl"), "{err}");

        let err = get_with_printer(&backend, dir.path(), "3333", |_| Ok(true))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("read session transcript"),
            "get must surface the read failure: {err}"
        );

        let mut lines = Vec::new();
        let code = list_with_printer(&backend, dir.path(), |line| {
            lines.push(line.to_string());
            Ok(true)
        })
        .unwrap();
        assert_eq!(code, 1, "an unreadable transcript makes list non-zero");
        assert!(
            lines.is_empty(),
            "no row for a transcript that failed to read"
        );
    }

    #[test]
    fn list_skips_bad_transcripts_but_returns_nonzero() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.jsonl");
        let good = dir.path().join("good.jsonl");
        std::fs::write(&good, "{\"typed\":\"hello\"}\n").unwrap();
        let backend = ExplicitFilesBackend::new(vec![missing, good]);
        let mut lines = Vec::new();

        let code = list_with_printer(&backend, dir.path(), |line| {
            lines.push(line.to_string());
            Ok(true)
        })
        .unwrap();

        assert_eq!(code, 1, "one skipped transcript makes list non-zero");
        assert_eq!(lines.len(), 1, "the readable session still lists");
        assert!(lines[0].contains("good"));
        assert!(lines[0].contains("hello"));
    }

    #[test]
    fn get_prints_numbered_timestamped_prompts_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("11111111.jsonl");
        std::fs::write(
            &path,
            "\
{\"timestamp\":\"2026-07-14T02:16:33.123Z\",\"typed\":\"first ask\"}
{\"timestamp\":\"2026-07-14T02:18:00Z\",\"typed\":\"second ask\"}
",
        )
        .unwrap();
        let backend = ExplicitFilesBackend::new(vec![path]);
        let mut printed = Vec::new();

        let code = get_with_printer(&backend, dir.path(), "1111", |line| {
            printed.push(line.to_string());
            Ok(true)
        })
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(
            printed,
            vec![
                "\n[1] 2026-07-14 02:16\nfirst ask".to_string(),
                "\n[2] 2026-07-14 02:18\nsecond ask".to_string(),
            ],
            "get numbers prompts from 1 and shows each turn's minute-precision timestamp"
        );
    }

    #[test]
    fn get_reports_a_session_with_no_typed_prompts() {
        // A tool/injected-only shell resolves and exits 0 — it must say so
        // rather than printing nothing at all.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("22222222.jsonl");
        std::fs::write(&path, "{\"ts\":\"2026-07-14T02:16:00Z\"}\n").unwrap();
        let backend = ExplicitFilesBackend::new(vec![path]);
        let mut printed = Vec::new();

        let code = get_with_printer(&backend, dir.path(), "2222", |line| {
            printed.push(line.to_string());
            Ok(true)
        })
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(printed, vec!["(no typed prompts in this session)"]);
    }

    #[test]
    fn get_stops_cleanly_when_printer_hangs_up() {
        // `session get | head` closes the pipe; the Rust runtime ignores
        // SIGPIPE, so this must stop reading and writing instead of reaching
        // malformed data later in a large transcript.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("33333333.jsonl");
        std::fs::write(&path, b"{\"typed\":\"first\"}\n\xff\xfe").unwrap();
        let backend = ExplicitFilesBackend::new(vec![path]);
        let mut printed = Vec::new();

        let code = get_with_printer(&backend, dir.path(), "3333", |line| {
            printed.push(line.to_string());
            Ok(false)
        })
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(
            printed.len(),
            1,
            "get stops after a broken-pipe-style false"
        );
    }

    #[test]
    fn get_still_fails_fast_on_bad_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.jsonl");
        let backend = ExplicitFilesBackend::new(vec![missing]);

        let err = get(&backend, dir.path(), "missing")
            .unwrap_err()
            .to_string();

        assert!(err.contains("open session transcript"), "{err}");
    }

    #[test]
    fn list_reports_discovery_errors_but_keeps_readable_transcripts() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good.jsonl");
        std::fs::write(&good, "{\"typed\":\"hello\"}\n").unwrap();
        let backend = ExplicitFilesBackend::with_list_errors(
            vec![good],
            vec!["walk session directory /sessions: permission denied".to_string()],
        );
        let mut lines = Vec::new();

        let code = list_with_printer(&backend, dir.path(), |line| {
            lines.push(line.to_string());
            Ok(true)
        })
        .unwrap();

        assert_eq!(code, 1, "discovery errors make list non-zero");
        assert_eq!(lines.len(), 1, "readable sessions still list");
        assert!(lines[0].contains("hello"));
    }

    #[test]
    fn list_orders_sessions_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.jsonl");
        let new = dir.path().join("new.jsonl");
        std::fs::write(
            &old,
            "{\"ts\":\"2026-07-14T02:16:00Z\",\"typed\":\"old prompt\"}\n",
        )
        .unwrap();
        std::fs::write(
            &new,
            "{\"ts\":\"2026-07-14T02:17:00Z\",\"typed\":\"new prompt\"}\n",
        )
        .unwrap();
        let backend = ExplicitFilesBackend::new(vec![old, new]);
        let mut lines = Vec::new();

        let code = list_with_printer(&backend, dir.path(), |line| {
            lines.push(line.to_string());
            Ok(true)
        })
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].contains("new prompt") && lines[1].contains("old prompt"),
            "list rows should be newest first: {lines:?}"
        );
    }

    #[test]
    fn list_places_sessions_without_timestamps_after_timestamped_rows() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.jsonl");
        let new = dir.path().join("new.jsonl");
        let no_ts = dir.path().join("no-ts.jsonl");
        std::fs::write(
            &old,
            "{\"ts\":\"2026-07-14T02:16:00Z\",\"typed\":\"old prompt\"}\n",
        )
        .unwrap();
        std::fs::write(
            &new,
            "{\"ts\":\"2026-07-14T02:17:00Z\",\"typed\":\"new prompt\"}\n",
        )
        .unwrap();
        std::fs::write(&no_ts, "{\"typed\":\"no timestamp\"}\n").unwrap();
        let backend = ExplicitFilesBackend::new(vec![no_ts, new, old]);
        let mut lines = Vec::new();

        let code = list_with_printer(&backend, dir.path(), |line| {
            lines.push(line.to_string());
            Ok(true)
        })
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(lines.len(), 3);
        assert!(
            lines[0].contains("new prompt")
                && lines[1].contains("old prompt")
                && lines[2].contains("no timestamp"),
            "timestamp-less sessions should not sort above real timestamps: {lines:?}"
        );
    }

    #[test]
    fn list_stops_cleanly_when_printer_hangs_up() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.jsonl");
        let second = dir.path().join("second.jsonl");
        std::fs::write(
            &first,
            "{\"ts\":\"2026-07-14T02:17:00Z\",\"typed\":\"first\"}\n",
        )
        .unwrap();
        std::fs::write(
            &second,
            "{\"ts\":\"2026-07-14T02:16:00Z\",\"typed\":\"second\"}\n",
        )
        .unwrap();
        let backend = ExplicitFilesBackend::new(vec![first, second]);
        let mut printed = Vec::new();

        let code = list_with_printer(&backend, dir.path(), |line| {
            printed.push(line.to_string());
            Ok(false)
        })
        .unwrap();

        assert_eq!(code, 0);
        assert_eq!(
            printed.len(),
            1,
            "list should stop writing after a broken-pipe-style false"
        );
        assert!(printed[0].contains("first"));
    }

    #[test]
    fn get_and_delete_still_fail_fast_on_discovery_errors() {
        let dir = tempfile::tempdir().unwrap();
        let backend = ExplicitFilesBackend::with_files_error("discovery failed");

        let err = get(&backend, dir.path(), "anything")
            .unwrap_err()
            .to_string();
        assert!(err.contains("discovery failed"), "{err}");

        let err = delete(&backend, dir.path(), &[], false, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("discovery failed"), "{err}");
    }

    #[test]
    fn resolved_snapshots_cannot_read_or_delete_outside_or_non_normalized_paths() {
        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let outside_path = outside.path().join("outside.jsonl");
        let inside_path = home.path().join("inside.jsonl");
        std::fs::write(&outside_path, "{\"typed\":\"outside\"}\n").unwrap();
        std::fs::write(&inside_path, "{\"typed\":\"inside\"}\n").unwrap();

        for (candidate, expected) in [
            (outside_path.clone(), "outside profile home"),
            (
                home.path().join("sessions/../inside.jsonl"),
                "not a normalized child",
            ),
        ] {
            let backend = ExplicitFilesBackend::new(vec![candidate.clone()]);
            let id = backend.id_of(&candidate);
            let err = get_with_printer(&backend, home.path(), &id, |_| Ok(true))
                .unwrap_err()
                .to_string();
            assert!(err.contains(expected), "{candidate:?}: {err}");

            let mut input = Cursor::new(Vec::<u8>::new());
            let err = delete_targets_with_input(
                &backend,
                home.path(),
                vec![candidate.clone()],
                true,
                &mut input,
            )
            .unwrap_err()
            .to_string();
            assert!(err.contains(expected), "{candidate:?}: {err}");
        }

        assert!(outside_path.exists());
        assert!(inside_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn session_discovery_does_not_follow_transcript_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside_file = dir.path().join("outside.jsonl");
        std::fs::write(&outside_file, "{}\n").unwrap();
        let outside_dir = dir.path().join("outside-dir");
        std::fs::create_dir(&outside_dir).unwrap();
        std::fs::write(outside_dir.join("nested.jsonl"), "{}\n").unwrap();
        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        symlink(&outside_file, sessions.join("linked.jsonl")).unwrap();
        symlink(&outside_dir, sessions.join("linked-dir")).unwrap();

        let files = TestBackend.files(dir.path()).unwrap();

        assert!(
            files.is_empty(),
            "host-side browsing must not follow transcript or directory symlinks"
        );
    }

    #[cfg(unix)]
    #[test]
    fn get_rejects_symlinked_transcript_even_from_a_resolved_snapshot() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside.jsonl");
        let link = dir.path().join("11111111.jsonl");
        std::fs::write(&outside, "{\"typed\":\"outside\"}\n").unwrap();
        symlink(&outside, &link).unwrap();
        let backend = ExplicitFilesBackend::new(vec![link]);

        let err = get_with_printer(&backend, dir.path(), "1111", |_| Ok(true))
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("session transcript is not a regular file"),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn delete_rejects_a_transcript_replaced_by_a_symlink_after_discovery() {
        use std::os::unix::fs::symlink;

        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let transcript = write_session(home.path(), "11111111");
        let targets =
            delete_targets(&TestBackend, home.path(), &["1111".to_string()], false).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0], transcript);

        std::fs::remove_file(&transcript).unwrap();
        let outside_transcript = outside.path().join("outside.jsonl");
        std::fs::write(&outside_transcript, "{\"typed\":\"outside\"}\n").unwrap();
        symlink(&outside_transcript, &transcript).unwrap();

        let mut input = Cursor::new(Vec::<u8>::new());
        let err = delete_targets_with_input(&TestBackend, home.path(), targets, true, &mut input)
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("session transcript is not a regular file"),
            "{err}"
        );
        assert_eq!(
            std::fs::read_to_string(&outside_transcript).unwrap(),
            "{\"typed\":\"outside\"}\n"
        );
        assert!(
            transcript
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink(),
            "a failed delete must leave the replacement symlink itself untouched"
        );
    }

    #[cfg(unix)]
    #[test]
    fn get_rejects_fifo_replacement_without_blocking() {
        use std::os::unix::ffi::OsStrExt;

        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("11111111.jsonl");
        let fifo_path = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        let result = unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) };
        assert_eq!(result, 0, "create FIFO: {}", io::Error::last_os_error());
        let backend = ExplicitFilesBackend::new(vec![fifo]);

        let err = get_with_printer(&backend, dir.path(), "1111", |_| Ok(true))
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("session transcript is not a regular file"),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn reads_do_not_follow_an_ancestor_replaced_after_discovery() {
        use std::os::unix::fs::symlink;

        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let transcript = write_session(home.path(), "11111111");
        let snapshot = TestBackend.files(home.path()).unwrap();
        assert_eq!(snapshot, [transcript]);

        std::fs::remove_file(&snapshot[0]).unwrap();
        std::fs::remove_dir(home.path().join("sessions")).unwrap();
        std::fs::write(
            outside.path().join("11111111.jsonl"),
            "{\"typed\":\"outside\"}\n",
        )
        .unwrap();
        symlink(outside.path(), home.path().join("sessions")).unwrap();

        let err = TestBackend
            .prompts_in(home.path(), &snapshot[0])
            .err()
            .expect("a replaced ancestor must be rejected")
            .to_string();

        assert!(err.contains("open session path"), "{err}");
        assert!(
            outside.path().join("11111111.jsonl").exists(),
            "reading a resolved snapshot must not follow a replaced ancestor"
        );
    }

    #[cfg(unix)]
    #[test]
    fn delete_does_not_follow_an_ancestor_replaced_after_discovery() {
        use std::os::unix::fs::symlink;

        let home = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let transcript = write_session(home.path(), "11111111");
        let targets =
            delete_targets(&TestBackend, home.path(), &["1111".to_string()], false).unwrap();
        assert_eq!(targets, [transcript]);

        std::fs::remove_file(&targets[0]).unwrap();
        std::fs::remove_dir(home.path().join("sessions")).unwrap();
        let outside_transcript = outside.path().join("11111111.jsonl");
        std::fs::write(&outside_transcript, "{}\n").unwrap();
        symlink(outside.path(), home.path().join("sessions")).unwrap();

        let mut input = Cursor::new(Vec::<u8>::new());
        let err = delete_targets_with_input(&TestBackend, home.path(), targets, true, &mut input)
            .unwrap_err()
            .to_string();

        assert!(err.contains("open session path"), "{err}");
        assert!(
            outside_transcript.exists(),
            "deleting a resolved snapshot must not follow a replaced ancestor"
        );
    }

    #[cfg(unix)]
    #[test]
    fn tolerant_session_discovery_does_not_follow_transcript_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside.jsonl");
        std::fs::write(&outside, "{}\n").unwrap();
        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        symlink(&outside, sessions.join("linked.jsonl")).unwrap();

        let discovery = walk_jsonl_tolerant(&sessions, |_| true).unwrap();

        assert!(
            discovery.files.is_empty(),
            "list's tolerant walk must not follow transcript-shaped symlinks"
        );
        assert!(
            discovery.errors.is_empty(),
            "skipped transcript symlinks should not be reported as walk failures"
        );
    }

    #[test]
    fn session_discovery_rejects_a_non_directory_session_path() {
        // A file where the transcript tree should be is a broken profile, not an
        // empty one: reporting "no sessions" would hide it.
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        std::fs::write(&sessions, "not a directory\n").unwrap();

        let err = walk_jsonl(&sessions, |_| true).unwrap_err().to_string();
        assert!(err.contains("session path is not a directory"), "{err}");

        let err = walk_jsonl_tolerant(&sessions, |_| true)
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert!(err.contains("session path is not a directory"), "{err}");
    }

    #[test]
    fn session_discovery_reports_no_files_for_a_missing_tree() {
        // A profile that has never run an agent has no transcript dir at all;
        // that is empty, not an error.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("never-used");

        assert!(walk_jsonl(&missing, |_| true).unwrap().is_empty());
        let discovery = walk_jsonl_tolerant(&missing, |_| true).unwrap();
        assert!(discovery.files.is_empty());
        assert!(discovery.errors.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn tolerant_walk_reports_unreadable_subdirectories_without_hiding_readable_ones() {
        // `list`'s walk is tolerant on purpose: one unreadable child dir must be
        // reported while every readable transcript still lists.
        let dir = tempfile::tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        let readable = sessions.join("readable");
        let locked = sessions.join("locked");
        std::fs::create_dir_all(&readable).unwrap();
        std::fs::create_dir_all(&locked).unwrap();
        let good = readable.join("11111111.jsonl");
        std::fs::write(&good, "{\"typed\":\"hello\"}\n").unwrap();
        std::fs::write(locked.join("22222222.jsonl"), "{}\n").unwrap();
        let lock = crate::testutil::UnreadableDir::new(&locked);

        let discovery = walk_jsonl_tolerant(&sessions, |_| true).unwrap();
        lock.restore();

        assert_eq!(
            discovery.files,
            vec![good],
            "the readable transcript must still be discovered"
        );
        assert_eq!(
            discovery.errors.len(),
            1,
            "the unreadable subdirectory is reported: {:?}",
            discovery.errors
        );
        assert!(
            discovery.errors[0].contains("walk session directory"),
            "{:?}",
            discovery.errors
        );

        // The strict walk `get`/`delete` use instead fails fast: a destructive
        // or single-target action must not act on a partial view of the tree.
        let lock = crate::testutil::UnreadableDir::new(&locked);
        let strict = walk_jsonl(&sessions, |_| true);
        lock.restore();
        let err = strict.unwrap_err().to_string();
        assert!(err.contains("walk session directory"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn session_discovery_rejects_a_symlinked_profile_home() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let actual_home = root.path().join("actual-home");
        let linked_home = root.path().join("linked-home");
        write_session(&actual_home, "11111111");
        symlink(&actual_home, &linked_home).unwrap();

        let err = TestBackend.files(&linked_home).unwrap_err().to_string();

        assert!(
            err.contains("profile home is not a real directory"),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn session_discovery_rejects_a_symlinked_agent_state_directory() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let outside = root.path().join("outside-claude");
        let transcript = outside.join("projects/p/11111111.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(&transcript, "{}\n").unwrap();
        std::fs::create_dir(&home).unwrap();
        symlink(&outside, home.join(".claude")).unwrap();

        let err = crate::session_claude::Claude
            .files(&home)
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("session directory is not a real directory"),
            "{err}"
        );
        assert!(
            transcript.exists(),
            "outside transcript must remain untouched"
        );
    }

    #[test]
    fn list_and_delete_report_an_empty_profile_without_failing() {
        let dir = tempfile::tempdir().unwrap();
        let missing_home = dir.path().join("missing-home");

        for home in [dir.path(), missing_home.as_path()] {
            let mut printed = Vec::new();
            let code = list_with_printer(&TestBackend, home, |line| {
                printed.push(line.to_string());
                Ok(true)
            })
            .unwrap();

            assert_eq!(code, 0, "an empty profile is not a list failure");
            assert!(printed.is_empty(), "no rows to print: {printed:?}");

            let code = delete(&TestBackend, home, &[], false, true).unwrap();
            assert_eq!(code, 0, "deleting nothing is not a failure");
        }
        assert!(
            !missing_home.exists(),
            "session discovery must not initialize a profile home"
        );
    }

    #[test]
    fn delete_no_ids_selects_all_sessions_with_yes() {
        let dir = tempfile::tempdir().unwrap();
        let one = write_session(dir.path(), "11111111");
        let two = write_session(dir.path(), "22222222");

        delete(&TestBackend, dir.path(), &[], false, true).unwrap();

        assert!(!one.exists());
        assert!(!two.exists());
    }

    #[test]
    fn delete_all_flag_selects_all_sessions_with_yes() {
        let dir = tempfile::tempdir().unwrap();
        let one = write_session(dir.path(), "11111111");
        let two = write_session(dir.path(), "22222222");

        delete(&TestBackend, dir.path(), &[], true, true).unwrap();

        assert!(!one.exists());
        assert!(!two.exists());
    }

    #[test]
    fn delete_all_flag_cannot_be_mixed_with_ids() {
        let dir = tempfile::tempdir().unwrap();
        let one = write_session(dir.path(), "11111111");

        let err = delete(
            &TestBackend,
            dir.path(),
            &["11111111".to_string()],
            true,
            true,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("--all cannot be combined"), "{err}");
        assert!(one.exists());
    }

    #[test]
    fn delete_treats_all_as_a_session_id_without_all_flag() {
        let dir = tempfile::tempdir().unwrap();
        let all = write_session(dir.path(), "all");
        let other = write_session(dir.path(), "11111111");

        delete(&TestBackend, dir.path(), &["all".to_string()], false, true).unwrap();

        assert!(!all.exists());
        assert!(other.exists());
    }

    #[test]
    fn delete_no_ids_includes_sessions_without_typed_prompts() {
        // No-id delete clears the whole profile — including tool/injected-only
        // shells that carry no typed prompt. `list` shows those same shells
        // (empty title), so the two stay consistent and all rows are removable.
        let dir = tempfile::tempdir().unwrap();
        let a = write_session(dir.path(), "11111111");
        let shell = dir.path().join("sessions").join("22222222.jsonl");
        std::fs::write(&shell, "{}\n").unwrap();

        let targets = delete_targets(&TestBackend, dir.path(), &[], false).unwrap();

        assert_eq!(targets, vec![a, shell]);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_transcript_names_still_pass_discovery_filters() {
        use std::os::unix::ffi::OsStringExt;

        let transcript =
            PathBuf::from(std::ffi::OsString::from_vec(b"invalid-\xff.jsonl".to_vec()));

        assert!(
            has_wanted_transcript_name(&transcript, &|name| name == "invalid-\u{fffd}.jsonl"),
            "discovery must not silently omit a transcript solely because its name is not UTF-8"
        );
    }

    #[test]
    fn summarize_empty_shell_has_empty_title() {
        let dir = tempfile::tempdir().unwrap();
        let shell = dir.path().join("sessions").join("33333333.jsonl");
        std::fs::create_dir_all(shell.parent().unwrap()).unwrap();
        std::fs::write(&shell, "{}\n").unwrap();

        let s = TestBackend.summarize(&shell).unwrap();
        assert_eq!(s.title, "");
        assert!(s.id.starts_with("33333333"));
    }

    #[test]
    fn delete_multiple_ids_confirms_each_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let keep = write_session(dir.path(), "11111111");
        let remove = write_session(dir.path(), "22222222");
        let targets = delete_targets(
            &TestBackend,
            dir.path(),
            &["2222".to_string(), "1111".to_string()],
            false,
        )
        .unwrap();
        let mut input = Cursor::new(b"y\nn\n");

        delete_targets_with_input(&TestBackend, dir.path(), targets, false, &mut input).unwrap();

        assert!(keep.exists());
        assert!(!remove.exists());
    }

    #[test]
    fn delete_refuses_noninteractive_confirmation_without_yes() {
        if io::stdin().is_terminal() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let target = write_session(dir.path(), "11111111");

        let err = delete(
            &TestBackend,
            dir.path(),
            &["1111".to_string()],
            false,
            false,
        )
        .unwrap_err()
        .to_string();

        assert!(
            err.contains("without --yes in a non-interactive shell"),
            "{err}"
        );
        assert!(target.exists());
    }

    #[test]
    fn confirm_delete_accepts_only_explicit_yes_answers() {
        for yes in ["y\n", "Y\n", "yes\n", " YES \n"] {
            let mut input = Cursor::new(yes.as_bytes());
            assert!(confirm_delete("11111111", &mut input).unwrap(), "{yes:?}");
        }

        for no in ["", "\n", "n\n", "yeah\n", "yep\n", " yes please\n"] {
            let mut input = Cursor::new(no.as_bytes());
            assert!(!confirm_delete("11111111", &mut input).unwrap(), "{no:?}");
        }
    }

    #[test]
    fn delete_confirmation_read_errors_are_not_reported_as_successful_keeps() {
        struct FailingInput;

        impl io::Read for FailingInput {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("input failed"))
            }
        }

        impl BufRead for FailingInput {
            fn fill_buf(&mut self) -> io::Result<&[u8]> {
                Err(io::Error::other("input failed"))
            }

            fn consume(&mut self, _amount: usize) {}
        }

        let dir = tempfile::tempdir().unwrap();
        let target = write_session(dir.path(), "11111111");
        let targets =
            delete_targets(&TestBackend, dir.path(), &["1111".to_string()], false).unwrap();
        let error =
            delete_targets_with_input(&TestBackend, dir.path(), targets, false, &mut FailingInput)
                .unwrap_err()
                .to_string();

        assert!(
            error.contains("read session delete confirmation"),
            "{error}"
        );
        assert!(target.exists(), "an unread confirmation must not delete");
    }

    #[test]
    fn delete_targets_dedupes_repeated_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_session(dir.path(), "11111111");

        let targets = delete_targets(
            &TestBackend,
            dir.path(),
            &["1111".to_string(), "11111111".to_string()],
            false,
        )
        .unwrap();

        assert_eq!(targets, vec![path]);
    }

    #[test]
    fn delete_no_ids_orders_targets_by_session_id() {
        let dir = tempfile::tempdir().unwrap();
        let z = dir.path().join("z-session.jsonl");
        let a = dir.path().join("a-session.jsonl");
        std::fs::write(&z, "{}\n").unwrap();
        std::fs::write(&a, "{}\n").unwrap();
        let backend = ExplicitFilesBackend::new(vec![z.clone(), a.clone()]);

        let targets = delete_targets(&backend, dir.path(), &[], false).unwrap();

        assert_eq!(
            targets,
            vec![a, z],
            "no-id delete should prompt in deterministic session-id order"
        );
    }

    #[test]
    fn dispatch_rejects_rm_alias() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join(".claude/projects/p/11111111-2222-3333-4444-555555555555.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"timestamp":"2026-07-14T02:16:00Z"}"#).unwrap();
        let ids = vec!["1111".to_string()];

        let err = dispatch(AgentKind::Claude, dir.path(), "rm", &ids, false, true)
            .expect_err("session rm should be rejected");

        assert!(err.to_string().contains("unknown session action: rm"));
        assert!(
            path.exists(),
            "rejected session rm must not delete transcripts"
        );
    }

    #[test]
    fn dispatch_rejects_bad_usage() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let err = |action: &str, ids: &[&str], all: bool, yes: bool| -> String {
            let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
            dispatch(AgentKind::Claude, home, action, &ids, all, yes)
                .unwrap_err()
                .to_string()
        };

        assert!(err("frobnicate", &[], false, false).contains("unknown session action"));
        assert!(err("list", &["3f2a"], false, false).contains("does not accept ids"));
        assert!(err("list", &[], false, true).contains("does not use -y"));
        assert!(err("list", &[], true, false).contains("does not use --all"));
        assert!(err("get", &[], false, false).contains("need a session id"));
        assert!(err("get", &["a", "b"], false, false).contains("accepts exactly one id"));
        assert!(err("get", &[], false, true).contains("does not use -y"));
        assert!(err("get", &["a"], true, false).contains("does not use --all"));
    }

    #[test]
    fn resolve_exact_id_wins_over_prefix_ambiguity() {
        // An id that happens to prefix another id must still be addressable:
        // the exact match wins instead of reading as an ambiguous prefix.
        let dir = tempfile::tempdir().unwrap();
        let exact = write_session(dir.path(), "1111");
        write_session(dir.path(), "11112222");

        let got = resolve(&TestBackend, dir.path(), "1111").unwrap();

        assert_eq!(got, exact);
    }

    #[test]
    fn resolve_duplicate_exact_ids_is_ambiguous() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("sessions/a/11111111.jsonl");
        let second = dir.path().join("sessions/b/11111111.jsonl");
        std::fs::create_dir_all(first.parent().unwrap()).unwrap();
        std::fs::create_dir_all(second.parent().unwrap()).unwrap();
        std::fs::write(&first, "{}\n").unwrap();
        std::fs::write(&second, "{}\n").unwrap();

        let err = resolve(&TestBackend, dir.path(), "11111111")
            .unwrap_err()
            .to_string();

        assert!(err.contains("ambiguous id '11111111' matches 2 sessions"));
        assert!(err.contains(&first.display().to_string()));
        assert!(err.contains(&second.display().to_string()));
    }

    #[test]
    fn resolve_ambiguous_prefix_lists_all_candidates() {
        let dir = tempfile::tempdir().unwrap();
        write_session(dir.path(), "11112222");
        write_session(dir.path(), "11113333");

        let err = resolve(&TestBackend, dir.path(), "1111")
            .unwrap_err()
            .to_string();

        assert!(err.contains("ambiguous id '1111' matches 2 sessions"));
        assert!(err.contains("11112222"));
        assert!(err.contains("11113333"));
    }

    #[test]
    fn delete_resolves_all_ids_before_removing_anything() {
        let dir = tempfile::tempdir().unwrap();
        let keep = write_session(dir.path(), "11111111");

        let err = delete(
            &TestBackend,
            dir.path(),
            &["1111".to_string(), "missing".to_string()],
            false,
            true,
        )
        .unwrap_err();

        assert!(err.to_string().contains("no session matches: missing"));
        assert!(keep.exists());
    }
}
