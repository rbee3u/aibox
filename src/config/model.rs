//! Config identity, selections, edits, and domain snapshots.

use super::visual::{CustomProviderInput, VisualAuthInput, VisualConfigOptionInput};
use crate::tenant;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::fmt;
/// A validated Named Config name.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub(crate) struct NamedConfigName(String);

impl NamedConfigName {
    /// Parse a lowercase DNS label.
    pub(crate) fn parse(value: &str) -> Result<Self> {
        tenant::validate_name("config", value)?;
        Ok(Self(value.to_string()))
    }

    /// Return the validated name as text.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NamedConfigName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One Agent-defined native Config file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfigFile {
    /// The Agent's main native configuration file.
    Main,
    /// The Agent's native credential file.
    Auth,
}

impl ConfigFile {
    pub(crate) fn all(agent: crate::agent::AgentKind) -> impl Iterator<Item = Self> {
        [
            Some(Self::Main),
            agent.native_auth_file().map(|_| Self::Auth),
        ]
        .into_iter()
        .flatten()
    }

    /// Resolve a wire filename against one Agent contract.
    pub(crate) fn parse(agent: crate::agent::AgentKind, value: &str) -> Result<Self> {
        if value == agent.main_config_file() {
            return Ok(Self::Main);
        }
        if agent.native_auth_file() == Some(value) {
            return Ok(Self::Auth);
        }
        bail!("unsupported Config file for {}: {value}", agent.tag())
    }

    /// Return the native filename for one Agent.
    pub(crate) fn as_str(self, agent: crate::agent::AgentKind) -> &'static str {
        match self {
            Self::Main => agent.main_config_file(),
            Self::Auth => agent
                .native_auth_file()
                .expect("ConfigFile::Auth requires an Agent auth contract"),
        }
    }
}

/// A mutually exclusive Current or Named Config selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ConfigTarget {
    /// The selected Agent's Current Config.
    Current,
    /// One validated Named Config.
    Named(NamedConfigName),
}

impl ConfigTarget {
    pub(crate) fn named(&self) -> Option<&NamedConfigName> {
        match self {
            Self::Current => None,
            Self::Named(name) => Some(name),
        }
    }

    pub(crate) fn is_current(&self) -> bool {
        matches!(self, Self::Current)
    }
}

/// One legal Raw or Visual Config edit submitted after wire decoding.
#[derive(Clone, Debug)]
pub(crate) enum ConfigEdit {
    /// Submit decoded bytes for the selected native file.
    ///
    /// Current Config accepts arbitrary bytes; Named Config validates the
    /// selected file before committing it.
    Raw {
        content: Vec<u8>,
        custom_provider: Option<CustomProviderInput>,
    },
    /// Render the main Named Config from Visual Editor options.
    VisualMain {
        options: Vec<VisualConfigOptionInput>,
        custom_provider: Option<CustomProviderInput>,
    },
    /// Render the Codex credential file from the Visual Editor.
    VisualAuth(VisualAuthInput),
}

impl ConfigEdit {
    pub(super) fn custom_provider(&self) -> Option<&CustomProviderInput> {
        match self {
            Self::Raw {
                custom_provider, ..
            }
            | Self::VisualMain {
                custom_provider, ..
            } => custom_provider.as_ref(),
            Self::VisualAuth(_) => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct LastApplication {
    pub(crate) applied: String,
    pub(crate) applied_at: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConfigDrift {
    Untracked,
    Clean,
    Dirty,
    SourceMissing,
    ComparisonError,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct ApplicationStatus {
    pub(crate) last_application: Option<LastApplication>,
    pub(crate) drift: ConfigDrift,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub(crate) detail: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConfigFileSnapshot {
    pub(crate) file: String,
    pub(crate) exists: bool,
    pub(crate) content: Vec<u8>,
    pub(crate) revision: String,
}

pub(crate) struct ConfigSaveResult {
    pub(crate) snapshot: ConfigFileSnapshot,
    pub(crate) linked: Option<ConfigFileSnapshot>,
}

pub(crate) struct ConfigDiagnostic {
    pub(crate) message: String,
    pub(crate) line: usize,
    pub(crate) column: usize,
}
