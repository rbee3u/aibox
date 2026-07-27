//! Command-line surface plus the argv pre-split that keeps pass-through agent
//! args away from clap.

use crate::agent::AgentKind;
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "aibox",
    about = "Run coding agents inside a Docker container that is the sandbox boundary",
    long_about = "Run coding agents (Claude Code, OpenAI Codex) inside a Docker container \
                  that is the sandbox boundary.\n\n\
                  Pass args straight to the underlying agent after `--`:\n    \
                  aibox codex -- \"fix the build\"",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build aibox Docker image(s).
    Build(BuildArgs),
    /// Run Claude Code (wraps `@anthropic-ai/claude-code`).
    Claude(AgentArgs),
    /// Run OpenAI Codex (wraps `@openai/codex`).
    Codex(AgentArgs),
}

impl Command {
    pub fn agent_kind(&self) -> Option<AgentKind> {
        match self {
            Command::Build(_) => None,
            Command::Claude(_) => Some(AgentKind::Claude),
            Command::Codex(_) => Some(AgentKind::Codex),
        }
    }

    pub fn agent_args(&self) -> Option<&AgentArgs> {
        match self {
            Command::Build(_) => None,
            Command::Claude(args) | Command::Codex(args) => Some(args),
        }
    }
}

#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Which agent image to build. Omit to build both.
    #[arg(value_enum)]
    pub target: Option<BuildTarget>,

    /// Disable the Docker build cache and pull a fresh Debian base image.
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BuildTarget {
    Claude,
    Codex,
}

#[derive(Debug, Args)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub action: Option<Action>,

    #[command(flatten)]
    pub run: RunArgs,
}

#[derive(Debug, Subcommand)]
pub enum Action {
    /// Manage provider configuration overlays.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Browse this profile's saved chat transcripts (host-side; no container).
    Session {
        /// `list` (default), `get`, or `delete`.
        #[arg(default_value = "list")]
        action: String,
        /// Session short id or unique prefix. `delete` accepts many; none means all.
        #[arg(value_name = "ID")]
        ids: Vec<String>,
        /// Skip delete confirmations.
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    List,
    Get {
        #[arg(value_parser = parse_provider)]
        provider: String,
    },
    Create {
        #[arg(value_parser = parse_provider)]
        provider: String,
    },
    Apply {
        #[arg(value_parser = parse_provider)]
        provider: String,
    },
    Edit {
        #[arg(value_parser = parse_provider)]
        provider: String,
        /// Edit the auth file. Codex only.
        #[arg(long)]
        auth: bool,
    },
    Delete {
        #[arg(value_parser = parse_provider)]
        provider: String,
        /// Skip delete confirmation.
        #[arg(short, long)]
        yes: bool,
    },
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Config profile name. Use `host` only with config/session commands.
    #[arg(short, long, default_value = "default", value_parser = parse_profile)]
    pub profile: String,

    /// Project dir mounted at /work (default: current dir).
    #[arg(short, long)]
    pub work: Option<String>,

    /// Extra bind mount, Docker syntax `host:container[:ro]` (repeatable).
    #[arg(short, long)]
    pub mount: Vec<String>,

    /// Keep the agent's normal permission prompts / sandbox instead of bypassing.
    #[arg(long)]
    pub safe: bool,

    /// Codex only: run headless `codex exec`. Pass the prompt after `--`.
    #[arg(long)]
    pub exec: bool,
}

fn parse_profile(value: &str) -> Result<String, String> {
    crate::profile::validate_name("profile", value)
        .map(|()| value.to_string())
        .map_err(|error| error.to_string())
}

fn parse_provider(value: &str) -> Result<String, String> {
    crate::profile::validate_name("provider", value)
        .map(|()| value.to_string())
        .map_err(|error| error.to_string())
}

/// Split argv at the first `--`: everything before is parsed by clap, everything
/// after is pass-through for the agent. The `--` itself is dropped.
pub fn split_passthrough(argv: Vec<String>) -> (Vec<String>, Vec<String>) {
    match argv.iter().position(|a| a == "--") {
        Some(i) => {
            let mut left = argv;
            let right = left.split_off(i + 1);
            left.pop();
            (left, right)
        }
        None => (argv, Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn split_cuts_argv_at_the_first_dashdash_only() {
        let (left, right) =
            split_passthrough(v(&["aibox", "codex", "--exec", "--", "fix", "--", "tests"]));
        assert_eq!(left, v(&["aibox", "codex", "--exec"]));
        assert_eq!(right, v(&["fix", "--", "tests"]));
    }

    #[test]
    fn parses_config_commands() {
        let cli =
            Cli::try_parse_from(["aibox", "codex", "-p", "host", "config", "apply", "openai"])
                .unwrap();
        let args = cli.command.agent_args().unwrap();
        assert_eq!(args.run.profile, "host");
        match args.action.as_ref().unwrap() {
            Action::Config {
                command: ConfigCommand::Apply { provider },
            } => assert_eq!(provider, "openai"),
            _ => panic!("expected config apply"),
        }

        let cli =
            Cli::try_parse_from(["aibox", "codex", "config", "edit", "openai", "--auth"]).unwrap();
        match cli.command.agent_args().unwrap().action.as_ref().unwrap() {
            Action::Config {
                command: ConfigCommand::Edit { provider, auth },
            } => {
                assert_eq!(provider, "openai");
                assert!(*auth);
            }
            _ => panic!("expected config edit"),
        }
    }

    #[test]
    fn parses_session_delete() {
        let cli = Cli::try_parse_from([
            "aibox", "claude", "-p", "host", "session", "delete", "-y", "abc",
        ])
        .unwrap();
        match cli.command.agent_args().unwrap().action.as_ref().unwrap() {
            Action::Session { action, ids, yes } => {
                assert_eq!(action, "delete");
                assert_eq!(ids, &["abc".to_string()]);
                assert!(*yes);
            }
            _ => panic!("expected session"),
        }
    }

    #[test]
    fn old_env_and_refresh_surface_are_gone() {
        assert!(Cli::try_parse_from(["aibox", "codex", "-e", "relay"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "codex", "refresh"]).is_err());
    }

    #[test]
    fn invalid_names_are_rejected_by_parser() {
        assert!(Cli::try_parse_from(["aibox", "codex", "-p", "bad.name"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "codex", "config", "get", "bad.name"]).is_err());
    }
}
