//! Browsing saved chat transcripts straight from the profile home — no container,
//! no relay. The `session` surface (`list` / `get` / `delete`) is shared, with the
//! per-agent on-disk format behind [`SessionBackend`].
//!
//! [`serde_json`] parses each JSONL line, so string decoding (UTF-8, `\uXXXX`,
//! surrogate pairs) falls out for free. The two agents differ only in *where* the
//! fields live; that difference is the two [`SessionBackend`] impls ([`claude`],
//! [`codex`]). Everything below — file discovery glue, id-prefix resolution,
//! newest-first listing, and delete-with-confirm — is shared.

pub mod claude;
pub mod codex;

use crate::agent::AgentKind;
use anyhow::{bail, Result};
use serde_json::Value;
use std::io::Write;
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

/// One session's list-row data. A session with no typed prompts (the user never
/// asked anything) yields `None` from [`SessionBackend::summarize`] and is
/// dropped from the listing (the "at least one typed prompt" guard).
pub struct SessionSummary {
    /// Full session id (the row shows the first 8 chars).
    pub id: String,
    /// Session start timestamp (ISO-8601), or empty if none was found.
    pub start_ts: String,
    /// The agent-generated title (Claude) or first typed prompt (both), or empty.
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
/// list/get loops that consume those answers ([`summarize`](Self::summarize) /
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

    /// Summarize one transcript for `list`. `None` = no typed prompts, so the
    /// session is skipped (needs at least one typed prompt). Shared loop over both
    /// formats; the per-agent answers come from the required methods above.
    fn summarize(&self, path: &Path) -> Option<SessionSummary> {
        let lines = json_lines(path);
        let mut first_typed = String::new();
        let mut typed = 0u32;
        for v in &lines {
            if let Some(t) = self.typed_text(v) {
                typed += 1;
                if first_typed.is_empty() {
                    first_typed = t;
                }
            }
        }
        if typed == 0 {
            return None;
        }
        Some(SessionSummary {
            id: self.id_of(path),
            start_ts: self.start_ts(&lines),
            title: self.title(&lines, &first_typed),
        })
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

/// `session` dispatch: `list` (default), `get <id>`, `delete <id>`.
pub fn dispatch(agent: AgentKind, home: &Path, action: &str, id: Option<&str>) -> Result<i32> {
    let backend = backend_for(agent);
    match action {
        "list" => list(backend.as_ref(), home),
        "get" => get(backend.as_ref(), home, id),
        "delete" | "rm" => delete(backend.as_ref(), home, id),
        other => bail!("unknown session action: {other} (use list|get|delete)"),
    }
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

/// List this profile's sessions, newest first: `shortid  date  title`. Sessions
/// with no typed prompts are skipped. Columns are `%-8s  %-16s  %s`.
fn list(backend: &dyn SessionBackend, home: &Path) -> Result<i32> {
    // Collect (start_ts, id, title) for every session with ≥1 typed prompt.
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for f in backend.files(home) {
        if let Some(s) = backend.summarize(&f) {
            // Titles can contain newlines/tabs; collapse them to single spaces.
            let title = collapse_ws(&s.title);
            rows.push((s.start_ts, s.id, title));
        }
    }
    if rows.is_empty() {
        eprintln!(">> no sessions in this profile");
        return Ok(0);
    }
    // Newest first: sort by the raw ISO timestamp descending (matches `sort -r`
    // on the ts-prefixed lines — ISO-8601 sorts lexically).
    rows.sort_by(|a, b| b.0.cmp(&a.0));

    for (ts, id, title) in rows {
        let short = &id[..id.len().min(8)];
        let disp = fmt_ts(&ts);
        // %-8s  %-16s  %s
        println!("{short:<8}  {disp:<16}  {title}");
    }
    Ok(0)
}

/// Print your typed prompts from one session, numbered + timestamped, full text
/// (for copy-paste).
fn get(backend: &dyn SessionBackend, home: &Path, id: Option<&str>) -> Result<i32> {
    let path = resolve(backend, home, id.unwrap_or(""))?;
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

/// Delete one transcript, asking first (not reversible).
fn delete(backend: &dyn SessionBackend, home: &Path, id: Option<&str>) -> Result<i32> {
    let path = resolve(backend, home, id.unwrap_or(""))?;
    let sid = backend.id_of(&path);
    eprint!("delete session {sid}? [y/N] ");
    std::io::stderr().flush().ok();
    let mut ans = String::new();
    std::io::stdin().read_line(&mut ans).ok();
    match ans.trim().to_lowercase().as_str() {
        "y" | "yes" => {
            std::fs::remove_file(&path)
                .map_err(|e| anyhow::anyhow!("delete {}: {e}", path.display()))?;
            eprintln!(">> deleted {sid}");
        }
        _ => eprintln!(">> kept {sid}"),
    }
    Ok(0)
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
}
