//! Optional capabilities derived from native state in a Tenant.
//!
//! Statusline Components can edit native Current Config in a Managed Tenant
//! Home or the Host Home, while runtime Components own Managed Tenant-local
//! executables and SDK directories. There is no Component registry, so
//! inspection derives state directly from native files.

mod catalog;
mod native;
mod node_agent;
mod python;
mod runtime;
mod rust_go;
mod statusline;
mod updates;

pub(crate) use updates::{LatestProvider, LatestSnapshot, OfficialLatestProvider, check_snapshot};

/// Types reachable only through a seam production code does not name itself.
///
/// [`LatestSnapshot`] carries `Vec<LatestEntry>` and `ts_rs` will not export a
/// nested type on its own, so `service/control/contract.rs` must name both.
/// `LatestResult` is [`LatestProvider`]'s return type: production reaches the
/// provider only through [`check_snapshot`], which yields a whole
/// [`LatestSnapshot`], while implementing that trait — as the fixture provider
/// in `testutil` does — requires naming what `fetch` returns.
/// See `docs/adr/0009-rust-owned-console-contract.md`.
#[cfg(test)]
pub(crate) use updates::{LatestEntry, LatestEntryState, LatestResult};

use crate::agent::AgentKind;
use crate::tenant::{Tenant, TenantEnvironmentCapabilities};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;
use std::str::FromStr;

// Native Component files are untrusted and must be bounded before parsing.
use crate::foundation::MAX_NATIVE_CONFIG_BYTES as MAX_CONFIG_BYTES;

/// One optional capability that AIBox can install into a Tenant's native state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ComponentKind {
    /// Tenant-local Node.js runtime.
    Node,
    /// Tenant-local OpenAI Codex executable.
    Codex,
    /// Tenant-local Anthropic Claude Code executable.
    Claude,
    /// Tenant-local uv and CPython toolchain.
    Python,
    /// Claude Code statusline integration.
    ClaudeStatusline,
    /// OpenAI Codex statusline integration.
    CodexStatusline,
    /// Tenant-local stable Rust toolchain.
    Rust,
    /// Tenant-local stable Go toolchain.
    Go,
}

impl ComponentKind {
    pub(crate) const ALL: [Self; 8] = [
        Self::Codex,
        Self::CodexStatusline,
        Self::Claude,
        Self::ClaudeStatusline,
        Self::Node,
        Self::Python,
        Self::Rust,
        Self::Go,
    ];
    pub(crate) const STATUSLINES: [Self; 2] = [Self::ClaudeStatusline, Self::CodexStatusline];

    /// Stable Component name.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Python => "python",
            Self::ClaudeStatusline => "claude-statusline",
            Self::CodexStatusline => "codex-statusline",
            Self::Rust => "rust",
            Self::Go => "go",
        }
    }

    pub(crate) fn supports_version(self) -> bool {
        !self.is_statusline()
    }

    pub(crate) fn is_statusline(self) -> bool {
        matches!(self, Self::ClaudeStatusline | Self::CodexStatusline)
    }

    fn for_agent(agent: AgentKind) -> Self {
        match agent {
            AgentKind::Claude => Self::Claude,
            AgentKind::Codex => Self::Codex,
        }
    }
}

/// State derived from a Component's native files in one Tenant's Home.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ComponentStatus {
    /// The Component exactly matches the current AIBox definition.
    Installed {
        /// Stable runtime or toolchain version; absent for statusline Components.
        version: Option<String>,
    },
    /// Some statusline state exists but differs from the current definition.
    Modified,
    /// Recognizable AIBox-owned state exists but is only partially installed
    /// or is not healthy enough to run.
    Incomplete,
    /// Component state exists but AIBox must not take ownership of it.
    Unmanaged,
    /// No Component-owned state exists.
    NotInstalled,
}

#[derive(Debug)]
pub(crate) struct ComponentInspection {
    pub(crate) kind: ComponentKind,
    pub(crate) status: Option<ComponentStatus>,
    pub(crate) error: Option<String>,
}

/// A Component name and optional stable runtime or toolchain version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ComponentSpec {
    /// Selected Component.
    kind: ComponentKind,
    /// Requested stable version; absence selects latest for versioned
    /// Components and is required for statuslines.
    version: Option<String>,
}

impl ComponentSpec {
    pub(crate) fn new(kind: ComponentKind, version: Option<String>) -> Result<Self, String> {
        if version.is_some() && !kind.supports_version() {
            return Err(format!("{} does not accept a version", kind.name()));
        }
        let version = version
            .as_deref()
            .map(validate_stable_version)
            .transpose()?;
        Ok(Self { kind, version })
    }

    pub(crate) fn kind(&self) -> ComponentKind {
        self.kind
    }
}

impl fmt::Display for ComponentSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.kind.name())?;
        if let Some(version) = &self.version {
            write!(formatter, "@{version}")?;
        }
        Ok(())
    }
}

impl FromStr for ComponentSpec {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (name, version) = value
            .split_once('@')
            .map_or((value, None), |(name, version)| (name, Some(version)));
        let kind = name.parse::<ComponentKind>()?;
        Self::new(kind, version.map(str::to_owned))
    }
}

impl FromStr for ComponentKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "node" => Ok(Self::Node),
            "codex" => Ok(Self::Codex),
            "claude" => Ok(Self::Claude),
            "python" => Ok(Self::Python),
            "claude-statusline" => Ok(Self::ClaudeStatusline),
            "codex-statusline" => Ok(Self::CodexStatusline),
            "rust" => Ok(Self::Rust),
            "go" => Ok(Self::Go),
            _ => Err(format!("unknown Component {value:?}")),
        }
    }
}

pub(crate) fn validate_stable_version(version: &str) -> Result<String, String> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
        })
    {
        return Err(format!(
            "invalid stable Component version {version:?}; expected X.Y.Z"
        ));
    }
    Ok(version.to_string())
}

pub(crate) fn inspect_catalog(selected: &Tenant) -> Result<Vec<ComponentInspection>> {
    catalog::inspect_catalog(selected)
}

pub(crate) fn inspect_tenant_environment_components(
    home: &Path,
) -> (TenantEnvironmentCapabilities, Vec<String>) {
    catalog::inspect_tenant_environment_components(home)
}

pub(crate) fn require_agent_component(agent: AgentKind, home: &Path) -> Result<()> {
    catalog::require_agent_component(agent, home)
}

/// Install one Component into the selected Tenant's native state.
///
/// `log` streams container output to a Management Operation; statusline
/// Components never start a container and ignore it.
pub(crate) fn install_component(
    selected: &Tenant,
    component: &ComponentSpec,
    log: Option<crate::docker::LogCallback>,
) -> Result<i32> {
    reject_host_runtime_component(selected, component.kind)?;
    match component.kind {
        ComponentKind::ClaudeStatusline => statusline::install_claude_statusline(selected),
        ComponentKind::CodexStatusline => statusline::install_codex_statusline(selected),
        ComponentKind::Node
        | ComponentKind::Codex
        | ComponentKind::Claude
        | ComponentKind::Python
        | ComponentKind::Rust
        | ComponentKind::Go => {
            let Tenant::Managed(tenant) = selected else {
                unreachable!("Host runtime Components are rejected above")
            };
            runtime::install_runtime_component(
                tenant,
                component,
                &crate::docker::DockerCli::system(),
                log,
            )
        }
    }
}

pub(crate) fn remove_component(selected: &Tenant, kind: ComponentKind) -> Result<i32> {
    reject_host_runtime_component(selected, kind)?;
    if !catalog::tenant_home_exists(selected)? {
        if matches!(selected, Tenant::Host { .. }) {
            bail!(
                "Host Home does not exist: {}",
                selected.home_dir().display()
            );
        }
        return Ok(0);
    }
    let status = catalog::inspect(kind, selected.home_dir())?;
    if status == ComponentStatus::NotInstalled {
        return Ok(0);
    }
    if status == ComponentStatus::Unmanaged {
        bail!(
            "{} has unmanaged Component state; refusing to remove foreign files",
            kind.name()
        );
    }
    match kind {
        ComponentKind::Node => node_agent::remove_node(selected.home_dir())?,
        ComponentKind::Codex => node_agent::remove_codex(selected.home_dir())?,
        ComponentKind::Claude => node_agent::remove_claude(selected.home_dir())?,
        ComponentKind::Python => python::remove_python(selected.home_dir())?,
        ComponentKind::ClaudeStatusline => statusline::remove_claude_statusline(selected)?,
        ComponentKind::CodexStatusline => statusline::remove_codex_statusline(selected)?,
        ComponentKind::Rust => rust_go::remove_rust(selected.home_dir())?,
        ComponentKind::Go => rust_go::remove_go(selected.home_dir())?,
    }
    Ok(0)
}

fn reject_host_runtime_component(selected: &Tenant, kind: ComponentKind) -> Result<()> {
    if matches!(selected, Tenant::Host { .. }) && !kind.is_statusline() {
        bail!(
            "{} is unavailable to the Host Tenant; it supports only claude-statusline and codex-statusline",
            kind.name()
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "component_tests.rs"]
mod tests;
