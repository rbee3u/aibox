//! Agent-specific runtime and configuration contracts.
//!
//! Shared orchestration asks [`AgentKind`] for paths, managed files, and command
//! construction. Transcript parsing remains in the two session backend modules
//! because the agents use different on-disk formats.

use crate::runspec::Invocation;
use std::ffi::OsString;

/// Which agent a command targets. Selected by `--agent` on agent-scoped commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum AgentKind {
    /// Anthropic Claude Code.
    Claude,
    /// OpenAI Codex.
    Codex,
}

impl AgentKind {
    /// Lowercase name used by the CLI, paths, and executable.
    pub fn tag(self) -> &'static str {
        match self {
            AgentKind::Claude => "claude",
            AgentKind::Codex => "codex",
        }
    }

    /// Built-in shared Docker image tag for this agent.
    pub fn image_default(self) -> &'static str {
        match self {
            AgentKind::Claude | AgentKind::Codex => crate::docker::IMAGE,
        }
    }

    /// Absolute home directory mounted inside the container.
    pub fn container_home(self) -> &'static str {
        match self {
            AgentKind::Claude | AgentKind::Codex => "/home/aibox",
        }
    }

    /// Agent state directory relative to the shared Tenant Home.
    pub fn active_dir_name(self) -> &'static str {
        match self {
            AgentKind::Claude => ".claude",
            AgentKind::Codex => ".codex",
        }
    }

    /// Primary configuration file materialized from a Provider.
    pub fn main_config_file(self) -> &'static str {
        match self {
            AgentKind::Claude => "settings.json",
            AgentKind::Codex => "config.toml",
        }
    }

    /// Native authentication file in the Agent Configuration, when separate.
    pub fn active_auth_file(self) -> Option<&'static str> {
        match self {
            AgentKind::Claude => None,
            AgentKind::Codex => Some("auth.json"),
        }
    }

    /// Provider credential file. Claude credentials are projected from this
    /// string map into `settings.json`'s `env` object during materialization.
    pub fn provider_auth_file(self) -> &'static str {
        match self {
            AgentKind::Claude | AgentKind::Codex => "auth.json",
        }
    }

    /// Native files comprising the Agent Configuration.
    pub fn agent_config_files(self) -> &'static [&'static str] {
        match self {
            AgentKind::Claude => &["settings.json"],
            AgentKind::Codex => &["config.toml", "auth.json"],
        }
    }

    /// Files comprising one Provider definition.
    pub fn provider_files(self) -> &'static [&'static str] {
        match self {
            AgentKind::Claude => &["settings.json", "auth.json", ".metadata.json"],
            AgentKind::Codex => &["config.toml", "auth.json", ".metadata.json"],
        }
    }

    /// Build the agent command without adding provider data to the container.
    pub fn build_invocation(self, passthrough: &[OsString]) -> Invocation {
        let mut agent_cmd = vec![OsString::from(self.tag())];
        agent_cmd.extend(passthrough.iter().cloned());
        Invocation {
            extra_run_args: Vec::new(),
            agent_cmd,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_kind_carries_agent_contracts() {
        assert_eq!(AgentKind::Claude.tag(), "claude");
        assert_eq!(AgentKind::Codex.tag(), "codex");
        assert_eq!(AgentKind::Claude.image_default(), "aibox:latest");
        assert_eq!(AgentKind::Codex.image_default(), "aibox:latest");
        assert_eq!(AgentKind::Claude.container_home(), "/home/aibox");
        assert_eq!(AgentKind::Codex.container_home(), "/home/aibox");
        assert_eq!(AgentKind::Claude.active_dir_name(), ".claude");
        assert_eq!(AgentKind::Codex.active_dir_name(), ".codex");
        assert_eq!(AgentKind::Claude.main_config_file(), "settings.json");
        assert_eq!(AgentKind::Codex.main_config_file(), "config.toml");
        assert_eq!(AgentKind::Codex.active_auth_file(), Some("auth.json"));
        assert_eq!(AgentKind::Claude.active_auth_file(), None);
        assert_eq!(AgentKind::Claude.provider_auth_file(), "auth.json");
        assert_eq!(AgentKind::Claude.agent_config_files(), &["settings.json"]);
        assert_eq!(
            AgentKind::Codex.agent_config_files(),
            &["config.toml", "auth.json"]
        );
    }

    #[test]
    fn build_invocation_no_longer_injects_provider_config() {
        let pass = vec![OsString::from("--model"), OsString::from("opus")];
        let inv = AgentKind::Claude.build_invocation(&pass);
        assert_eq!(inv.agent_cmd, ["claude", "--model", "opus"]);
        assert!(inv.extra_run_args.is_empty());

        let inv = AgentKind::Codex.build_invocation(&[]);
        assert_eq!(inv.agent_cmd, ["codex"]);
        assert!(inv.extra_run_args.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn invocation_preserves_non_utf8_passthrough_arguments() {
        use std::os::unix::ffi::OsStringExt;

        let opaque = OsString::from_vec(vec![b'f', 0x80, b'o']);
        let pass = vec![opaque.clone()];

        let inv = AgentKind::Codex.build_invocation(&pass);

        assert_eq!(inv.agent_cmd, [OsString::from("codex"), opaque]);
    }
}
