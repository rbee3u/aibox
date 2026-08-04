//! Coding Agent-specific runtime and configuration contracts.
//!
//! Shared orchestration asks [`AgentKind`] for paths, Agent Profile files, and
//! command construction. Transcript parsing remains in the two Session backend
//! modules because the Coding Agents use different on-disk formats.

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
        for (agent, tag, state_dir, main, native_auth, agent_files, profile_files) in [
            (
                AgentKind::Claude,
                "claude",
                ".claude",
                "settings.json",
                None,
                &["settings.json"][..],
                &["settings.json", "auth.json", ".metadata.json"][..],
            ),
            (
                AgentKind::Codex,
                "codex",
                ".codex",
                "config.toml",
                Some("auth.json"),
                &["config.toml", "auth.json"][..],
                &["config.toml", "auth.json", ".metadata.json"][..],
            ),
        ] {
            assert_eq!(agent.tag(), tag, "{agent:?}");
            assert_eq!(agent.state_dir_name(), state_dir, "{agent:?}");
            assert_eq!(agent.main_config_file(), main, "{agent:?}");
            assert_eq!(agent.native_auth_file(), native_auth, "{agent:?}");
            assert_eq!(agent.profile_auth_file(), "auth.json", "{agent:?}");
            assert_eq!(agent.agent_config_files(), agent_files, "{agent:?}");
            assert_eq!(agent.profile_files(), profile_files, "{agent:?}");
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
