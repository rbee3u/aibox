//! Explicit host-side observations of official Component release sources.
//!
//! The snapshot is separate from native Component inspection. It is an
//! in-memory observation owned by the Service, not desired state or a Tenant
//! registry.

use crate::component::{ComponentKind, validate_stable_version};
use anyhow::{Context, Result, bail};
use futures_util::future::BoxFuture;
use futures_util::stream::StreamExt as _;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const SOURCE_TIMEOUT: Duration = Duration::from_secs(10);

/// Versioned Components whose stable release sources can be checked.
pub(crate) const VERSIONED_COMPONENTS: [ComponentKind; 6] = [
    ComponentKind::Node,
    ComponentKind::Codex,
    ComponentKind::Claude,
    ComponentKind::Python,
    ComponentKind::Rust,
    ComponentKind::Go,
];

/// One result in the Service latest-release observation.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub(crate) enum LatestEntryState {
    Available,
    Unavailable,
}

/// One result in the Service latest-release observation.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct LatestEntry {
    pub(crate) kind: ComponentKind,
    pub(crate) state: LatestEntryState,
    pub(crate) version: Option<String>,
    pub(crate) source: String,
    pub(crate) error: Option<String>,
}

/// The most recent explicit Component Update Check.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct LatestSnapshot {
    pub(crate) checked_at: String,
    pub(crate) entries: Vec<LatestEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LatestResult {
    Available {
        version: String,
        source: &'static str,
    },
    Unavailable {
        source: &'static str,
        error: String,
    },
}

/// Provider abstraction keeps route tests socket-free while production uses
/// the fixed official sources below.
pub(crate) trait LatestProvider: Send + Sync {
    fn fetch(&self, kind: ComponentKind) -> BoxFuture<'static, LatestResult>;
}

#[derive(Clone)]
pub(crate) struct OfficialLatestProvider {
    client: Client,
}

impl OfficialLatestProvider {
    pub(crate) fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("aibox/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(SOURCE_TIMEOUT)
            .timeout(SOURCE_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("build Component release client")?;
        Ok(Self { client })
    }

    async fn fetch_kind(client: Client, kind: ComponentKind) -> LatestResult {
        match kind {
            ComponentKind::Node => match fetch_node(&client).await {
                Ok(version) => LatestResult::Available {
                    version,
                    source: "nodejs.org",
                },
                Err(error) => unavailable("nodejs.org", error.to_string()),
            },
            ComponentKind::Go => match fetch_go(&client).await {
                Ok(version) => LatestResult::Available {
                    version,
                    source: "go.dev",
                },
                Err(error) => unavailable("go.dev", error.to_string()),
            },
            ComponentKind::Rust => match fetch_rust(&client).await {
                Ok(version) => LatestResult::Available {
                    version,
                    source: "static.rust-lang.org",
                },
                Err(error) => unavailable("static.rust-lang.org", error.to_string()),
            },
            ComponentKind::Python => match fetch_python(&client).await {
                Ok(version) => LatestResult::Available {
                    version,
                    source: "github.com/astral-sh/python-build-standalone",
                },
                Err(error) => unavailable(
                    "github.com/astral-sh/python-build-standalone",
                    error.to_string(),
                ),
            },
            ComponentKind::Codex => match fetch_codex(&client).await {
                Ok(version) => LatestResult::Available {
                    version,
                    source: "github.com/openai/codex",
                },
                Err(error) => unavailable("github.com/openai/codex", error.to_string()),
            },
            ComponentKind::Claude => match fetch_claude(&client).await {
                Ok(version) => LatestResult::Available {
                    version,
                    source: "registry.npmjs.org/@anthropic-ai/claude-code",
                },
                Err(error) => unavailable(
                    "registry.npmjs.org/@anthropic-ai/claude-code",
                    error.to_string(),
                ),
            },
            ComponentKind::ClaudeStatusline | ComponentKind::CodexStatusline => {
                unreachable!("statusline Components do not have remote release entries")
            }
        }
    }
}

impl LatestProvider for OfficialLatestProvider {
    fn fetch(&self, kind: ComponentKind) -> BoxFuture<'static, LatestResult> {
        let client = self.client.clone();
        Box::pin(async move { Self::fetch_kind(client, kind).await })
    }
}

#[cfg(test)]
pub(crate) struct FixtureLatestProvider {
    pub(crate) results: std::collections::BTreeMap<String, LatestResult>,
}

#[cfg(test)]
impl LatestProvider for FixtureLatestProvider {
    fn fetch(&self, kind: ComponentKind) -> BoxFuture<'static, LatestResult> {
        let result = self
            .results
            .get(kind.name())
            .cloned()
            .unwrap_or_else(|| unavailable(kind.name(), "fixture has no result"));
        Box::pin(async move { result })
    }
}

pub(crate) async fn check_snapshot(provider: Arc<dyn LatestProvider>) -> LatestSnapshot {
    check_snapshot_with_timeout(provider, SOURCE_TIMEOUT).await
}

async fn check_snapshot_with_timeout(
    provider: Arc<dyn LatestProvider>,
    timeout: Duration,
) -> LatestSnapshot {
    let results = futures_util::stream::iter(VERSIONED_COMPONENTS)
        .map(|kind| {
            let provider = provider.clone();
            async move {
                let result = tokio::time::timeout(timeout, provider.fetch(kind))
                    .await
                    .unwrap_or_else(|_| unavailable(kind.name(), "release source timed out"));
                (kind, result)
            }
        })
        .buffer_unordered(VERSIONED_COMPONENTS.len())
        .collect::<Vec<_>>()
        .await;
    snapshot_from_results(results)
}

fn snapshot_from_results(results: Vec<(ComponentKind, LatestResult)>) -> LatestSnapshot {
    let mut entries = results
        .into_iter()
        .map(|(kind, result)| match result {
            LatestResult::Available { version, source } => LatestEntry {
                kind,
                state: LatestEntryState::Available,
                version: Some(version),
                source: source.to_string(),
                error: None,
            },
            LatestResult::Unavailable { source, error } => LatestEntry {
                kind,
                state: LatestEntryState::Unavailable,
                version: None,
                source: source.to_string(),
                error: Some(error),
            },
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.kind);
    LatestSnapshot {
        checked_at: OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string()),
        entries,
    }
}

async fn fetch_node(client: &Client) -> Result<String> {
    let value = fetch_json(client, "https://nodejs.org/dist/index.json").await?;
    parse_node_releases(&value)
}

fn parse_node_releases(value: &Value) -> Result<String> {
    let releases = value
        .as_array()
        .context("Node.js release source is not an array")?;
    releases
        .iter()
        .filter_map(|release| release.get("version").and_then(Value::as_str))
        .find_map(|version| {
            version
                .strip_prefix('v')
                .and_then(|value| validate_stable_version(value).ok())
        })
        .context("Node.js release source has no stable release")
}

#[derive(Deserialize)]
struct GoRelease {
    version: String,
    stable: bool,
}

async fn fetch_go(client: &Client) -> Result<String> {
    let value = fetch_json(client, "https://go.dev/dl/?mode=json&include=all").await?;
    parse_go_releases(value)
}

fn parse_go_releases(value: Value) -> Result<String> {
    let releases: Vec<GoRelease> =
        serde_json::from_value(value).context("parse Go release list")?;
    releases
        .into_iter()
        .filter(|release| release.stable)
        .find_map(|release| {
            release
                .version
                .strip_prefix("go")
                .and_then(|value| validate_stable_version(value).ok())
        })
        .context("Go release source has no stable release")
}

async fn fetch_codex(client: &Client) -> Result<String> {
    let value = fetch_json(
        client,
        "https://api.github.com/repos/openai/codex/releases/latest",
    )
    .await?;
    parse_codex_release(&value)
}

fn parse_codex_release(value: &Value) -> Result<String> {
    value
        .get("tag_name")
        .and_then(Value::as_str)
        .and_then(|tag| tag.strip_prefix("rust-v"))
        .and_then(|version| validate_stable_version(version).ok())
        .context("Codex release source has no stable rust-vX.Y.Z tag")
}

async fn fetch_claude(client: &Client) -> Result<String> {
    let value = fetch_json(
        client,
        "https://registry.npmjs.org/@anthropic-ai/claude-code/latest",
    )
    .await?;
    parse_claude_release(&value)
}

fn parse_claude_release(value: &Value) -> Result<String> {
    value
        .get("version")
        .and_then(Value::as_str)
        .and_then(|version| validate_stable_version(version).ok())
        .context("Claude release source has no stable X.Y.Z version")
}

async fn fetch_python(client: &Client) -> Result<String> {
    let value = fetch_json(
        client,
        "https://api.github.com/repos/astral-sh/python-build-standalone/releases/latest",
    )
    .await?;
    parse_python_release(&value)
}

fn parse_python_release(value: &Value) -> Result<String> {
    value
        .get("assets")
        .and_then(Value::as_array)
        .context("Python build release source has no assets")?
        .iter()
        .filter_map(|asset| asset.get("name").and_then(Value::as_str))
        .filter_map(|name| {
            name.strip_prefix("cpython-")?
                .split_once('+')
                .map(|value| value.0)
        })
        .filter_map(|version| validate_stable_version(version).ok())
        .filter(|version| version.starts_with("3."))
        .max_by_key(|version| stable_version_parts(version).unwrap_or_default())
        .context("Python build release source has no stable CPython 3 release")
}

fn stable_version_parts(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.split('.').map(str::parse::<u32>);
    Some((
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    ))
}

async fn fetch_rust(client: &Client) -> Result<String> {
    let response = client
        .get("https://static.rust-lang.org/dist/channel-rust-stable.toml")
        .send()
        .await
        .context("request Rust stable channel")?
        .error_for_status()
        .context("Rust stable channel returned an error")?;
    let content = read_limited(response).await?;
    let content = std::str::from_utf8(&content).context("Rust stable channel is not UTF-8")?;
    parse_rust_channel(content)
}

fn parse_rust_channel(content: &str) -> Result<String> {
    let document = content
        .parse::<toml_edit::DocumentMut>()
        .context("parse Rust stable channel")?;
    document
        .get("pkg")
        .and_then(toml_edit::Item::as_table_like)
        .and_then(|pkg| pkg.get("rust"))
        .and_then(toml_edit::Item::as_table_like)
        .and_then(|rust| rust.get("version"))
        .and_then(toml_edit::Item::as_str)
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| validate_stable_version(value).ok())
        .context("Rust stable channel has no stable release version")
}

async fn fetch_json(client: &Client, url: &str) -> Result<Value> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("request {url}"))?
        .error_for_status()
        .with_context(|| format!("{url} returned an error"))?;
    let content = read_limited(response).await?;
    serde_json::from_slice(&content).with_context(|| format!("parse {url} response"))
}

async fn read_limited(response: reqwest::Response) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        bail!("release source response exceeds {MAX_RESPONSE_BYTES} bytes");
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("read release source response")?;
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            bail!("release source response exceeds {MAX_RESPONSE_BYTES} bytes");
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn unavailable(source: &'static str, error: impl Into<String>) -> LatestResult {
    LatestResult::Unavailable {
        source,
        error: error.into(),
    }
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
