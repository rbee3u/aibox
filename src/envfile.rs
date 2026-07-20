//! Env-file parsing and the `base` + relay merge.
//!
//! Read one or more `docker --env-file` format files in order, drop comments and
//! blank lines, and keep `KEY=VALUE` lines with **last value winning** per key
//! while **preserving first-seen order**. A later file (the relay) thus overrides
//! an earlier one (`base`), and a `KEY=` line with an empty value blanks a base
//! default. An [`IndexMap`] gives order-plus-override directly.

use anyhow::{bail, Result};
use indexmap::IndexMap;
use std::path::Path;

/// A merged set of `KEY=VALUE` env lines, order-preserving. Stored values are
/// the full original lines (so `KEY=` stays `KEY=`).
pub struct MergedEnv {
    /// key -> full `KEY=VALUE` line, in first-seen order.
    entries: IndexMap<String, String>,
}

/// The key part of a real (non-comment) line: everything before the first `=`,
/// or the whole line for a bare `KEY` pass-through.
fn key_of(line: &str) -> &str {
    match line.find('=') {
        Some(eq) => &line[..eq],
        None => line,
    }
}

/// True for a key docker `--env-file` accepts: `[A-Za-z_][A-Za-z0-9_]*`.
fn valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Validate every real line's key in one env-file source, naming the file and
/// line on failure. Catches the classic `KEY = value` / `export KEY=value`
/// edits *before* the run: left unchecked they surface as a misleading
/// "missing required keys" (Codex reads the merge itself) or as a docker
/// `--env-file` parse error pointing at a staged temp file that is already
/// deleted by the time anyone could look at it.
pub fn check_keys(source: &Path, src: &str) -> Result<()> {
    for (i, raw) in src.lines().enumerate() {
        let s = raw.trim_start();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        if !valid_key(key_of(s)) {
            bail!(
                "{} line {}: {:?} is not a valid KEY=VALUE line \
                 (keys match [A-Za-z_][A-Za-z0-9_]*; no spaces around '=', no `export`)",
                source.display(),
                i + 1,
                raw
            );
        }
    }
    Ok(())
}

impl MergedEnv {
    /// Merge the given file contents in order. Later contents override earlier
    /// per key; first-seen order is preserved. Comments (`#…`) and blank lines
    /// are dropped. Leading whitespace on a line is trimmed before parsing.
    pub fn merge(sources: &[String]) -> Self {
        let mut entries: IndexMap<String, String> = IndexMap::new();
        for src in sources {
            for raw in src.lines() {
                let s = raw.trim_start();
                if s.is_empty() || s.starts_with('#') {
                    continue;
                }
                // A line with no '=' is treated as a bare key, stored as-is.
                entries.insert(key_of(s).to_string(), s.to_string());
            }
        }
        MergedEnv { entries }
    }

    /// The merged lines in order, each a full `KEY=VALUE` string.
    fn lines(&self) -> impl Iterator<Item = &str> {
        self.entries.values().map(|s| s.as_str())
    }

    /// Render as an env-file body (one `KEY=VALUE` per line, trailing newline if
    /// non-empty). This is what gets written to the 0600 temp file Docker reads.
    pub fn to_env_file(&self) -> String {
        let mut out = String::new();
        for line in self.lines() {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// Look up the value part (after `=`) of a key, if present. Used by Codex to
    /// read specific keys (CODEX_BASE_URL, …) out of the merge, and by Claude's
    /// endpoint warning.
    ///
    /// A bare `KEY` line (no `=`) passes the host's value through — docker
    /// `--env-file` semantics — so it resolves to `$KEY` from the wrapper's
    /// environment (empty if unset). That keeps required-key checks and
    /// warnings consistent with what the container will actually see.
    pub fn get(&self, key: &str) -> Option<String> {
        let line = self.entries.get(key)?;
        match line.find('=') {
            Some(eq) => Some(line[eq + 1..].to_string()),
            None => Some(std::env::var(key).unwrap_or_default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(x: &str) -> String {
        x.to_string()
    }

    #[test]
    fn later_source_overrides_earlier() {
        let base = s("A=1\nB=2\n");
        let relay = s("B=3\nC=4\n");
        let m = MergedEnv::merge(&[base, relay]);
        assert_eq!(m.get("A").as_deref(), Some("1"));
        assert_eq!(m.get("B").as_deref(), Some("3")); // relay wins
        assert_eq!(m.get("C").as_deref(), Some("4"));
    }

    #[test]
    fn preserves_first_seen_order() {
        let base = s("A=1\nB=2\n");
        let relay = s("B=3\nC=4\n");
        let m = MergedEnv::merge(&[base, relay]);
        let lines: Vec<&str> = m.lines().collect();
        // A, B, C — B keeps its original position even though relay sets it again.
        assert_eq!(lines, vec!["A=1", "B=3", "C=4"]);
    }

    #[test]
    fn empty_value_blanks_a_base_default() {
        let base = s("A=default\n");
        let relay = s("A=\n");
        let m = MergedEnv::merge(&[base, relay]);
        assert_eq!(m.get("A").as_deref(), Some("")); // blanked
    }

    #[test]
    fn bare_key_resolves_from_host_env() {
        // A bare `KEY` line passes the host value through (docker --env-file
        // semantics); `get` must agree with what the container will see.
        let m = MergedEnv::merge(&[s("AIBOX_TEST_BARE_PASSTHROUGH\n")]);
        std::env::set_var("AIBOX_TEST_BARE_PASSTHROUGH", "host-value");
        assert_eq!(
            m.get("AIBOX_TEST_BARE_PASSTHROUGH").as_deref(),
            Some("host-value")
        );
        std::env::remove_var("AIBOX_TEST_BARE_PASSTHROUGH");
        assert_eq!(m.get("AIBOX_TEST_BARE_PASSTHROUGH").as_deref(), Some(""));
        // A key that appears nowhere is still None.
        assert_eq!(m.get("AIBOX_TEST_NOT_THERE"), None);
    }

    #[test]
    fn comments_and_blanks_dropped() {
        let src = s("# c\n\n  \nA=1\n  B=2\n");
        let m = MergedEnv::merge(&[src]);
        let lines: Vec<&str> = m.lines().collect();
        assert_eq!(lines, vec!["A=1", "B=2"]);
    }

    #[test]
    fn leading_whitespace_trimmed() {
        let src = s("   KEY=val\n");
        let m = MergedEnv::merge(&[src]);
        assert_eq!(m.get("KEY").as_deref(), Some("val"));
    }

    #[test]
    fn check_keys_accepts_valid_env_files() {
        let src = "# comment\n\nKEY=value\n_UNDER=1\nBARE_PASSTHROUGH\nBLANKED=\n  INDENTED=ok\n";
        assert!(check_keys(Path::new("/p/base"), src).is_ok());
    }

    #[test]
    fn check_keys_rejects_malformed_keys_with_file_and_line() {
        // The classic editing mistakes: spaces around '=', shell `export`,
        // a missing key, and a bare non-key word.
        for (src, line_no, bad) in [
            ("GOOD=1\nCODEX_API_KEY = sk-x\n", 2, "CODEX_API_KEY = sk-x"),
            ("export KEY=v\n", 1, "export KEY=v"),
            ("=value\n", 1, "=value"),
            ("2FOO=x\n", 1, "2FOO=x"),
            ("some words\n", 1, "some words"),
        ] {
            let err = check_keys(Path::new("/p/envs/relay"), src)
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("/p/envs/relay") && err.contains(&format!("line {line_no}")),
                "error should name file and line: {err}"
            );
            assert!(err.contains(bad), "error should quote the raw line: {err}");
        }
    }
}
