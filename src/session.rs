//! Browsing saved chat transcripts straight from the profile home — no container,
//! no relay. The `session` surface (`list` / `get` / `delete`) is shared, with the
//! per-agent on-disk format behind [`SessionBackend`].
//!
//! [`serde_json`] parses each JSONL line, so string decoding (UTF-8, `\uXXXX`,
//! surrogate pairs) falls out for free. The two agents differ only in *where* the
//! fields live; that difference is the two agent-specific backend modules.
//! Everything below — file discovery glue, id-prefix resolution, newest-first
//! listing, and delete confirmation — is shared.

use crate::agent::AgentKind;
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

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
    pub files: Vec<PathBuf>,
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
    let p = entry.path();
    entry.file_type().is_file()
        && p.extension().is_some_and(|e| e == "jsonl")
        && p.file_name().and_then(|n| n.to_str()).is_some_and(keep)
}

/// Collect every `.jsonl` transcript under `base` (recursively), keeping only
/// those whose file name passes `keep`. Empty if `base` isn't a directory. Shared
/// by both backends' `files()`; they differ only in the base dir and the filter
/// (Claude keeps all, Codex keeps `rollout-` names).
pub(crate) fn walk_jsonl(base: &Path, keep: impl Fn(&str) -> bool) -> Result<Vec<PathBuf>> {
    match std::fs::symlink_metadata(base) {
        Ok(meta) if meta.file_type().is_dir() => {}
        Ok(_) => bail!("session path is not a directory: {}", base.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(e).with_context(|| format!("inspect session directory {}", base.display()));
        }
    }
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(base) {
        let entry = entry.with_context(|| format!("walk session directory {}", base.display()))?;
        if is_wanted_transcript(&entry, &keep) {
            out.push(entry.path().to_path_buf());
        }
    }
    Ok(out)
}

pub(crate) fn walk_jsonl_tolerant(
    base: &Path,
    keep: impl Fn(&str) -> bool,
) -> Result<SessionDiscovery> {
    match std::fs::symlink_metadata(base) {
        Ok(meta) if meta.file_type().is_dir() => {}
        Ok(_) => bail!("session path is not a directory: {}", base.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SessionDiscovery::from_files(Vec::new()));
        }
        Err(e) => {
            return Err(e).with_context(|| format!("inspect session directory {}", base.display()));
        }
    }
    let mut out = SessionDiscovery::from_files(Vec::new());
    for entry in walkdir::WalkDir::new(base) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                out.errors
                    .push(format!("walk session directory {}: {e}", base.display()));
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
/// line to `f` (unparseable lines are skipped, matching the old
/// collect-then-filter behavior). Open and read failures are returned to the
/// caller instead of being misreported as an empty session.
///
/// Streaming on purpose: a profile's transcripts can run to hundreds of MB and
/// `list` visits every one, so no whole file — nor its parsed lines — is ever
/// held in memory at once.
pub(crate) fn for_each_json_line(path: &Path, mut f: impl FnMut(&Value)) -> Result<()> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("open session transcript {}", path.display()))?;
    let mut reader = io::BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(()),
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("read session transcript {}", path.display()));
            }
            Ok(_) => {}
        }
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            f(&v);
        }
    }
}

/// A line's top-level `timestamp` as a string (empty if absent). The one field
/// both formats surface identically; folded here so neither backend repeats the
/// `get("timestamp").and_then(as_str).unwrap_or("")` dance.
pub(crate) fn ts_of(v: &Value) -> String {
    v.get("timestamp")
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
    /// The agent-generated title (Claude) or first typed prompt (both), or empty
    /// when the session has neither (a tool/injected-only shell).
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
/// [`summarize`](Self::summarize) / [`prompts`](Self::prompts)) are written
/// once here as provided methods, so the two backends can't drift out of sync.
pub trait SessionBackend {
    /// Path components of the transcript tree beneath the profile home
    /// (e.g. `[".claude", "projects"]`), resolved only through real directory
    /// entries — see [`checked_session_dir`].
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

    /// The session start timestamp from one parsed line; the first `Some` wins
    /// and stops the lookup. Claude answers for any line bearing a top-level
    /// `timestamp`; Codex answers for the first `session_meta` timestamp.
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
    fn summarize(&self, path: &Path) -> Result<SessionSummary> {
        let mut start_ts: Option<String> = None;
        let mut fallback_start_ts: Option<String> = None;
        let mut first_typed: Option<String> = None;
        let mut title: Option<String> = None;
        for_each_json_line(path, |v| {
            if start_ts.is_none() {
                start_ts = self.start_ts_of(v);
            }
            if fallback_start_ts.is_none() {
                fallback_start_ts = self.fallback_start_ts_of(v);
            }
            if first_typed.is_none() {
                first_typed = self.typed_text(v);
            }
            if let Some(t) = self.title_of(v) {
                if !t.is_empty() {
                    title = Some(t);
                }
            }
        })?;
        Ok(SessionSummary {
            id: self.id_of(path),
            start_ts: start_ts.or(fallback_start_ts).unwrap_or_default(),
            title: title.or(first_typed).unwrap_or_default(),
        })
    }

    /// Every typed prompt in one transcript, in order, for `get`. Shared
    /// streaming loop; the per-line text (and wrapper filtering) is
    /// [`typed_text`](Self::typed_text).
    fn prompts(&self, path: &Path) -> Result<Vec<Prompt>> {
        let mut out = Vec::new();
        for_each_json_line(path, |v| {
            if let Some(text) = self.typed_text(v) {
                out.push(Prompt {
                    timestamp: ts_of(v),
                    text,
                });
            }
        })?;
        Ok(out)
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

/// `session` dispatch: `list` (default), `get <id>`, `delete [id...]`.
pub fn dispatch(
    agent: AgentKind,
    home: &Path,
    action: &str,
    ids: &[String],
    yes: bool,
) -> Result<i32> {
    let backend = backend_for(agent);
    match action {
        "list" => {
            reject_yes("list", yes)?;
            if !ids.is_empty() {
                bail!("session list does not accept ids");
            }
            list(backend.as_ref(), home)
        }
        "get" => {
            reject_yes("get", yes)?;
            match ids {
                [id] => get(backend.as_ref(), home, id),
                [] => bail!("need a session id (or unique prefix)"),
                _ => bail!("session get accepts exactly one id"),
            }
        }
        "delete" | "rm" => delete(backend.as_ref(), home, ids, yes),
        other => bail!("unknown session action: {other} (use list|get|delete)"),
    }
}

fn reject_yes(action: &str, yes: bool) -> Result<()> {
    if yes {
        bail!("session {action} does not use -y/--yes");
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
    for f in files {
        let id = backend.id_of(f);
        if id == query {
            exact_matches.push(f.clone());
        } else if id.starts_with(query) {
            prefix_matches.push(f.clone());
        }
    }
    let matches = if exact_matches.is_empty() {
        prefix_matches
    } else {
        exact_matches
    };
    match matches.len() {
        0 => bail!("no session matches: {query}"),
        1 => Ok(matches.into_iter().next().unwrap()),
        n => {
            let mut msg = format!("ambiguous id '{query}' matches {n} sessions:");
            for m in &matches {
                msg.push_str(&format!("\n     {}  {}", backend.id_of(m), m.display()));
            }
            bail!(msg)
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
    let mut rows: Vec<(String, String, String)> = Vec::new();
    let discovery = backend.list_files(home)?;
    let mut failed = !discovery.errors.is_empty();
    for e in discovery.errors {
        eprintln!("!! {e}");
    }
    for f in discovery.files {
        match backend.summarize(&f) {
            Ok(s) => {
                let title = list_title(&s.title);
                rows.push((s.start_ts, s.id, title));
            }
            Err(e) => {
                eprintln!("!! {}: {e:#}", f.display());
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
    rows.sort_by(|a, b| b.0.cmp(&a.0));

    for (ts, id, title) in rows {
        // By chars, not bytes: ids come from arbitrary transcript file names,
        // and a byte slice could split a multi-byte char and panic.
        let short: String = id.chars().take(8).collect();
        let disp = fmt_ts(&ts);
        if !print(&format!("{short:<8}  {disp:<16}  {title}"))? {
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
    eprintln!(">> session {sid}");
    let prompts = backend.prompts(&path)?;
    if prompts.is_empty() {
        print("(no typed prompts in this session)")?;
        return Ok(0);
    }
    for (i, p) in prompts.iter().enumerate() {
        let d = fmt_ts(&p.timestamp);
        if !print(&format!("\n[{}] {d}\n{}", i + 1, p.text))? {
            break; // reader hung up; nothing left to show
        }
    }
    Ok(0)
}

/// Delete transcripts, asking once per target unless `yes` is set. Passing no
/// ids selects every transcript for this profile.
fn delete(backend: &dyn SessionBackend, home: &Path, ids: &[String], yes: bool) -> Result<i32> {
    let targets = delete_targets(backend, home, ids)?;
    if targets.is_empty() {
        eprintln!(">> no sessions in this profile");
        return Ok(0);
    }

    let stdin = io::stdin();
    let mut input = stdin.lock();
    delete_targets_with_input(backend, targets, yes, &mut input)
}

fn delete_targets(
    backend: &dyn SessionBackend,
    home: &Path,
    ids: &[String],
) -> Result<Vec<PathBuf>> {
    if ids.is_empty() {
        // Every transcript, matching `list` (which now shows them all). No-id
        // delete clears the whole profile, tool/injected-only shells included.
        let mut targets = backend.files(home)?;
        targets.sort_by_key(|p| backend.id_of(p));
        return Ok(targets);
    }

    // Walk the transcript tree once, then resolve every id against that one
    // snapshot — `delete a b c` used to re-walk per id.
    let files = backend.files(home)?;
    let mut targets = Vec::new();
    for id in ids {
        let path = resolve_in(backend, &files, id)?;
        if !targets.iter().any(|existing| existing == &path) {
            targets.push(path);
        }
    }
    Ok(targets)
}

fn delete_targets_with_input(
    backend: &dyn SessionBackend,
    targets: Vec<PathBuf>,
    yes: bool,
    input: &mut dyn BufRead,
) -> Result<i32> {
    for path in targets {
        let sid = backend.id_of(&path);
        let delete = yes || confirm_delete(&sid, input);
        if delete {
            std::fs::remove_file(&path).with_context(|| format!("delete {}", path.display()))?;
            eprintln!(">> deleted {sid}");
        } else {
            eprintln!(">> kept {sid}");
        }
    }
    Ok(0)
}

fn confirm_delete(sid: &str, input: &mut dyn BufRead) -> bool {
    eprint!("delete session {sid}? [y/N] ");
    io::stderr().flush().ok();
    let mut ans = String::new();
    input.read_line(&mut ans).ok();
    matches!(ans.trim().to_lowercase().as_str(), "y" | "yes")
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
    format!("{date} {time}").trim_end().to_string()
}

/// Collapse runs of control characters and non-plain-space whitespace to a
/// single space (titles are one-liners in the listing). Keep ordinary spaces as
/// authored so readable prompt snippets do not get over-normalized.
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for c in s.chars() {
        if c.is_control() || (c.is_whitespace() && c != ' ') {
            if !in_run {
                out.push(' ');
                in_run = true;
            }
        } else {
            out.push(c);
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

    /// A transcript that opens but fails mid-read — invalid UTF-8 from a
    /// truncated multi-byte write, an interrupted flush, or on-disk corruption —
    /// makes `BufRead::read_line` fail with `InvalidData`. That is a distinct arm
    /// from the missing-file open error and from a merely unparseable-but-UTF-8
    /// line, which is silently skipped. The contract: `list` reports it and
    /// returns non-zero instead of a blank row, and the read paths fail fast
    /// rather than pretend the session is empty — checked through the public
    /// `get` entry point and through `prompts` underneath it (which names the
    /// file it could not read).
    #[test]
    fn non_utf8_transcript_is_reported_by_list_and_fails_the_read_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("33333333.jsonl");
        // Valid line, then a lone continuation byte: read_line errors on it.
        std::fs::write(&path, b"{\"typed\":\"ok\"}\n\xff\xfe").unwrap();
        let backend = ExplicitFilesBackend::new(vec![path.clone()]);

        // `prompts` fails fast with the read-error context, naming the file,
        // rather than reading the transcript as an empty prompt list.
        let err = backend
            .prompts(&path)
            .err()
            .expect("invalid UTF-8 must not read as an empty prompt list")
            .to_string();
        assert!(err.contains("read session transcript"), "{err}");
        assert!(err.contains("33333333.jsonl"), "{err}");

        // `get` surfaces that same failure through its own public entry point.
        let err = get_with_printer(&backend, dir.path(), "3333", |_| Ok(true))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("read session transcript"),
            "get must surface the read failure: {err}"
        );

        // `list` surfaces the failure and returns non-zero rather than a blank
        // row for a session it could not actually read.
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
        // SIGPIPE, so this must stop writing instead of panicking.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("33333333.jsonl");
        std::fs::write(
            &path,
            "{\"typed\":\"first\"}\n{\"typed\":\"second\"}\n{\"typed\":\"third\"}\n",
        )
        .unwrap();
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

        let err = delete(&backend, dir.path(), &[], true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("discovery failed"), "{err}");
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
        // A profile with no transcripts is a normal state (nothing run yet), so
        // both read and destructive paths exit 0.
        let dir = tempfile::tempdir().unwrap();
        let mut printed = Vec::new();

        let code = list_with_printer(&TestBackend, dir.path(), |line| {
            printed.push(line.to_string());
            Ok(true)
        })
        .unwrap();

        assert_eq!(code, 0, "an empty profile is not a list failure");
        assert!(printed.is_empty(), "no rows to print: {printed:?}");

        let code = delete(&TestBackend, dir.path(), &[], true).unwrap();
        assert_eq!(code, 0, "deleting nothing is not a failure");
    }

    #[test]
    fn delete_no_ids_selects_all_sessions_with_yes() {
        let dir = tempfile::tempdir().unwrap();
        let one = write_session(dir.path(), "11111111");
        let two = write_session(dir.path(), "22222222");

        delete(&TestBackend, dir.path(), &[], true).unwrap();

        assert!(!one.exists());
        assert!(!two.exists());
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

        let targets = delete_targets(&TestBackend, dir.path(), &[]).unwrap();

        assert_eq!(targets, vec![a, shell]);
    }

    #[test]
    fn summarize_empty_shell_has_empty_title() {
        // A transcript with no typed prompt still summarizes for `list`; its
        // title is empty.
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
        )
        .unwrap();
        let mut input = Cursor::new(b"y\nn\n");

        delete_targets_with_input(&TestBackend, targets, false, &mut input).unwrap();

        assert!(keep.exists());
        assert!(!remove.exists());
    }

    #[test]
    fn confirm_delete_accepts_only_explicit_yes_answers() {
        for yes in ["y\n", "Y\n", "yes\n", " YES \n"] {
            let mut input = Cursor::new(yes.as_bytes());
            assert!(confirm_delete("11111111", &mut input), "{yes:?}");
        }

        for no in ["", "\n", "n\n", "yeah\n", "yep\n", " yes please\n"] {
            let mut input = Cursor::new(no.as_bytes());
            assert!(!confirm_delete("11111111", &mut input), "{no:?}");
        }
    }

    #[test]
    fn delete_targets_dedupes_repeated_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_session(dir.path(), "11111111");

        let targets = delete_targets(
            &TestBackend,
            dir.path(),
            &["1111".to_string(), "11111111".to_string()],
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

        let targets = delete_targets(&backend, dir.path(), &[]).unwrap();

        assert_eq!(
            targets,
            vec![a, z],
            "no-id delete should prompt in deterministic session-id order"
        );
    }

    #[test]
    fn dispatch_rm_alias_deletes_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join(".claude/projects/p/11111111-2222-3333-4444-555555555555.jsonl");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"timestamp":"2026-07-14T02:16:00Z"}"#).unwrap();
        let ids = vec!["1111".to_string()];

        let code = dispatch(AgentKind::Claude, dir.path(), "rm", &ids, true).unwrap();

        assert_eq!(code, 0);
        assert!(
            !path.exists(),
            "session rm must be the same destructive action as session delete"
        );
    }

    #[test]
    fn dispatch_rejects_bad_usage() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let err = |action: &str, ids: &[&str], yes: bool| -> String {
            let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
            dispatch(AgentKind::Claude, home, action, &ids, yes)
                .unwrap_err()
                .to_string()
        };

        assert!(err("frobnicate", &[], false).contains("unknown session action"));
        assert!(err("list", &["3f2a"], false).contains("does not accept ids"));
        assert!(err("list", &[], true).contains("does not use -y"));
        assert!(err("get", &[], false).contains("need a session id"));
        assert!(err("get", &["a", "b"], false).contains("accepts exactly one id"));
        assert!(err("get", &[], true).contains("does not use -y"));
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
            true,
        )
        .unwrap_err();

        assert!(err.to_string().contains("no session matches: missing"));
        assert!(keep.exists());
    }
}
