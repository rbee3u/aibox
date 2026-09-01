//! Transcript discovery, bounded JSONL reads, and anchored filesystem access.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};

// Transcripts stream line by line, but a container-written JSONL record still
// needs a bound before it is buffered for parsing.
pub(super) const MAX_TRANSCRIPT_LINE_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const UUID_TEXT_LEN: usize = 36;

pub(super) fn terminal_safe(value: &str) -> String {
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

pub(super) fn safe_path(path: &Path) -> String {
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
/// inside the selected Home: host-side Session access must stay beneath the
/// selected Home. Strict and tolerant walks share this predicate.
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

pub(super) fn try_for_each_json_line(
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
pub(super) fn test_transcript_home(path: &Path, components: &[&str]) -> Result<PathBuf> {
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

pub(super) fn open_session_transcript(home: &Path, path: &Path) -> Result<fs::File> {
    crate::foundation::safe_fs::open_regular_beneath(home, path, "session transcript")
}

pub(super) fn remove_session_transcript(home: &Path, path: &Path) -> Result<()> {
    crate::foundation::safe_fs::remove_regular_beneath(home, path, "session transcript")
}
