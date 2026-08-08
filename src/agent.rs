//! Coding Agent-specific runtime and configuration contracts.
//!
//! Shared orchestration asks [`AgentKind`] for paths, Named Config files, and
//! command construction. Transcript parsing remains in the two Session backend
//! modules because the Coding Agents use different on-disk formats.

use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::ffi::OsString;

/// Primitive value accepted by one fixed Config Field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConfigValueKind {
    String,
    Bool,
}

/// One fixed main-configuration field that every Config Application updates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ConfigField {
    pub(crate) path: &'static [&'static str],
    pub(crate) value_kind: ConfigValueKind,
}

const CLAUDE_CONFIG_FIELDS: &[ConfigField] = &[
    ConfigField {
        path: &["env", "ANTHROPIC_BASE_URL"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["env", "ANTHROPIC_AUTH_TOKEN"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["env", "ANTHROPIC_DEFAULT_HAIKU_MODEL"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["env", "ANTHROPIC_DEFAULT_SONNET_MODEL"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["env", "ANTHROPIC_DEFAULT_OPUS_MODEL"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["env", "ANTHROPIC_DEFAULT_FABLE_MODEL"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["permissions", "defaultMode"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["skipDangerousModePermissionPrompt"],
        value_kind: ConfigValueKind::Bool,
    },
];

const CODEX_CONFIG_FIELDS: &[ConfigField] = &[
    ConfigField {
        path: &["approval_policy"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["sandbox_mode"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["model_reasoning_effort"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["plan_mode_reasoning_effort"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["model"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["model_provider"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["model_providers", "custom", "name"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["model_providers", "custom", "base_url"],
        value_kind: ConfigValueKind::String,
    },
    ConfigField {
        path: &["model_providers", "custom", "requires_openai_auth"],
        value_kind: ConfigValueKind::Bool,
    },
];

const DEFAULT_CODEX_CONFIG: &str = r#"approval_policy = "never"
sandbox_mode = "danger-full-access"
model_reasoning_effort = "xhigh"
plan_mode_reasoning_effort = "xhigh"
model = "gpt-5.6-sol"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://example.com/v1"
requires_openai_auth = true
"#;

const DEFAULT_CODEX_AUTH: &str = r#"{
  "OPENAI_API_KEY": "sk-example"
}
"#;

const DEFAULT_CLAUDE_CONFIG: &str = r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-example",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4-5",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-5[1m]",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-5[1m]",
    "ANTHROPIC_DEFAULT_FABLE_MODEL": "claude-fable-5[1m]"
  },
  "permissions": {
    "defaultMode": "bypassPermissions"
  },
  "skipDangerousModePermissionPrompt": true
}
"#;

/// Which Coding Agent a command targets.
///
/// Selected by `--agent` on Coding Agent-scoped commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum AgentKind {
    /// Anthropic Claude Code.
    Claude,
    /// OpenAI Codex.
    Codex,
}

impl AgentKind {
    /// Every Coding Agent supported by aibox.
    pub const ALL: [Self; 2] = [Self::Claude, Self::Codex];

    /// Lowercase name used by the CLI, paths, and executable.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// Human-readable name used in user-facing messages.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
        }
    }

    /// Agent state directory relative to the shared Tenant Home.
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
            (Self::Claude, "settings.json") => Some("{}\n"),
            (Self::Codex, "config.toml") => Some(""),
            (Self::Codex, "auth.json") => Some("{}\n"),
            _ => None,
        }
    }

    /// Fixed main-configuration fields accepted by a Named Config.
    pub(crate) const fn config_fields(self) -> &'static [ConfigField] {
        match self {
            Self::Claude => CLAUDE_CONFIG_FIELDS,
            Self::Codex => CODEX_CONFIG_FIELDS,
        }
    }

    /// Built-in native main configuration used by `config create`.
    pub const fn config_template(self) -> &'static str {
        match self {
            Self::Claude => DEFAULT_CLAUDE_CONFIG,
            Self::Codex => DEFAULT_CODEX_CONFIG,
        }
    }

    /// Built-in native credential template used by `config create`, if separate.
    pub const fn config_auth_template(self) -> Option<&'static str> {
        match self {
            Self::Claude => None,
            Self::Codex => Some(DEFAULT_CODEX_AUTH),
        }
    }

    /// Parse the Coding Agent's native main configuration into a JSON object.
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

    /// Build the Coding Agent command without adding Named Config data.
    pub fn build_command(self, passthrough: &[OsString]) -> Vec<OsString> {
        let mut command = vec![OsString::from(self.tag())];
        command.extend(passthrough.iter().cloned());
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_kind_carries_agent_contracts() {
        assert_eq!(AgentKind::Claude.display_name(), "Claude");
        assert_eq!(AgentKind::Codex.display_name(), "Codex");

        for (
            agent,
            tag,
            state_dir,
            main,
            native_auth,
            config_files,
            empty_files,
            config_fields,
            auth,
        ) in [
            (
                AgentKind::Claude,
                "claude",
                ".claude",
                "settings.json",
                None,
                &["settings.json"][..],
                &[("settings.json", "{}\n")][..],
                &[
                    (&["env", "ANTHROPIC_BASE_URL"][..], ConfigValueKind::String),
                    (
                        &["env", "ANTHROPIC_AUTH_TOKEN"][..],
                        ConfigValueKind::String,
                    ),
                    (
                        &["env", "ANTHROPIC_DEFAULT_HAIKU_MODEL"][..],
                        ConfigValueKind::String,
                    ),
                    (
                        &["env", "ANTHROPIC_DEFAULT_SONNET_MODEL"][..],
                        ConfigValueKind::String,
                    ),
                    (
                        &["env", "ANTHROPIC_DEFAULT_OPUS_MODEL"][..],
                        ConfigValueKind::String,
                    ),
                    (
                        &["env", "ANTHROPIC_DEFAULT_FABLE_MODEL"][..],
                        ConfigValueKind::String,
                    ),
                    (&["permissions", "defaultMode"][..], ConfigValueKind::String),
                    (
                        &["skipDangerousModePermissionPrompt"][..],
                        ConfigValueKind::Bool,
                    ),
                ][..],
                None,
            ),
            (
                AgentKind::Codex,
                "codex",
                ".codex",
                "config.toml",
                Some("auth.json"),
                &["config.toml", "auth.json"][..],
                &[("config.toml", ""), ("auth.json", "{}\n")][..],
                &[
                    (&["approval_policy"][..], ConfigValueKind::String),
                    (&["sandbox_mode"][..], ConfigValueKind::String),
                    (&["model_reasoning_effort"][..], ConfigValueKind::String),
                    (&["plan_mode_reasoning_effort"][..], ConfigValueKind::String),
                    (&["model"][..], ConfigValueKind::String),
                    (&["model_provider"][..], ConfigValueKind::String),
                    (
                        &["model_providers", "custom", "name"][..],
                        ConfigValueKind::String,
                    ),
                    (
                        &["model_providers", "custom", "base_url"][..],
                        ConfigValueKind::String,
                    ),
                    (
                        &["model_providers", "custom", "requires_openai_auth"][..],
                        ConfigValueKind::Bool,
                    ),
                ][..],
                Some("{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n"),
            ),
        ] {
            assert_eq!(agent.tag(), tag, "{agent:?}");
            assert_eq!(agent.state_dir_name(), state_dir, "{agent:?}");
            assert_eq!(agent.main_config_file(), main, "{agent:?}");
            assert_eq!(agent.native_auth_file(), native_auth, "{agent:?}");
            assert_eq!(agent.config_files(), config_files, "{agent:?}");
            assert_eq!(agent.config_auth_template(), auth, "{agent:?}");
            for (file, expected) in empty_files {
                assert_eq!(
                    agent.empty_config_file(file),
                    Some(*expected),
                    "{agent:?} {file}"
                );
            }
            assert_eq!(agent.empty_config_file("unknown"), None, "{agent:?}");
            let actual_fields: Vec<_> = agent
                .config_fields()
                .iter()
                .map(|field| (field.path, field.value_kind))
                .collect();
            assert_eq!(actual_fields, config_fields, "{agent:?}");
        }
    }

    #[test]
    fn build_command_preserves_passthrough_without_injecting_named_config() {
        let pass = vec![OsString::from("--model"), OsString::from("opus")];
        let command = AgentKind::Claude.build_command(&pass);
        assert_eq!(command, ["claude", "--model", "opus"]);

        let command = AgentKind::Codex.build_command(&[]);
        assert_eq!(command, ["codex"]);
    }

    #[cfg(unix)]
    #[test]
    fn command_preserves_non_utf8_passthrough_arguments() {
        use std::os::unix::ffi::OsStringExt;

        let opaque = OsString::from_vec(vec![b'f', 0x80, b'o']);
        let pass = vec![opaque.clone()];

        let command = AgentKind::Codex.build_command(&pass);

        assert_eq!(command, [OsString::from("codex"), opaque]);
    }
}
