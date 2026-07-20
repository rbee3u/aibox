//! Browsing saved chat transcripts straight from the profile home — no container,
//! no relay. The `session` surface (`list` / `get` / `delete`) is shared, with the
//! per-agent on-disk format behind [`SessionBackend`].
//!
//! [`serde_json`] parses each JSONL line, so string decoding (UTF-8, `\uXXXX`,
//! surrogate pairs) falls out for free. The two agents differ only in *where* the
//! fields live; that difference is the two [`SessionBackend`] impls ([`claude`],
//! [`codex`]). Everything below — file discovery glue, id-prefix resolution,
//! newest-first listing, and delete confirmation — is shared.

pub mod claude;
pub mod codex;

use crate::agent::AgentKind;
use anyhow::{bail, Result};
use serde_json::Value;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

/// Collect every `.jsonl` transcript under `base` (recursively), keeping only
/// those whose file name passes `keep`. Empty if `base` isn't a directory. Shared
/// by both backends' `files()`; they differ only in the base dir and the filter
/// (Claude keeps all, Codex keeps `rollout-` names).
pub(crate) fn walk_jsonl(base: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    if !base.is_dir() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(base).into_iter().flatten() {
        let p = entry.path();
        if p.is_file()
            && p.extension().is_some_and(|e| e == "jsonl")
            && p.file_name().and_then(|n| n.to_str()).is_some_and(&keep)
        {
            out.push(p.to_path_buf());
        }
    }
    out
}

/// Read a transcript and parse each line as JSON, skipping unparseable lines.
/// Empty if the file can't be read. Shared by both backends' `summarize`/`prompts`,
/// which then extract their agent-specific fields from the parsed values.
pub(crate) fn json_lines(path: &Path) -> Vec<Value> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
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

/// The per-agent on-disk transcript format. The two impls ([`claude::Claude`],
/// [`codex::Codex`]) diverge only in the four required methods below — *where*
/// each field lives on a line and which lines count as a real prompt. The two
/// summary/get loops that consume those answers ([`summarize`](Self::summarize) /
/// [`prompts`](Self::prompts)) are written once here as provided methods, so the
/// two backends can't drift out of sync.
pub trait SessionBackend {
    /// All transcript files under this profile home (empty if none yet).
    fn files(&self, home: &Path) -> Vec<PathBuf>;

    /// The session id for a transcript path.
    fn id_of(&self, path: &Path) -> String;

    /// `Some(text)` iff `v` is a prompt the user actually typed — with any
    /// injected/wrapper turns already filtered out. `None` for every other line.
    /// This is the heart of the divergence: Claude keys off `promptSource:typed`,
    /// Codex off a wrapper-filtered `response_item` user message.
    fn typed_text(&self, v: &Value) -> Option<String>;

    /// The session start timestamp, given every parsed line. Claude takes the
    /// first line bearing one; Codex takes line 0's (the `session_meta`).
    fn start_ts(&self, lines: &[Value]) -> String;

    /// The `list` row title, given every parsed line and the first typed prompt.
    /// Default is just the first prompt; Claude overrides to prefer an `ai-title`.
    fn title(&self, _lines: &[Value], first_prompt: &str) -> String {
        first_prompt.to_string()
    }

    /// Summarize one transcript for `list`. Every transcript summarizes — a
    /// session with no typed prompt just gets an empty title (unless a backend's
    /// `title` finds something else, like Claude's `ai-title`), so tool/injected-
    /// only shells still list and can be cleared. Shared loop over both formats;
    /// the per-agent answers come from the required methods above.
    fn summarize(&self, path: &Path) -> SessionSummary {
        let lines = json_lines(path);
        let first_typed = lines
            .iter()
            .find_map(|v| self.typed_text(v))
            .unwrap_or_default();
        SessionSummary {
            id: self.id_of(path),
            start_ts: self.start_ts(&lines),
            title: self.title(&lines, &first_typed),
        }
    }

    /// Every typed prompt in one transcript, in order, for `get`. Shared loop; the
    /// per-line text (and wrapper filtering) is [`typed_text`](Self::typed_text).
    fn prompts(&self, path: &Path) -> Vec<Prompt> {
        json_lines(path)
            .into_iter()
            .filter_map(|v| {
                self.typed_text(&v).map(|text| Prompt {
                    timestamp: ts_of(&v),
                    text,
                })
            })
            .collect()
    }
}

/// Resolve `AgentKind` to its backend. The one bridge between the enum and the
/// session trait objects.
pub fn backend_for(agent: AgentKind) -> Box<dyn SessionBackend> {
    match agent {
        AgentKind::Claude => Box::new(claude::Claude),
        AgentKind::Codex => Box::new(codex::Codex),
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

/// Resolve a full id or unique prefix to exactly one transcript path. Zero
/// matches or an ambiguous prefix fail with a message (the ambiguous case lists
/// the candidates).
fn resolve(backend: &dyn SessionBackend, home: &Path, query: &str) -> Result<PathBuf> {
    if query.is_empty() {
        bail!("need a session id (or unique prefix)");
    }
    let mut matches: Vec<PathBuf> = Vec::new();
    for f in backend.files(home) {
        if backend.id_of(&f).starts_with(query) {
            matches.push(f);
        }
    }
    match matches.len() {
        0 => bail!("no session matches: {query}"),
        1 => Ok(matches.into_iter().next().unwrap()),
        n => {
            let mut msg = format!("ambiguous id '{query}' matches {n} sessions:");
            for m in &matches {
                msg.push_str(&format!("\n     {}", backend.id_of(m)));
            }
            bail!(msg)
        }
    }
}

/// List this profile's sessions, newest first: `shortid  date  title`. Every
/// transcript lists (tool/injected-only shells show an empty title) so nothing is
/// hidden from `list` or no-id `delete`. Columns are `%-8s  %-16s  %s`.
fn list(backend: &dyn SessionBackend, home: &Path) -> Result<i32> {
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for f in backend.files(home) {
        let s = backend.summarize(&f);
        // Titles can contain newlines/tabs; collapse them to single spaces.
        let title = collapse_ws(&s.title);
        rows.push((s.start_ts, s.id, title));
    }
    if rows.is_empty() {
        eprintln!(">> no sessions in this profile");
        return Ok(0);
    }
    // Newest first: ISO-8601 sorts lexically, so a plain string sort works.
    rows.sort_by(|a, b| b.0.cmp(&a.0));

    for (ts, id, title) in rows {
        // By chars, not bytes: ids come from arbitrary transcript file names,
        // and a byte slice could split a multi-byte char and panic.
        let short: String = id.chars().take(8).collect();
        let disp = fmt_ts(&ts);
        println!("{short:<8}  {disp:<16}  {title}");
    }
    Ok(0)
}

/// Print your typed prompts from one session, numbered + timestamped, full text
/// (for copy-paste).
fn get(backend: &dyn SessionBackend, home: &Path, id: &str) -> Result<i32> {
    let path = resolve(backend, home, id)?;
    let sid = backend.id_of(&path);
    eprintln!(">> session {sid}");
    let prompts = backend.prompts(&path);
    if prompts.is_empty() {
        println!("(no typed prompts in this session)");
        return Ok(0);
    }
    for (i, p) in prompts.iter().enumerate() {
        let d = fmt_ts(&p.timestamp);
        println!("\n[{}] {d}\n{}", i + 1, p.text);
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
        let mut targets = backend.files(home);
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
        fn files(&self, home: &Path) -> Vec<PathBuf> {
            walk_jsonl(&home.join("sessions"), |_| true)
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

        fn start_ts(&self, _lines: &[Value]) -> String {
            String::new()
        }
    }

    fn write_session(home: &Path, id: &str) -> PathBuf {
        let path = home.join("sessions").join(format!("{id}.jsonl"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{}\n").unwrap();
        path
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

        let s = TestBackend.summarize(&shell);
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
