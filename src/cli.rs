//! Command-line surface plus the argv pre-split that keeps pass-through agent
//! args away from clap.

use crate::agent::AgentKind;
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "aibox",
    about = "Run coding agents inside a Docker container that is the sandbox boundary",
    long_about = "Run coding agents (Claude Code, OpenAI Codex) inside a Docker container \
                  that is the sandbox boundary.\n\n\
                  Pass args straight to the underlying agent after `--`:\n    \
                  aibox -- \"fix the build\"\n\n\
                  Select Claude for runs/config/session instead of the default Codex agent with \
                  `--agent claude`.",
    version
)]
pub struct Cli {
    /// Agent selector for agent-scoped commands. Run/config/session default to Codex.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    #[command(flatten)]
    pub run: RunArgs,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build the aibox Docker image.
    Build(BuildArgs),
    /// Manage shared profile homes.
    Profile(ProfileArgs),
    /// Manage provider configuration overlays.
    Config(ConfigArgs),
    /// Browse this profile's saved chat transcripts (host-side; no container).
    Session(SessionArgs),
}

#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Disable the Docker build cache and pull a fresh Debian base image.
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct SessionArgs {
    /// Session backend to browse. Omit for Codex.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    #[command(subcommand)]
    pub command: Option<SessionCommand>,
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    List {
        /// Session backend to browse. Omit for Codex.
        #[arg(long, value_enum)]
        agent: Option<AgentKind>,
    },
    Get {
        /// Session short id or unique prefix.
        id: String,
        /// Session backend to browse. Omit for Codex.
        #[arg(long, value_enum)]
        agent: Option<AgentKind>,
    },
    Delete {
        /// Session short id or unique prefix. Accepts many; none means all.
        #[arg(value_name = "ID")]
        ids: Vec<String>,
        /// Delete all sessions explicitly.
        #[arg(long)]
        all: bool,
        /// Skip delete confirmations.
        #[arg(short = 'y', long)]
        yes: bool,
        /// Session backend to browse. Omit for Codex.
        #[arg(long, value_enum)]
        agent: Option<AgentKind>,
    },
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    /// Provider agent to manage. Omit for Codex.
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    List {
        /// Provider agent to manage. Omit for Codex.
        #[arg(long, value_enum)]
        agent: Option<AgentKind>,
    },
    Get {
        #[arg(value_parser = parse_provider)]
        provider: String,
        /// Provider agent to manage. Omit for Codex.
        #[arg(long, value_enum)]
        agent: Option<AgentKind>,
    },
    Create {
        #[arg(value_parser = parse_provider)]
        provider: String,
        /// Provider agent to manage. Omit for Codex.
        #[arg(long, value_enum)]
        agent: Option<AgentKind>,
    },
    Apply {
        #[arg(value_parser = parse_provider)]
        provider: String,
        /// Provider agent to manage. Omit for Codex.
        #[arg(long, value_enum)]
        agent: Option<AgentKind>,
    },
    Edit {
        #[arg(value_parser = parse_provider)]
        provider: String,
        /// Edit the auth file. Codex only.
        #[arg(long)]
        auth: bool,
        /// Provider agent to manage. Omit for Codex.
        #[arg(long, value_enum)]
        agent: Option<AgentKind>,
    },
    Delete {
        /// Provider name. Accepts many; none means all.
        #[arg(value_name = "PROVIDER", value_parser = parse_provider)]
        providers: Vec<String>,
        /// Delete all providers explicitly.
        #[arg(long)]
        all: bool,
        /// Skip delete confirmations.
        #[arg(short, long)]
        yes: bool,
        /// Provider agent to manage. Omit for Codex.
        #[arg(long, value_enum)]
        agent: Option<AgentKind>,
    },
}

#[derive(Debug, Args)]
pub struct ProfileArgs {
    #[command(subcommand)]
    pub command: ProfileCommand,
}

#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    List,
    Create {
        #[arg(value_parser = parse_ordinary_profile)]
        profile: String,
    },
    Delete {
        /// Profile name. Accepts many; none means all.
        #[arg(value_name = "PROFILE", value_parser = parse_ordinary_profile)]
        profiles: Vec<String>,
        /// Delete all profiles explicitly.
        #[arg(long)]
        all: bool,
        /// Skip delete confirmations.
        #[arg(short, long)]
        yes: bool,
    },
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Config profile name. Use `host` only with config/session commands.
    #[arg(short, long, value_parser = parse_profile)]
    pub profile: Option<String>,

    /// Project dir mounted at /work (default: current dir).
    #[arg(short, long)]
    pub work: Option<String>,

    /// Extra bind mount, Docker syntax `host:container[:ro]` (repeatable).
    #[arg(short, long)]
    pub mount: Vec<String>,

    /// Codex only: run headless `codex exec`. Pass the prompt after `--`.
    #[arg(long)]
    pub exec: bool,
}

impl RunArgs {
    pub fn profile_name(&self) -> &str {
        self.profile.as_deref().unwrap_or("default")
    }
}

fn parse_profile(value: &str) -> Result<String, String> {
    crate::profile::validate_name("profile", value)
        .map(|()| value.to_string())
        .map_err(|error| error.to_string())
}

fn parse_ordinary_profile(value: &str) -> Result<String, String> {
    crate::profile::validate_ordinary_profile_name(value)
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
        let (left, right) = split_passthrough(v(&["aibox", "--exec", "--", "fix", "--", "tests"]));
        assert_eq!(left, v(&["aibox", "--exec"]));
        assert_eq!(right, v(&["fix", "--", "tests"]));
    }

    #[test]
    fn parses_default_codex_run() {
        let cli = Cli::try_parse_from(["aibox"]).unwrap();
        assert_eq!(cli.agent, None);
        assert!(cli.command.is_none());
        assert_eq!(cli.run.profile_name(), "default");
    }

    #[test]
    fn parses_claude_run_and_passthrough() {
        let (left, right) = split_passthrough(v(&["aibox", "--agent", "claude", "--", "fix"]));
        let cli = Cli::try_parse_from(left).unwrap();
        assert_eq!(cli.agent, Some(AgentKind::Claude));
        assert!(cli.command.is_none());
        assert_eq!(right, v(&["fix"]));
    }

    #[test]
    fn parses_config_commands() {
        let cli = Cli::try_parse_from([
            "aibox", "--agent", "claude", "-p", "host", "config", "apply", "openai",
        ])
        .unwrap();
        assert_eq!(cli.agent, Some(AgentKind::Claude));
        assert_eq!(cli.run.profile_name(), "host");
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                agent,
                command:
                    ConfigCommand::Apply {
                        provider,
                        agent: apply_agent,
                    },
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*apply_agent, None);
                assert_eq!(provider, "openai");
            }
            _ => panic!("expected config apply"),
        }

        let cli = Cli::try_parse_from([
            "aibox", "config", "--agent", "claude", "edit", "openai", "--auth",
        ])
        .unwrap();
        assert_eq!(cli.agent, None);
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                agent,
                command:
                    ConfigCommand::Edit {
                        provider,
                        auth,
                        agent: edit_agent,
                    },
            }) => {
                assert_eq!(*agent, Some(AgentKind::Claude));
                assert_eq!(*edit_agent, None);
                assert_eq!(provider, "openai");
                assert!(*auth);
            }
            _ => panic!("expected config edit"),
        }

        let cli = Cli::try_parse_from(["aibox", "config", "list", "--agent", "claude"]).unwrap();
        assert_eq!(cli.agent, None);
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                agent,
                command: ConfigCommand::List { agent: list_agent },
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*list_agent, Some(AgentKind::Claude));
            }
            _ => panic!("expected config list"),
        }

        let cli =
            Cli::try_parse_from(["aibox", "config", "get", "openai", "--agent", "claude"]).unwrap();
        assert_eq!(cli.agent, None);
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                agent,
                command:
                    ConfigCommand::Get {
                        provider,
                        agent: get_agent,
                    },
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*get_agent, Some(AgentKind::Claude));
                assert_eq!(provider, "openai");
            }
            _ => panic!("expected config get"),
        }
    }

    #[test]
    fn parses_session_delete() {
        let cli = Cli::try_parse_from([
            "aibox", "--agent", "claude", "-p", "host", "session", "delete", "-y", "abc",
        ])
        .unwrap();
        assert_eq!(cli.agent, Some(AgentKind::Claude));
        match cli.command.as_ref().unwrap() {
            Command::Session(SessionArgs {
                agent,
                command:
                    Some(SessionCommand::Delete {
                        ids,
                        all,
                        yes,
                        agent: delete_agent,
                    }),
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*delete_agent, None);
                assert_eq!(ids, &["abc".to_string()]);
                assert!(!*all);
                assert!(*yes);
            }
            _ => panic!("expected session"),
        }

        let cli = Cli::try_parse_from(["aibox", "session", "delete", "abc", "--agent", "claude"])
            .unwrap();
        assert_eq!(cli.agent, None);
        match cli.command.as_ref().unwrap() {
            Command::Session(SessionArgs {
                agent,
                command:
                    Some(SessionCommand::Delete {
                        ids,
                        all,
                        yes,
                        agent: delete_agent,
                    }),
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*delete_agent, Some(AgentKind::Claude));
                assert_eq!(ids, &["abc".to_string()]);
                assert!(!*all);
                assert!(!*yes);
            }
            _ => panic!("expected session"),
        }

        let cli = Cli::try_parse_from(["aibox", "session", "delete", "abc", "--yes"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Session(SessionArgs {
                agent,
                command:
                    Some(SessionCommand::Delete {
                        ids,
                        all,
                        yes,
                        agent: delete_agent,
                    }),
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*delete_agent, None);
                assert_eq!(ids, &["abc".to_string()]);
                assert!(!*all);
                assert!(*yes);
            }
            _ => panic!("expected session"),
        }

        let cli = Cli::try_parse_from(["aibox", "session", "delete", "--all", "--yes"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Session(SessionArgs {
                command: Some(SessionCommand::Delete { ids, all, yes, .. }),
                ..
            }) => {
                assert!(ids.is_empty());
                assert!(*all);
                assert!(*yes);
            }
            _ => panic!("expected session"),
        }
    }

    #[test]
    fn parses_subcommand_options_before_their_positionals() {
        let cli = Cli::try_parse_from(["aibox", "session", "delete", "--yes"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Session(SessionArgs {
                agent,
                command:
                    Some(SessionCommand::Delete {
                        ids,
                        all,
                        yes,
                        agent: delete_agent,
                    }),
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*delete_agent, None);
                assert!(ids.is_empty());
                assert!(!*all);
                assert!(*yes);
            }
            _ => panic!("expected session"),
        }

        assert!(Cli::try_parse_from(["aibox", "session", "--yes", "delete", "abc"]).is_err());

        let cli = Cli::try_parse_from(["aibox", "session"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Session(SessionArgs {
                agent,
                command: None,
            }) => {
                assert_eq!(*agent, None);
            }
            _ => panic!("expected session"),
        }

        let cli = Cli::try_parse_from(["aibox", "config", "delete", "--yes", "openai"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                command:
                    ConfigCommand::Delete {
                        providers,
                        all,
                        yes,
                        agent,
                    },
                ..
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(providers, &["openai".to_string()]);
                assert!(!*all);
                assert!(*yes);
            }
            _ => panic!("expected config delete"),
        }

        let cli =
            Cli::try_parse_from(["aibox", "config", "delete", "openai", "anthropic", "--yes"])
                .unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                command:
                    ConfigCommand::Delete {
                        providers,
                        all,
                        yes,
                        ..
                    },
                ..
            }) => {
                assert_eq!(providers, &["openai".to_string(), "anthropic".to_string()]);
                assert!(!*all);
                assert!(*yes);
            }
            _ => panic!("expected config delete"),
        }

        let cli = Cli::try_parse_from(["aibox", "config", "delete", "--yes"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                command:
                    ConfigCommand::Delete {
                        providers,
                        all,
                        yes,
                        ..
                    },
                ..
            }) => {
                assert!(providers.is_empty());
                assert!(!*all);
                assert!(*yes);
            }
            _ => panic!("expected config delete"),
        }

        let cli = Cli::try_parse_from(["aibox", "config", "delete", "--all", "--yes"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                command:
                    ConfigCommand::Delete {
                        providers,
                        all,
                        yes,
                        ..
                    },
                ..
            }) => {
                assert!(providers.is_empty());
                assert!(*all);
                assert!(*yes);
            }
            _ => panic!("expected config delete"),
        }

        let cli = Cli::try_parse_from(["aibox", "config", "edit", "--auth", "openai"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                command:
                    ConfigCommand::Edit {
                        provider,
                        auth,
                        agent,
                    },
                ..
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(provider, "openai");
                assert!(*auth);
            }
            _ => panic!("expected config edit"),
        }

        let cli = Cli::try_parse_from(["aibox", "profile", "delete", "--yes", "default"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Profile(ProfileArgs {
                command:
                    ProfileCommand::Delete {
                        profiles, all, yes, ..
                    },
            }) => {
                assert_eq!(profiles, &["default".to_string()]);
                assert!(!*all);
                assert!(*yes);
            }
            _ => panic!("expected profile delete"),
        }

        let cli = Cli::try_parse_from(["aibox", "profile", "delete", "default", "work", "--yes"])
            .unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Profile(ProfileArgs {
                command:
                    ProfileCommand::Delete {
                        profiles, all, yes, ..
                    },
            }) => {
                assert_eq!(profiles, &["default".to_string(), "work".to_string()]);
                assert!(!*all);
                assert!(*yes);
            }
            _ => panic!("expected profile delete"),
        }

        let cli = Cli::try_parse_from(["aibox", "profile", "delete", "--yes"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Profile(ProfileArgs {
                command:
                    ProfileCommand::Delete {
                        profiles, all, yes, ..
                    },
            }) => {
                assert!(profiles.is_empty());
                assert!(!*all);
                assert!(*yes);
            }
            _ => panic!("expected profile delete"),
        }

        let cli = Cli::try_parse_from(["aibox", "profile", "delete", "--all", "--yes"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Profile(ProfileArgs {
                command:
                    ProfileCommand::Delete {
                        profiles, all, yes, ..
                    },
            }) => {
                assert!(profiles.is_empty());
                assert!(*all);
                assert!(*yes);
            }
            _ => panic!("expected profile delete"),
        }
    }

    #[test]
    fn non_global_options_cannot_cross_command_boundaries() {
        assert!(Cli::try_parse_from(["aibox", "session", "-p", "host", "list"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "config", "-p", "host", "list"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "--force", "build"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "--agent", "claude", "list"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "list", "--agent", "claude"]).is_err());
    }

    #[test]
    fn parses_profile_commands() {
        let cli = Cli::try_parse_from(["aibox", "profile", "list"]).unwrap();
        match cli.command.unwrap() {
            Command::Profile(ProfileArgs {
                command: ProfileCommand::List,
            }) => {}
            _ => panic!("expected profile list"),
        }

        let cli = Cli::try_parse_from(["aibox", "profile", "create", "work"]).unwrap();
        match cli.command.unwrap() {
            Command::Profile(ProfileArgs {
                command: ProfileCommand::Create { profile },
            }) => assert_eq!(profile, "work"),
            _ => panic!("expected profile create"),
        }

        let cli = Cli::try_parse_from(["aibox", "profile", "delete", "default", "--yes"]).unwrap();
        match cli.command.unwrap() {
            Command::Profile(ProfileArgs {
                command:
                    ProfileCommand::Delete {
                        profiles, all, yes, ..
                    },
            }) => {
                assert_eq!(profiles, &["default".to_string()]);
                assert!(!all);
                assert!(yes);
            }
            _ => panic!("expected profile delete"),
        }
    }

    #[test]
    fn parses_build_commands() {
        let cli = Cli::try_parse_from(["aibox", "build"]).unwrap();
        assert_eq!(cli.agent, None);
        match cli.command.unwrap() {
            Command::Build(BuildArgs { force }) => assert!(!force),
            _ => panic!("expected build"),
        }

        let cli = Cli::try_parse_from(["aibox", "--agent", "claude", "build"]).unwrap();
        assert_eq!(cli.agent, Some(AgentKind::Claude));
        match cli.command.unwrap() {
            Command::Build(BuildArgs { force }) => assert!(!force),
            _ => panic!("expected build"),
        }

        let cli = Cli::try_parse_from(["aibox", "build", "--force"]).unwrap();
        assert_eq!(cli.agent, None);
        match cli.command.unwrap() {
            Command::Build(BuildArgs { force }) => assert!(force),
            _ => panic!("expected build"),
        }

        let cli = Cli::try_parse_from(["aibox", "build", "-f"]).unwrap();
        assert_eq!(cli.agent, None);
        match cli.command.unwrap() {
            Command::Build(BuildArgs { force }) => assert!(force),
            _ => panic!("expected build"),
        }

        assert!(Cli::try_parse_from(["aibox", "build", "--agent", "claude"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "build", "--force", "--agent", "claude"]).is_err());
    }

    #[test]
    fn old_agent_command_surface_is_gone() {
        assert!(Cli::try_parse_from(["aibox", "codex"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "claude"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "build", "codex"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "-e", "relay"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "--safe"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "refresh"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "--agent", "openai"]).is_err());
    }

    #[test]
    fn invalid_names_are_rejected_by_parser() {
        assert!(Cli::try_parse_from(["aibox", "-p", "bad.name"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "config", "get", "bad.name"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "create", "bad.name"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "create", "host"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "delete", "host"]).is_err());
    }
}
