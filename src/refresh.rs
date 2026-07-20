//! `refresh`: rewrite a config file's doc/example comments to the current
//! template while keeping every real config line the user added.
//!
//! The algorithm, given the old file's contents and a freshly generated
//! template:
//!
//! 1. Collect the old file's real (uncommented) lines — `KEY=VALUE` and bare
//!    `KEY` pass-through lines alike — last value winning per key, in
//!    first-seen order.
//! 2. Emit the fresh template line by line. After each `#KEY=` *example* line,
//!    if the old file had a real value for that key, drop it in directly under
//!    the example.
//! 3. Any real key with no matching example in the template is appended in a
//!    trailing "settings kept from your old file" block, so nothing is lost.
//!
//! The merge itself is a pure string transform; `run_refresh` decides whether
//! to write the result or print it (`--dry-run`).

use crate::agent::TEMPLATE_VERSION;
use crate::profile::{self, Profile};
use crate::template;
use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use std::fs;
use std::path::Path;

/// The trailing-block header, emitted once before any orphaned keys.
const ORPHAN_HEADER: &str =
    "# --- settings kept from your old file (no matching example above) ---";

/// Rewrite `old` against `template`: template docs/examples, with the user's real
/// values re-inserted under their matching examples and orphans appended.
/// Returns the new file contents without a trailing newline; `refresh_one` adds
/// one when writing.
pub fn merge(old: &str, template: &str) -> String {
    // 1. Collect real lines from the old file (last wins, keep order). A line
    //    with no '=' is a bare key (docker --env-file passes the host value
    //    through) — kept too, matching the run path in `envfile`.
    let mut vals: IndexMap<String, String> = IndexMap::new();
    for raw in old.lines() {
        let s = raw.trim_start();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        let key = match s.find('=') {
            Some(eq) => &s[..eq],
            None => s,
        };
        vals.insert(key.to_string(), s.to_string());
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

/// Dispatch a `refresh` invocation for `target`:
/// - `None` — base + every relay under `envs/`;
/// - `Some("base")` — just `base`;
/// - `Some(relay)` — one relay, resolved exactly like `-e` (a name under
///   `envs/`, or an explicit path when it contains `/` or ends in `.env`).
///
/// `dry_run` prints the result instead of writing.
pub fn run_refresh(prof: &Profile, target: Option<&str>, dry_run: bool) -> Result<i32> {
    match target {
        None => {
            refresh_one(prof, &prof.base_file, None, dry_run, false)?;
            if let Ok(rd) = fs::read_dir(&prof.envs_dir) {
                let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
                entries.sort();
                for path in entries {
                    if !path.is_file() {
                        continue;
                    }
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    // Hidden files (`.DS_Store` and friends) are never relays;
                    // skipping them keeps a stray binary file from aborting the
                    // whole sweep. An explicit `refresh <name>` still reaches
                    // them.
                    if name.starts_with('.') {
                        continue;
                    }
                    refresh_one(prof, &path, Some(&name), dry_run, false)?;
                }
            }
        }
        Some("base") => refresh_one(prof, &prof.base_file, None, dry_run, true)?,
        Some(relay) => {
            // Resolve exactly like `-e` does (name under envs/ vs explicit
            // path), so `refresh X` always targets the same file a run with
            // `-e X` reads.
            let rref = prof.relay_ref(relay);
            let name = rref
                .path()
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(relay)
                .to_string();
            refresh_one(prof, rref.path(), Some(&name), dry_run, true)?;
        }
    }
    Ok(0)
}

/// Refresh one file in place (or to stdout under `dry_run`). `relay_name` is
/// `None` for `base` (uses the base template) or `Some(name)` for a relay. A
/// missing file errors when the target was named explicitly (`required`) — a
/// typo must not exit 0 — and is skipped with a notice during the no-target
/// sweep.
fn refresh_one(
    prof: &Profile,
    file: &Path,
    relay_name: Option<&str>,
    dry_run: bool,
    required: bool,
) -> Result<()> {
    if !file.is_file() {
        if required {
            bail!("not found: {}", file.display());
        }
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
            ">> refreshed {} -> template v{TEMPLATE_VERSION}",
            file.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::AgentKind;

    #[test]
    fn run_refresh_skips_hidden_files_in_envs() {
        let root = tempfile::tempdir().unwrap();
        let prof = Profile::resolve(AgentKind::Claude, root.path(), "default");
        prof.resolve_relay_for_run("r").unwrap(); // scaffold base + relay
        let junk = [0u8, 159, 146, 150]; // not valid UTF-8
        let ds = prof.envs_dir.join(".DS_Store");
        std::fs::write(&ds, junk).unwrap();

        run_refresh(&prof, None, false).unwrap();

        assert_eq!(std::fs::read(&ds).unwrap(), junk, "dotfile left untouched");
    }

    #[test]
    fn run_refresh_explicit_missing_target_errors_but_sweep_skips() {
        let root = tempfile::tempdir().unwrap();
        let prof = Profile::resolve(AgentKind::Claude, root.path(), "default");
        // Nothing scaffolded yet: naming a missing target (a typo'd relay,
        // or base before first use) must fail, not exit 0.
        assert!(run_refresh(&prof, Some("base"), false).is_err());
        assert!(run_refresh(&prof, Some("nope"), false).is_err());
        // The no-target sweep still skips missing files quietly.
        assert!(run_refresh(&prof, None, false).is_ok());
    }

    #[test]
    fn run_refresh_resolves_path_target_like_a_run() {
        let root = tempfile::tempdir().unwrap();
        let prof = Profile::resolve(AgentKind::Claude, root.path(), "default");
        // An explicit path target (`-e` would read this same file) is refreshed
        // in place, not looked up under envs/.
        let outside = root.path().join("outside.env");
        std::fs::write(&outside, "ANTHROPIC_BASE_URL=https://x\n").unwrap();

        run_refresh(&prof, Some(outside.to_str().unwrap()), false).unwrap();

        let refreshed = std::fs::read_to_string(&outside).unwrap();
        assert!(refreshed.starts_with("# aibox-template:"));
        assert!(refreshed.contains("ANTHROPIC_BASE_URL=https://x"));
        assert!(!prof.envs_dir.join("outside.env").exists());
    }

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
    fn bare_key_line_is_kept() {
        // docker --env-file passes bare `KEY` lines through from the host env;
        // refresh must not drop them.
        let template = "#FOO=example\n";
        let old = "FOO=1\nMY_HOST_VAR\n";
        let got = merge(old, template);
        assert!(got.contains(ORPHAN_HEADER));
        assert!(got.trim_end().ends_with("MY_HOST_VAR"));
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

    /// Every shipped template (2 agents x base/relay), as `refresh` and
    /// first-run scaffolding generate it.
    fn shipped_templates() -> Vec<String> {
        let mut out = Vec::new();
        for agent in [AgentKind::Claude, AgentKind::Codex] {
            out.push(template::base_template(agent, TEMPLATE_VERSION));
            out.push(template::relay_template(agent, "r", TEMPLATE_VERSION));
        }
        out
    }

    #[test]
    fn shipped_templates_examples_match_refresh_pattern() {
        for template in shipped_templates() {
            let mut examples = 0;
            for line in template.lines() {
                // Templates are all comments and blanks; an active line would
                // silently configure every fresh profile.
                assert!(
                    line.is_empty() || line.starts_with('#'),
                    "active line in template: {line:?}"
                );
                // A `#` directly followed by a key-ish char is meant as a
                // `#KEY=example`. refresh must recognize it, or the user's real
                // line would be exiled to the orphan block on refresh.
                let keyish = line.strip_prefix('#').is_some_and(|rest| {
                    rest.chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                });
                if keyish {
                    assert!(
                        example_key(line).is_some(),
                        "example line does not match refresh pattern: {line:?}"
                    );
                    examples += 1;
                }
            }
            assert!(examples > 0, "template has no example lines");
        }
    }

    #[test]
    fn merge_reinserts_real_values_under_shipped_examples_and_is_idempotent() {
        for template in shipped_templates() {
            let mut keys: Vec<String> = Vec::new();
            for k in template.lines().filter_map(example_key) {
                if !keys.contains(&k) {
                    keys.push(k);
                }
            }

            // A user file: the template with a real line for every key.
            let mut old = template.clone();
            for k in &keys {
                old.push_str(&format!("{k}=real-{k}\n"));
            }

            let once = merge(&old, &template);
            let lines: Vec<&str> = once.lines().collect();
            for k in &keys {
                let real = format!("{k}=real-{k}");
                let pos = lines
                    .iter()
                    .position(|l| example_key(l).as_deref() == Some(k.as_str()))
                    .expect("example survives the merge");
                assert_eq!(lines[pos + 1], real, "real value sits under its example");
            }
            assert!(
                !once.contains(ORPHAN_HEADER),
                "every template key matched its example"
            );

            let twice = merge(&once, &template);
            assert_eq!(once, twice, "refresh is idempotent");
        }
    }
}
