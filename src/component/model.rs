//! Component identities, specifications, and observed native state.

use crate::agent::AgentKind;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

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

    pub(super) fn for_agent(agent: AgentKind) -> Self {
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
    pub(super) kind: ComponentKind,
    /// Requested stable version; absence selects latest for versioned
    /// Components and is required for statuslines.
    pub(super) version: Option<String>,
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
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == value)
            .ok_or_else(|| format!("unknown Component {value:?}"))
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

/// Healthy Components that contribute defaults to a Tenant Environment.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct TenantEnvironmentCapabilities {
    pub(crate) node: bool,
    pub(crate) claude: bool,
    pub(crate) python: bool,
    pub(crate) rust: bool,
    pub(crate) go: bool,
}
