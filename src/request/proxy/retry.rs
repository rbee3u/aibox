//! Startup-loaded rules for retrying rate-limited upstream requests.

use anyhow::{Context, Result, bail};
use std::io::Read;
use std::path::Path;
use url::Url;

const RULE_FILE: &str = "retry_urls.txt";
const MAX_RULE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Default)]
pub(in crate::request) struct RetryRules(Vec<Url>);

impl RetryRules {
    pub(in crate::request) fn load(root: &Path) -> Result<Self> {
        let path = root.join(RULE_FILE);
        if !crate::foundation::safe_fs::real_file_exists(&path, "Request retry rules")? {
            return Ok(Self::default());
        }
        let file =
            crate::foundation::safe_fs::open_regular_beneath(root, &path, "Request retry rules")?;
        if file.metadata()?.len() > MAX_RULE_BYTES {
            bail!("{} exceeds {MAX_RULE_BYTES} bytes", path.display());
        }
        let mut bytes = Vec::new();
        file.take(MAX_RULE_BYTES + 1)
            .read_to_end(&mut bytes)
            .with_context(|| format!("read {}", path.display()))?;
        if bytes.len() as u64 > MAX_RULE_BYTES {
            bail!("{} exceeds {MAX_RULE_BYTES} bytes", path.display());
        }
        let content =
            String::from_utf8(bytes).with_context(|| format!("{} is not UTF-8", path.display()))?;
        let mut rules = Vec::new();
        for (index, raw) in content.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let url = Url::parse(line)
                .with_context(|| format!("{}:{} is not a valid URL", path.display(), index + 1))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                bail!(
                    "{}:{} must be an HTTP(S) base URL without credentials, query, or fragment",
                    path.display(),
                    index + 1
                );
            }
            rules.push(url);
        }
        Ok(Self(rules))
    }

    pub(super) fn matches(&self, target: &Url) -> bool {
        self.0.iter().any(|rule| {
            rule.scheme() == target.scheme()
                && rule.host() == target.host()
                && rule.port_or_known_default() == target.port_or_known_default()
                && path_matches(rule.path(), target.path())
        })
    }
}

fn path_matches(rule: &str, target: &str) -> bool {
    let base = rule.trim_end_matches('/');
    base.is_empty()
        || target == base
        || target
            .strip_prefix(base)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
#[path = "retry_tests.rs"]
mod tests;
