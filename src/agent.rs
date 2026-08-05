//! Coding Agent-specific runtime and configuration contracts.
//!
//! Shared orchestration asks [`AgentKind`] for paths, Agent Profile files, and
//! command construction. Transcript parsing remains in the two Session backend
//! modules because the Coding Agents use different on-disk formats.

use anyhow::{Context, Result};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::ffi::OsString;

const DEFAULT_CODEX_PROFILE: &str = r#"approval_policy = "never"
sandbox_mode = "danger-full-access"
model_reasoning_effort = "xhigh"
plan_mode_reasoning_effort = "xhigh"
model = "gpt-5.6-sol"
model_provider = "custom"

[model_providers.custom]
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

    /// Primary configuration file materialized from an Agent Profile.
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
    /// Claude credentials are projected from this string map into
    /// `settings.json`'s `env` object during materialization.
    pub const fn profile_auth_file(self) -> &'static str {
        match self {
            Self::Claude | Self::Codex => "auth.json",
        }
    }

    /// Native files comprising the Agent Configuration.
    pub const fn agent_config_files(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["settings.json"],
            Self::Codex => &["config.toml", "auth.json"],
        }
    }

    /// Files comprising one Agent Profile definition.
    pub const fn profile_files(self) -> &'static [&'static str] {
        match self {
            Self::Claude => &["settings.json", "auth.json", ".metadata.json"],
            Self::Codex => &["config.toml", "auth.json", ".metadata.json"],
        }
    }

    /// Built-in native main configuration used by `profile create`.
    pub const fn profile_template(self) -> &'static str {
        match self {
            Self::Claude => DEFAULT_CLAUDE_PROFILE,
            Self::Codex => DEFAULT_CODEX_PROFILE,
        }
    }

    /// Built-in credential source used by `profile create`.
    pub const fn profile_auth_template(self) -> &'static str {
        match self {
            Self::Claude => DEFAULT_CLAUDE_AUTH,
            Self::Codex => DEFAULT_CODEX_AUTH,
        }
    }

    /// Parse the Coding Agent's native main configuration into a JSON object.
    pub(crate) fn parse_main_config(self, content: &str) -> Result<Map<String, Value>> {
        if content.trim().is_empty() {
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

    /// Normalize native Agent Configuration files into `/config` and `/auth`.
    pub(crate) fn normalize_config_files(
        self,
        main: &str,
        auth: Option<&str>,
        claude_auth_keys: &BTreeSet<String>,
    ) -> Result<Value> {
        let mut config = self
            .parse_main_config(main)
            .context("parse Agent Configuration")?;
        let mut root = Map::new();
        match self {
            Self::Codex => {
                root.insert("config".to_string(), Value::Object(config));
                let auth = parse_json_object(auth.unwrap_or(""), "Agent Configuration auth.json")?;
                root.insert("auth".to_string(), Value::Object(auth));
            }
            Self::Claude => {
                let mut logical_auth = Map::new();
                if let Some(Value::Object(env)) = config.get_mut("env") {
                    for key in claude_auth_keys {
                        if let Some(value) = env.remove(key) {
                            logical_auth.insert(key.clone(), value);
                        }
                    }
                    if env.is_empty() {
                        config.remove("env");
                    }
                }
                root.insert("config".to_string(), Value::Object(config));
                root.insert("auth".to_string(), Value::Object(logical_auth));
            }
        }
        Ok(Value::Object(root))
    }

    /// Render a normalized `/config` and `/auth` tree into native files.
    pub(crate) fn render_config_files(self, tree: &Value) -> Result<(String, Option<String>)> {
        let object = tree
            .as_object()
            .context("normalized Agent Configuration must be an object")?;
        let mut config = object
            .get("config")
            .and_then(Value::as_object)
            .cloned()
            .context("normalized Agent Configuration needs /config object")?;
        let auth = object
            .get("auth")
            .and_then(Value::as_object)
            .cloned()
            .context("normalized Agent Configuration needs /auth object")?;
        match self {
            Self::Codex => Ok((
                self.render_main_config(&Value::Object(config))?,
                Some(format!(
                    "{}\n",
                    serde_json::to_string_pretty(&Value::Object(auth))?
                )),
            )),
            Self::Claude => {
                if !auth.is_empty() {
                    let env = config
                        .entry("env".to_string())
                        .or_insert_with(|| Value::Object(Map::new()));
                    let env = env
                        .as_object_mut()
                        .context("Claude settings.env must be an object")?;
                    for (key, value) in auth {
                        env.insert(key, value);
                    }
                }
                Ok((self.render_main_config(&Value::Object(config))?, None))
            }
        }
    }

    /// Build the Coding Agent command without adding Agent Profile data.
    pub fn build_command(self, passthrough: &[OsString]) -> Vec<OsString> {
        let mut command = vec![OsString::from(self.tag())];
        command.extend(passthrough.iter().cloned());
        command
    }
}

fn parse_json_object(content: &str, label: &str) -> Result<Map<String, Value>> {
    let value = if content.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str::<Value>(content).with_context(|| format!("parse {label}"))?
    };
    value
        .as_object()
        .cloned()
        .with_context(|| format!("{label} must be a JSON object"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_kind_carries_agent_contracts() {
        for (agent, tag, state_dir, main, native_auth, agent_files, profile_files, auth) in [
            (
                AgentKind::Claude,
                "claude",
                ".claude",
                "settings.json",
                None,
                &["settings.json"][..],
                &["settings.json", "auth.json", ".metadata.json"][..],
                "{\n  \"ANTHROPIC_AUTH_TOKEN\": \"sk-example\"\n}\n",
            ),
            (
                AgentKind::Codex,
                "codex",
                ".codex",
                "config.toml",
                Some("auth.json"),
                &["config.toml", "auth.json"][..],
                &["config.toml", "auth.json", ".metadata.json"][..],
                "{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n",
            ),
        ] {
            assert_eq!(agent.tag(), tag, "{agent:?}");
            assert_eq!(agent.state_dir_name(), state_dir, "{agent:?}");
            assert_eq!(agent.main_config_file(), main, "{agent:?}");
            assert_eq!(agent.native_auth_file(), native_auth, "{agent:?}");
            assert_eq!(agent.profile_auth_file(), "auth.json", "{agent:?}");
            assert_eq!(agent.agent_config_files(), agent_files, "{agent:?}");
            assert_eq!(agent.profile_files(), profile_files, "{agent:?}");
            assert_eq!(agent.profile_auth_template(), auth, "{agent:?}");
        }
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
