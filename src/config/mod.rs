//! Named Config catalog, Current Config access, one-shot Config Application,
//! and the entry points for global Codex Credential Propagation.

mod application;
mod auth;
mod catalog;
mod definition;
mod editing;
mod files;
mod layout;
mod native;
mod visual;

use crate::application_error::{ApplicationErrorKind, application_error};
use crate::tenant;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::fmt;

pub(crate) use auth::{
    AuthPropagationPlan, AuthPropagationPreview, AuthPropagationReport,
    credential_propagation_source_available, execute_auth_propagation, plan_auth_propagation_from,
    preview_auth_propagation,
};
#[cfg(test)]
pub(crate) use auth::{PropagationEntry, PropagationOutcome, PropagationPreviewEntry};
#[cfg(test)]
pub(crate) use catalog::ConfigCatalogState;
pub(crate) use catalog::{
    ConfigCatalogEntry, create_named_config, delete_named_configs, inspect_current_config,
    inspect_named_configs,
};
pub(crate) use definition::{application_status, apply_named_config};
#[cfg(test)]
pub(crate) use editing::{read_config_file, save_config_file, save_config_file_with_linked};
#[cfg(test)]
pub(crate) use files::ensure_named_config_directory;
pub(crate) use native::{
    config_file_warnings, diagnose_config_file, inspect_named_codex_auth, read_config_file_target,
    save_config_file_target, visual_config_state,
};
pub(crate) use visual::{
    CodexAuthInspection, CustomProviderInput, CustomProviderState, VisualAuthInput,
    VisualConfigOptionInput, VisualConfigOptionState, VisualConfigState,
};

// Bound every untrusted native Config file before allocating it all.
use crate::foundation::MAX_NATIVE_CONFIG_BYTES as MAX_CONFIG_BYTES;
const LAST_APPLICATION_SECTION: &str = "last_application";

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
    /// The Coding Agent's main native configuration file.
    Main,
    /// The Coding Agent's native credential file.
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

    /// Resolve a wire filename against one Coding Agent contract.
    pub(crate) fn parse(agent: crate::agent::AgentKind, value: &str) -> Result<Self> {
        if value == agent.main_config_file() {
            return Ok(Self::Main);
        }
        if agent.native_auth_file() == Some(value) {
            return Ok(Self::Auth);
        }
        bail!("unsupported Config file for {}: {value}", agent.tag())
    }

    /// Return the native filename for one Coding Agent.
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
    /// The selected Coding Agent's Current Config.
    Current,
    /// One validated Named Config.
    Named(NamedConfigName),
}

impl ConfigTarget {
    /// Convert the existing wire selector into one legal internal target.
    pub(crate) fn from_wire(config: Option<&str>, current: bool) -> Result<Self> {
        match (current, config) {
            (true, None) => Ok(Self::Current),
            (false, Some(config)) => Ok(Self::Named(NamedConfigName::parse(config)?)),
            _ => bail!("select exactly one of Current Config or a Named Config"),
        }
    }

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
    /// Replace the selected native file with arbitrary decoded bytes.
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
    /// Convert the existing wire fields into one mutually exclusive edit.
    pub(crate) fn from_wire(
        content: Vec<u8>,
        custom_provider: Option<CustomProviderInput>,
        visual_options: Option<Vec<VisualConfigOptionInput>>,
        visual_auth: Option<VisualAuthInput>,
    ) -> Result<Self> {
        if content.len() as u64 > MAX_CONFIG_BYTES {
            return Err(application_error(
                ApplicationErrorKind::InputTooLarge,
                format!("configuration file exceeds {MAX_CONFIG_BYTES} bytes"),
            ));
        }
        match (visual_options, visual_auth) {
            (Some(_), Some(_)) => {
                bail!("select exactly one Visual Config editor operation")
            }
            (Some(options), None) => Ok(Self::VisualMain {
                options,
                custom_provider,
            }),
            (None, Some(auth)) => {
                if custom_provider.is_some() {
                    bail!("Custom Provider is only available for the main Config file");
                }
                Ok(Self::VisualAuth(auth))
            }
            (None, None) => Ok(Self::Raw {
                content,
                custom_provider,
            }),
        }
    }

    fn custom_provider(&self) -> Option<&CustomProviderInput> {
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

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "definition_visual_tests.rs"]
mod definition_visual_tests;
