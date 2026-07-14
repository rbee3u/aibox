//! Browsing saved chat transcripts straight from the profile home — no container,
//! no relay. Ports the two scripts' shared `session` block (`list` / `get` /
//! `delete`), with the per-agent on-disk format behind [`SessionBackend`].
//!
//! ## The big win of the rewrite
//!
//! The Bash carried ~130 lines of hand-written awk to decode JSON strings —
//! UTF-8, `\uXXXX`, and surrogate pairs — purely because macOS ships BSD awk with
//! no `strtonum`/bit-ops. Here [`serde_json`] parses each JSONL line and every
//! escape falls out for free. The two agents differ only in *where* the fields
//! live; that difference is the two [`SessionBackend`] impls ([`claude`],
//! [`codex`]). Everything below — file discovery glue, id-prefix resolution,
//! newest-first listing, and delete-with-confirm — is shared, exactly as the Bash
//! kept the block byte-for-byte parallel between the two scripts.

pub mod claude;
pub mod codex;

use crate::agent::AgentKind;
use anyhow::{bail, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

/// One session's list-row data. A session with no typed prompts (the user never
/// asked anything) yields `None` from [`SessionBackend::summarize`] and is
/// dropped from the listing, matching the Bash `typed > 0` guard.
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
/// [`codex::Codex`]) are the only place session handling diverges.
pub trait SessionBackend {
    /// All transcript files under this profile home (empty if none yet).
    fn files(&self, home: &Path) -> Vec<PathBuf>;

    /// The session id for a transcript path.
    fn id_of(&self, path: &Path) -> String;

    /// Summarize one transcript for `list`. `None` = no typed prompts, so the
    /// session is skipped.
    fn summarize(&self, path: &Path) -> Option<SessionSummary>;

    /// Every typed prompt in one transcript, in order, for `get`.
    fn prompts(&self, path: &Path) -> Vec<Prompt>;
}

/// Resolve `AgentKind` to its backend. The one bridge between the enum and the
/// session trait objects.
pub fn backend_for(agent: AgentKind) -> Box<dyn SessionBackend> {
    match agent {
        AgentKind::Claude => Box::new(claude::Claude),
        AgentKind::Codex => Box::new(codex::Codex),
    }
}

/// `session` dispatch: `list` (default), `get <id>`, `delete <id>`. Mirrors the
/// Bash `session_dispatch`.
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
/// the candidates), matching the Bash `session_resolve`.
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
/// with no typed prompts are skipped. Ports the Bash `session_list` (including the
/// `%-8s  %-16s  %s` columns and the newest-first sort).
fn list(backend: &dyn SessionBackend, home: &Path) -> Result<i32> {
    // Collect (start_ts, id, title) for every session with ≥1 typed prompt.
    let mut rows: Vec<(String, String, String)> = Vec::new();
    for f in backend.files(home) {
        if let Some(s) = backend.summarize(&f) {
            // Titles can contain newlines/tabs; collapse to spaces like the Bash
            // `gsub(/[\n\t]+/," ",ttl)`.
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
/// (for copy-paste). Ports the Bash `session_get`.
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
        // Matches the Bash `printf "\n[%d] %s\n%s\n"`.
        println!("\n[{}] {d}\n{}", i + 1, p.text);
    }
    Ok(0)
}

/// Delete one transcript, asking first (not reversible). Ports `session_delete`.
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
/// the timestamp is empty. Mirrors the Bash `substr(ts,1,10) " " substr(ts,12,5)`
/// (positions, not real date parsing — the stored value is already ISO-8601).
fn fmt_ts(ts: &str) -> String {
    if ts.is_empty() {
        return String::new();
    }
    let date: String = ts.chars().take(10).collect();
    let time: String = ts.chars().skip(11).take(5).collect();
    format!("{date} {time}").trim_end().to_string()
}

/// Collapse runs of newlines/tabs to a single space (titles are one-liners in the
/// listing). Matches the Bash `gsub(/[\n\t]+/," ",…)`.
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
