//! Session catalog resolution, listing, summaries, and deletion.

use super::backend::SessionBackend;
use super::filesystem::{
    SessionDiscoverySummary, remove_session_transcript, safe_path, terminal_safe,
};
use super::model::{SessionListData, SessionListRow};
use anyhow::{Result, bail};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const UUID_TEXT_LEN: usize = 36;
const UUID_SUFFIX_LEN: usize = 12;

pub(super) fn resolve(backend: &dyn SessionBackend, home: &Path, query: &str) -> Result<PathBuf> {
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
