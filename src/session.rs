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
use crate::print_line;
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
        let p = entry.path();
        // Do not follow a transcript-shaped symlink created inside the mounted
        // profile home. Host-side session browsing must stay inside the
        // container's transcript tree rather than becoming a path out of the
        // sandbox boundary.
        if entry.file_type().is_file()
            && p.extension().is_some_and(|e| e == "jsonl")
            && p.file_name().and_then(|n| n.to_str()).is_some_and(&keep)
        {
            out.push(p.to_path_buf());
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
        let p = entry.path();
        if entry.file_type().is_file()
            && p.extension().is_some_and(|e| e == "jsonl")
            && p.file_name().and_then(|n| n.to_str()).is_some_and(&keep)
        {
            out.files.push(p.to_path_buf());
        }
    }
    Ok(out)
}

/// Read a transcript line by line, parsing each as JSON and feeding it to `f`
/// along with its index among the *parsed* lines (unparseable lines are
/// skipped, matching the old collect-then-filter behavior). Open and read
/// failures are returned to the caller instead of being misreported as an empty
/// session.
///
/// Streaming on purpose: a profile's transcripts can run to hundreds of MB and
/// `list` visits every one, so no whole file — nor its parsed lines — is ever
/// held in memory at once.
pub(crate) fn for_each_json_line(path: &Path, mut f: impl FnMut(usize, &Value)) -> Result<()> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("open session transcript {}", path.display()))?;
    let mut reader = io::BufReader::new(file);
    let mut line = String::new();
    let mut idx = 0usize;
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
            f(idx, &v);
            idx += 1;
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
/// (`session_claude::Claude`, `session_codex::Codex`) diverge only in the four
/// required methods below — *where*
/// each field lives on a line and which lines count as a real prompt. The two
/// summary/get loops that consume those answers ([`summarize`](Self::summarize) /
/// [`prompts`](Self::prompts)) are written once here as provided methods, so the
/// two backends can't drift out of sync.
pub trait SessionBackend {
    /// All transcript files under this profile home (empty if none yet).
    fn files(&self, home: &Path) -> Result<Vec<PathBuf>>;

    /// Transcript files for `session list`. Backends can override this with a
    /// tolerant walk so one bad child path does not hide every readable session.
    fn list_files(&self, home: &Path) -> Result<SessionDiscovery> {
        self.files(home).map(SessionDiscovery::from_files)
    }

    /// The session id for a transcript path.
    fn id_of(&self, path: &Path) -> String;

    /// `Some(text)` iff `v` is a prompt the user actually typed — with any
    /// injected/wrapper turns already filtered out. `None` for every other line.
    /// This is the heart of the divergence: Claude keys off `promptSource:typed`,
    /// Codex off a wrapper-filtered `response_item` user message.
    fn typed_text(&self, v: &Value) -> Option<String>;

    /// The session start timestamp from one parsed line (fed in order with its
    /// index); the first `Some` wins and stops the lookup. Claude answers for
    /// any line bearing a top-level `timestamp`; Codex answers for the first
    /// `session_meta` timestamp.
    fn start_ts_of(&self, idx: usize, v: &Value) -> Option<String>;

    /// Lower-confidence timestamp candidate used only when
    /// [`start_ts_of`](Self::start_ts_of) never finds one.
    fn fallback_start_ts_of(&self, _idx: usize, _v: &Value) -> Option<String> {
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
        for_each_json_line(path, |idx, v| {
            if start_ts.is_none() {
                start_ts = self.start_ts_of(idx, v);
            }
            if fallback_start_ts.is_none() {
                fallback_start_ts = self.fallback_start_ts_of(idx, v);
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
        for_each_json_line(path, |_idx, v| {
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
    if query.is_empty() {
        bail!("need a session id (or unique prefix)");
    }
    let mut exact_matches: Vec<PathBuf> = Vec::new();
    let mut prefix_matches: Vec<PathBuf> = Vec::new();
    for f in backend.files(home)? {
        let id = backend.id_of(&f);
        if id == query {
            exact_matches.push(f);
        } else if id.starts_with(query) {
            prefix_matches.push(f);
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

/// List this profile's sessions, newest first: `shortid  date  title`. Every
/// transcript lists (tool/injected-only shells show an empty title) so nothing is
/// hidden from `list` or no-id `delete`. Columns are `%-8s  %-16s  %s`.
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
                // Titles can contain newlines/tabs; collapse them to single spaces.
                let title = collapse_ws(&s.title);
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
    let path = resolve(backend, home, id)?;
    let sid = backend.id_of(&path);
    eprintln!(">> session {sid}");
    let prompts = backend.prompts(&path)?;
    if prompts.is_empty() {
        print_line("(no typed prompts in this session)")?;
        return Ok(0);
    }
    for (i, p) in prompts.iter().enumerate() {
        let d = fmt_ts(&p.timestamp);
        if !print_line(&format!("\n[{}] {d}\n{}", i + 1, p.text))? {
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

    let mut targets = Vec::new();
    for id in ids {
        let path = resolve(backend, home, id)?;
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
            std::fs::remove_file(&path)
                .map_err(|e| anyhow::anyhow!("delete {}: {e}", path.display()))?;
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

/// Collapse runs of newlines/tabs to a single space (titles are one-liners in the
/// listing).
fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for c in s.chars() {
        if c == '\n' || c == '\t' {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::io::Cursor;

    struct TestBackend;

    impl SessionBackend for TestBackend {
        fn files(&self, home: &Path) -> Result<Vec<PathBuf>> {
            let Some(base) = checked_session_dir(home, &["sessions"])? else {
                return Ok(Vec::new());
            };
            walk_jsonl(&base, |_| true)
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

        fn start_ts_of(&self, _idx: usize, _v: &Value) -> Option<String> {
            None
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

        fn start_ts_of(&self, _idx: usize, _v: &Value) -> Option<String> {
            None
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
        assert_eq!(collapse_ws("plain"), "plain");
    }

    #[test]
    fn transcript_read_errors_are_not_reported_as_empty_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("sessions").join("missing.jsonl");

        let err = TestBackend
            .prompts(&missing)
            .err()
            .expect("missing transcript should fail")
            .to_string();

        assert!(err.contains("open session transcript"));
        assert!(err.contains("missing.jsonl"));
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
        let outside = dir.path().join("outside.jsonl");
        std::fs::write(&outside, "{}\n").unwrap();
        let sessions = dir.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        symlink(&outside, sessions.join("linked.jsonl")).unwrap();

        let files = TestBackend.files(dir.path()).unwrap();

        assert!(
            files.is_empty(),
            "host-side browsing must not follow symlinks"
        );
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
