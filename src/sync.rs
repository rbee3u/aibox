//! `sync`: refresh a config file's doc/example comments to the current template
//! while keeping every real config line the user added.
//!
//! The algorithm, given the old file's contents and a freshly generated
//! template:
//!
//! 1. Collect the old file's real (uncommented `KEY=VALUE`) lines, last value
//!    winning per key, in first-seen order.
//! 2. Emit the fresh template line by line. After each `#KEY=` *example* line,
//!    if the old file had a real value for that key, drop it in directly under
//!    the example.
//! 3. Any real key with no matching example in the template is appended in a
//!    trailing "settings kept from your old file" block, so nothing is lost.
//!
//! The merge itself is a pure string transform; `run_sync` decides whether to
//! write the result or print it (`--dry-run`).

use crate::agent::TEMPLATE_VERSION;
use crate::profile::{self, Profile};
use crate::template;
use anyhow::{Context, Result};
use indexmap::IndexMap;
use std::fs;
use std::path::Path;

/// The trailing-block header, emitted once before any orphaned keys.
const ORPHAN_HEADER: &str =
    "# --- settings kept from your old file (no matching example above) ---";

/// Rewrite `old` against `template`: template docs/examples, with the user's real
/// values re-inserted under their matching examples and orphans appended.
/// Returns the new file contents without a trailing newline; `sync_one` adds one
/// when writing.
pub fn merge(old: &str, template: &str) -> String {
    // 1. Collect real KEY=VALUE lines from the old file (last wins, keep order).
    let mut vals: IndexMap<String, String> = IndexMap::new();
    for raw in old.lines() {
        let s = raw.trim_start();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        let Some(eq) = s.find('=') else { continue };
        let key = s[..eq].to_string();
        vals.insert(key, s.to_string());
    }

    // 2. Walk the template; after each `#KEY=` example, emit the matching real
    //    line if we have one (once).
    let mut out_lines: Vec<String> = Vec::new();
    let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in template.lines() {
        out_lines.push(line.to_string());
        if let Some(key) = example_key(line) {
            if !done.contains(&key) {
                if let Some(val) = vals.get(&key) {
                    out_lines.push(val.clone());
                    done.insert(key);
                }
            }
        }
    }

    // 3. Append orphans (real keys with no matching example) in a trailing block.
    let mut first = true;
    for (key, val) in &vals {
        if done.contains(key) {
            continue;
        }
        if first {
            out_lines.push(String::new());
            out_lines.push(ORPHAN_HEADER.to_string());
            first = false;
        }
        out_lines.push(val.clone());
    }

    out_lines.join("\n")
}

/// If `line` is an example line of the form `#KEY=…` (a `#` immediately followed
/// by a key that starts with a letter/underscore, then `=`), return the key.
/// The key must match `#[A-Za-z_][A-Za-z0-9_]*=`.
fn example_key(line: &str) -> Option<String> {
    let rest = line.strip_prefix('#')?;
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    for (i, c) in chars {
        if c == '=' {
            return Some(rest[..i].to_string());
        }
        if c.is_ascii_alphanumeric() || c == '_' {
            continue;
        }
        // Any other char before '=' means this isn't a KEY= example.
        return None;
    }
    None
}

/// Dispatch a `sync` invocation for `target`:
/// - `None` — base + every relay under `envs/`;
/// - `Some("base")` — just `base`;
/// - `Some(relay)` — one relay under `envs/`.
///
/// `dry_run` prints the result instead of writing.
pub fn run_sync(prof: &Profile, target: Option<&str>, dry_run: bool) -> Result<i32> {
    match target {
        None => {
            sync_one(prof, &prof.base_file, None, dry_run)?;
            if let Ok(rd) = fs::read_dir(&prof.envs_dir) {
                let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
                entries.sort();
                for path in entries {
                    if path.is_file() {
                        let name = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("")
                            .to_string();
                        sync_one(prof, &path, Some(&name), dry_run)?;
                    }
                }
            }
        }
        Some("base") => sync_one(prof, &prof.base_file, None, dry_run)?,
        Some(relay) => {
            let path = prof.envs_dir.join(relay);
            sync_one(prof, &path, Some(relay), dry_run)?;
        }
    }
    Ok(0)
}

/// Sync one file in place (or to stdout under `dry_run`). `relay_name` is `None`
/// for `base` (uses the base template) or `Some(name)` for a relay. A missing
/// file is skipped with a notice.
fn sync_one(prof: &Profile, file: &Path, relay_name: Option<&str>, dry_run: bool) -> Result<()> {
    if !file.is_file() {
        eprintln!("!! not found, skipping: {}", file.display());
        return Ok(());
    }
    let old = fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    let template = match relay_name {
        None => template::base_template(prof.agent, TEMPLATE_VERSION),
        Some(name) => template::relay_template(prof.agent, name, TEMPLATE_VERSION),
    };
    let result = merge(&old, &template);
    if dry_run {
        println!("===== {} =====\n{result}\n", file.display());
    } else {
        // Exactly one trailing newline.
        profile::write_600(file, &format!("{result}\n"))?;
        eprintln!(
            ">> synced {} -> template v{TEMPLATE_VERSION}",
            file.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_key_matches_pattern() {
        assert_eq!(example_key("#FOO=bar").as_deref(), Some("FOO"));
        assert_eq!(example_key("#FOO_BAR2=x").as_deref(), Some("FOO_BAR2"));
        assert_eq!(example_key("#_X=1").as_deref(), Some("_X"));
        // A real (uncommented) line is not an example.
        assert_eq!(example_key("FOO=bar"), None);
        // Doc comment, no '='.
        assert_eq!(example_key("# just docs"), None);
        // Leading digit is not a valid key start.
        assert_eq!(example_key("#2FOO=x"), None);
        // Space after '#' then text — not KEY=.
        assert_eq!(example_key("# FOO=bar"), None);
    }

    #[test]
    fn real_value_replaces_under_example() {
        let template = "# doc\n#FOO=example\n#BAR=example\n";
        let old = "FOO=myvalue\n";
        let got = merge(old, template);
        assert_eq!(got, "# doc\n#FOO=example\nFOO=myvalue\n#BAR=example");
    }

    #[test]
    fn orphan_key_goes_to_trailing_block() {
        let template = "#FOO=example\n";
        let old = "FOO=1\nORPHAN=keepme\n";
        let got = merge(old, template);
        assert!(got.contains("#FOO=example\nFOO=1"));
        assert!(got.contains(ORPHAN_HEADER));
        assert!(got.trim_end().ends_with("ORPHAN=keepme"));
    }

    #[test]
    fn last_value_wins_per_key() {
        let template = "#FOO=example\n";
        let old = "FOO=first\nFOO=second\n";
        let got = merge(old, template);
        assert!(got.contains("FOO=second"));
        assert!(!got.contains("FOO=first"));
    }

    #[test]
    fn no_real_lines_yields_bare_template() {
        let template = "# doc\n#FOO=example\n";
        let got = merge("", template);
        assert_eq!(got, "# doc\n#FOO=example");
    }

    #[test]
    fn value_placed_once_even_with_repeated_example() {
        // If the template somehow lists the same example twice, the value lands
        // under the first only.
        let template = "#FOO=example\n#FOO=example\n";
        let old = "FOO=v\n";
        let got = merge(old, template);
        assert_eq!(got.matches("FOO=v").count(), 1);
    }
}
