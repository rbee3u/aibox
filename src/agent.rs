//! Coding Agent-specific runtime and configuration contracts.
//!
//! Shared orchestration asks [`AgentKind`] for paths, Agent Profile files, and
//! command construction. Transcript parsing remains in the two Session backend
//! modules because the Coding Agents use different on-disk formats.

use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::ffi::OsString;

/// Primitive value accepted by one fixed Agent Profile field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProfileValueKind {
    String,
    Bool,
}

/// One fixed main-configuration field that every Profile Application updates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProfileField {
    pub(crate) path: &'static [&'static str],
    pub(crate) value_kind: ProfileValueKind,
}

/// Agent-specific interpretation of an Agent Profile `auth.json`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProfileAuthKind {
    ClaudeToken,
    CodexObject,
}

const CLAUDE_PROFILE_FIELDS: &[ProfileField] = &[
    ProfileField {
        path: &["env", "ANTHROPIC_BASE_URL"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["env", "ANTHROPIC_DEFAULT_HAIKU_MODEL"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["env", "ANTHROPIC_DEFAULT_SONNET_MODEL"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["env", "ANTHROPIC_DEFAULT_OPUS_MODEL"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["env", "ANTHROPIC_DEFAULT_FABLE_MODEL"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["permissions", "defaultMode"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["skipDangerousModePermissionPrompt"],
        value_kind: ProfileValueKind::Bool,
    },
];

const CODEX_PROFILE_FIELDS: &[ProfileField] = &[
    ProfileField {
        path: &["approval_policy"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["sandbox_mode"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["model_reasoning_effort"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["plan_mode_reasoning_effort"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["model"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["model_provider"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["model_providers", "custom", "name"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["model_providers", "custom", "base_url"],
        value_kind: ProfileValueKind::String,
    },
    ProfileField {
        path: &["model_providers", "custom", "requires_openai_auth"],
        value_kind: ProfileValueKind::Bool,
    },
];

const DEFAULT_CODEX_PROFILE: &str = r#"approval_policy = "never"
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

const DEFAULT_CLAUDE_PROFILE: &str = r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://example.com",
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

const DEFAULT_CLAUDE_AUTH: &str = r#"{
  "ANTHROPIC_AUTH_TOKEN": "sk-example"
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

    /// Agent state directory relative to the shared Tenant Home.
    pub const fn state_dir_name(self) -> &'static str {
        match self {
            Self::Claude => ".claude",
            Self::Codex => ".codex",
        }
    }

    /// Primary native Agent Configuration file.
    pub const fn main_config_file(self) -> &'static str {
        match self {
            Self::Claude => "settings.json",
            Self::Codex => "config.toml",
        }
    }

    /// Native authentication file in the Agent Configuration, when separate.
    pub const fn native_auth_file(self) -> Option<&'static str> {
        match self {
            Self::Claude => None,
            Self::Codex => Some("auth.json"),
        }
    }

    /// Agent Profile credential file.
    ///
    /// Claude's optional token is projected into `settings.json.env` during
    /// Profile Application; Codex auth replaces the native file as an object.
    pub const fn profile_auth_file(self) -> &'static str {
        match self {
            Self::Claude | Self::Codex => "auth.json",
        }
    }

    /// Files comprising one complete Agent Profile definition.
    pub const fn profile_files(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["settings.json", "auth.json"],
            Self::Codex => &["config.toml", "auth.json"],
        }
    }

    /// Fixed main-configuration fields accepted by an Agent Profile.
    pub(crate) const fn profile_fields(self) -> &'static [ProfileField] {
        match self {
            Self::Claude => CLAUDE_PROFILE_FIELDS,
            Self::Codex => CODEX_PROFILE_FIELDS,
        }
    }

    /// Authentication contract used by an Agent Profile.
    pub(crate) const fn profile_auth_kind(self) -> ProfileAuthKind {
        match self {
            Self::Claude => ProfileAuthKind::ClaudeToken,
            Self::Codex => ProfileAuthKind::CodexObject,
        }
    }

    /// Built-in native main configuration used by `profile create`.
    pub const fn profile_template(self) -> &'static str {
        match self {
            Self::Claude => DEFAULT_CLAUDE_PROFILE,
            Self::Codex => DEFAULT_CODEX_PROFILE,
        }
    }

    /// Built-in credential template used by `profile create`.
    pub const fn profile_auth_template(self) -> &'static str {
        match self {
            Self::Claude => DEFAULT_CLAUDE_AUTH,
            Self::Codex => DEFAULT_CODEX_AUTH,
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

    /// Build the Coding Agent command without adding Agent Profile data.
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
        for (agent, tag, state_dir, main, native_auth, profile_files, auth) in [
            (
                AgentKind::Claude,
                "claude",
                ".claude",
                "settings.json",
                None,
                &["settings.json", "auth.json"][..],
                "{\n  \"ANTHROPIC_AUTH_TOKEN\": \"sk-example\"\n}\n",
            ),
            (
                AgentKind::Codex,
                "codex",
                ".codex",
                "config.toml",
                Some("auth.json"),
                &["config.toml", "auth.json"][..],
                "{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n",
            ),
        ] {
            assert_eq!(agent.tag(), tag, "{agent:?}");
            assert_eq!(agent.state_dir_name(), state_dir, "{agent:?}");
            assert_eq!(agent.main_config_file(), main, "{agent:?}");
            assert_eq!(agent.native_auth_file(), native_auth, "{agent:?}");
            assert_eq!(agent.profile_auth_file(), "auth.json", "{agent:?}");
            assert_eq!(agent.profile_files(), profile_files, "{agent:?}");
            assert_eq!(agent.profile_auth_template(), auth, "{agent:?}");
        }
        assert_eq!(AgentKind::Claude.profile_fields().len(), 7);
        assert_eq!(AgentKind::Codex.profile_fields().len(), 9);
        assert_eq!(
            AgentKind::Claude.profile_auth_kind(),
            ProfileAuthKind::ClaudeToken
        );
        assert_eq!(
            AgentKind::Codex.profile_auth_kind(),
            ProfileAuthKind::CodexObject
        );
    }

    #[test]
    fn build_command_preserves_passthrough_without_injecting_profile_config() {
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
