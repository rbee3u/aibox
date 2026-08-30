//! Coding Agent-specific runtime and configuration contracts.
//!
//! [`AgentKind`] holds every match over the closed Agent set, so adding an
//! Agent makes the compiler name each contract still missing. Each Agent's
//! field table and templates live in its own module.
//!
//! Shared orchestration asks [`AgentKind`] for paths, Named Config files, and
//! command construction. Transcript parsing remains in the two Session backend
//! modules because the Coding Agents use different on-disk formats.

mod claude;
mod codex;

use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::ffi::OsString;
use std::path::Path;

/// Native executable and opaque arguments for one Coding Agent launch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentInvocation {
    command: Vec<OsString>,
}

impl AgentInvocation {
    /// Return the native command before Tenant Environment composition.
    pub(crate) fn command(&self) -> &[OsString] {
        &self.command
    }
}

/// Primitive value accepted by one fixed main-configuration Config Field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MainConfigValueKind {
    String,
    Bool,
}

/// One fixed main-configuration field that every Config Application updates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MainConfigField {
    pub(crate) path: &'static [&'static str],
    pub(crate) value_kind: MainConfigValueKind,
    pub(crate) label: &'static str,
    pub(crate) description: &'static str,
    pub(crate) group: &'static str,
    pub(crate) enum_values: &'static [&'static str],
    pub(crate) sensitive: bool,
    pub(crate) required: bool,
    pub(crate) required_for_custom_provider: bool,
    pub(crate) request_proxy_route: bool,
}

const NO_ENUM_VALUES: &[&str] = &[];

/// Which Coding Agent a command targets.
///
/// Selected by `--agent` on Coding Agent-scoped commands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, serde::Deserialize, serde::Serialize,
)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    /// Anthropic Claude Code.
    Claude,
    /// OpenAI Codex.
    Codex,
}

impl AgentKind {
    /// Every Coding Agent supported by AIBox.
    pub const ALL: [Self; 2] = [Self::Claude, Self::Codex];

    /// Lowercase name used by the CLI, paths, and executable.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// Agent state directory relative to the selected Tenant Home or Host Home.
    pub const fn state_dir_name(self) -> &'static str {
        match self {
            Self::Claude => ".claude",
            Self::Codex => ".codex",
        }
    }

    /// Primary native Current Config file.
    pub const fn main_config_file(self) -> &'static str {
        match self {
            Self::Claude => "settings.json",
            Self::Codex => "config.toml",
        }
    }

    /// Native authentication file in the Current Config, when separate.
    pub const fn native_auth_file(self) -> Option<&'static str> {
        match self {
            Self::Claude => None,
            Self::Codex => Some("auth.json"),
        }
    }

    /// Native files comprising a Named Config or Current Config.
    pub const fn config_files(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["settings.json"],
            Self::Codex => &["config.toml", "auth.json"],
        }
    }

    /// Empty native content used when editing a missing Current Config file.
    pub fn empty_config_file(self, file: &str) -> Option<&'static str> {
        match (self, file) {
            (Self::Claude, "settings.json") | (Self::Codex, "auth.json") => Some("{}\n"),
            (Self::Codex, "config.toml") => Some(""),
            _ => None,
        }
    }

    /// Fixed main-configuration fields accepted by a Named Config.
    pub(crate) const fn main_config_fields(self) -> &'static [MainConfigField] {
        match self {
            Self::Claude => claude::MAIN_CONFIG_FIELDS,
            Self::Codex => codex::MAIN_CONFIG_FIELDS,
        }
    }

    /// Built-in native main configuration used when the Console creates a Named Config.
    pub const fn config_template(self) -> &'static str {
        match self {
            Self::Claude => claude::DEFAULT_CONFIG,
            Self::Codex => codex::DEFAULT_CONFIG,
        }
    }

    /// Built-in native credential template used when the Console creates a Named Config.
    pub const fn config_auth_template(self) -> Option<&'static str> {
        match self {
            Self::Claude => None,
            Self::Codex => Some(codex::DEFAULT_AUTH),
        }
    }

    /// Parse the Coding Agent's native main configuration (Claude JSON or
    /// Codex TOML) into a generic object map.
    pub(crate) fn parse_main_config(self, content: &str) -> Result<Map<String, Value>> {
        if self == Self::Codex && content.trim().is_empty() {
            return Ok(Map::new());
        }
        let value = match self {
            Self::Codex => toml_edit::de::from_str::<Value>(content)?,
            Self::Claude => serde_json::from_str::<Value>(content)?,
        };
        value
            .as_object()
            .cloned()
            .with_context(|| format!("{} main configuration must be an object", self.tag()))
    }

    /// Render a JSON object in the Coding Agent's native main format.
    pub(crate) fn render_main_config(self, value: &Value) -> Result<String> {
        if !value.is_object() {
            anyhow::bail!("{} main configuration must be an object", self.tag());
        }
        match self {
            Self::Codex => Ok(toml_edit::ser::to_string_pretty(value)?),
            Self::Claude => Ok(format!("{}\n", serde_json::to_string_pretty(value)?)),
        }
    }

    /// Build the native Coding Agent invocation without Tenant Environment
    /// wrapping or Named Config data.
    pub(crate) fn invocation(self, home: &Path, passthrough: &[OsString]) -> AgentInvocation {
        let mut command = vec![home.join(".local/bin").join(self.tag()).into_os_string()];
        command.extend(passthrough.iter().cloned());
        AgentInvocation { command }
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
