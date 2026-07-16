//! Env-file parsing and the `base` + relay merge.
//!
//! Read one or more `docker --env-file` format files in order, drop comments and
//! blank lines, and keep `KEY=VALUE` lines with **last value winning** per key
//! while **preserving first-seen order**. A later file (the relay) thus overrides
//! an earlier one (`base`), and a `KEY=` line with an empty value blanks a base
//! default. An [`IndexMap`] gives order-plus-override directly.

use indexmap::IndexMap;

/// A merged set of `KEY=VALUE` env lines, order-preserving. Stored values are
/// the full original lines (so `KEY=` stays `KEY=`).
pub struct MergedEnv {
    /// key -> full `KEY=VALUE` line, in first-seen order.
    entries: IndexMap<String, String>,
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
                let key = match s.find('=') {
                    Some(eq) => &s[..eq],
                    None => s,
                };
                entries.insert(key.to_string(), s.to_string());
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
    /// read specific keys (CODEX_BASE_URL, …) out of the merge.
    pub fn get(&self, key: &str) -> Option<&str> {
        let line = self.entries.get(key)?;
        match line.find('=') {
            Some(eq) => Some(&line[eq + 1..]),
            None => Some(""),
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
        assert_eq!(m.get("A"), Some("1"));
        assert_eq!(m.get("B"), Some("3")); // relay wins
        assert_eq!(m.get("C"), Some("4"));
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
        assert_eq!(m.get("A"), Some("")); // blanked
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
        assert_eq!(m.get("KEY"), Some("val"));
    }
}
