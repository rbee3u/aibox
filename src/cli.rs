//! Command-line surface plus the argv pre-split that keeps pass-through agent
//! args away from clap.

use crate::agent::AgentKind;
use clap::{Args, CommandFactory, Parser, Subcommand};
use std::ffi::{OsStr, OsString};

#[derive(Debug, Parser)]
#[command(
    name = "aibox",
    about = "Run coding agents inside a Docker container that is the sandbox boundary",
    long_about = "Run coding agents (Claude Code, OpenAI Codex) inside a Docker container \
                  that is the sandbox boundary.\n\n\
                  Pass args straight to the underlying agent after `--`:\n    \
                  aibox -- \"fix the build\"\n\n\
                  Select Claude for a run instead of the default Codex agent with \
                  `--agent claude`.",
    args_conflicts_with_subcommands = true,
    version
)]
pub struct Cli {
    /// Agent selector for runs. Omit for Codex.
    #[arg(id = "run-agent", long = "agent", value_name = "AGENT", value_enum)]
    pub agent: Option<AgentKind>,

    #[command(flatten)]
    pub run: RunArgs,

    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    pub fn parse() -> Self {
        Self::parse_from(std::env::args_os())
    }

    pub fn parse_from<I, T>(itr: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        match Self::try_parse_from(itr) {
            Ok(cli) => cli,
            Err(error) => error.exit(),
        }
    }

    pub fn try_parse_from<I, T>(itr: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let args: Vec<OsString> = itr.into_iter().map(Into::into).collect();
        reject_duplicate_scoped_options(&args)?;
        <Self as Parser>::try_parse_from(args)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Scope {
    Root,
    Config,
    Session,
    OtherCommand,
}

#[derive(Clone, Copy)]
enum ScopedOption {
    Agent,
    Profile,
}

#[derive(Default)]
struct ScopedOptionCounts {
    run_agent: bool,
    run_profile: bool,
    config_agent: bool,
    config_profile: bool,
    session_agent: bool,
    session_profile: bool,
}

impl ScopedOptionCounts {
    fn record(
        &mut self,
        scope: Scope,
        option: ScopedOption,
        display: &'static str,
    ) -> Result<(), clap::Error> {
        let seen = match (scope, option) {
            (Scope::Root, ScopedOption::Agent) => &mut self.run_agent,
            (Scope::Root, ScopedOption::Profile) => &mut self.run_profile,
            (Scope::Config, ScopedOption::Agent) => &mut self.config_agent,
            (Scope::Config, ScopedOption::Profile) => &mut self.config_profile,
            (Scope::Session, ScopedOption::Agent) => &mut self.session_agent,
            (Scope::Session, ScopedOption::Profile) => &mut self.session_profile,
            (Scope::OtherCommand, _) => return Ok(()),
        };

        if *seen {
            Err(duplicate_scoped_option_error(display))
        } else {
            *seen = true;
            Ok(())
        }
    }
}

fn duplicate_scoped_option_error(option: &str) -> clap::Error {
    clap::Error::raw(
        clap::error::ErrorKind::ArgumentConflict,
        format!("{option} must be provided only once in a command scope"),
    )
    .with_cmd(&Cli::command())
}

fn reject_duplicate_scoped_options(args: &[OsString]) -> Result<(), clap::Error> {
    let mut scope = Scope::Root;
    let mut counts = ScopedOptionCounts::default();
    let mut skip_next_value = false;

    for arg in args.iter().skip(1) {
        if skip_next_value {
            skip_next_value = false;
            continue;
        }
        if arg == OsStr::new("--") {
            break;
        }

        let Some(token) = arg.to_str() else {
            continue;
        };

        if let Some(next_scope) = subcommand_scope(scope, token) {
            scope = next_scope;
            continue;
        }

        if token == "--agent" {
            counts.record(scope, ScopedOption::Agent, "--agent")?;
            skip_next_value = true;
        } else if token.starts_with("--agent=") {
            counts.record(scope, ScopedOption::Agent, "--agent")?;
        } else if token == "--profile" {
            counts.record(scope, ScopedOption::Profile, "--profile")?;
            skip_next_value = true;
        } else if token.starts_with("--profile=") {
            counts.record(scope, ScopedOption::Profile, "--profile")?;
        } else if token == "-p" {
            counts.record(scope, ScopedOption::Profile, "--profile")?;
            skip_next_value = true;
        } else if token.starts_with("-p") && !token.starts_with("--") {
            counts.record(scope, ScopedOption::Profile, "--profile")?;
        } else if takes_value(token) {
            skip_next_value = true;
        }
    }

    Ok(())
}

fn subcommand_scope(current: Scope, token: &str) -> Option<Scope> {
    if current != Scope::Root || token.starts_with('-') {
        return None;
    }

    match token {
        "config" => Some(Scope::Config),
        "session" => Some(Scope::Session),
        "build" | "profile" => Some(Scope::OtherCommand),
        _ => None,
    }
}

fn takes_value(token: &str) -> bool {
    matches!(
        token,
        "--work" | "-w" | "--mount" | "-m" | "--agent" | "--profile" | "-p"
    )
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
    #[arg(
        id = "session-agent",
        long = "agent",
        value_name = "AGENT",
        value_enum,
        global = true
    )]
    pub agent: Option<AgentKind>,

    /// Config profile name. Use `host` to browse real host sessions.
    #[arg(
        id = "session-profile",
        short = 'p',
        long = "profile",
        value_name = "PROFILE",
        value_parser = parse_profile,
        global = true
    )]
    pub profile: Option<String>,

    #[command(subcommand)]
    pub command: Option<SessionCommand>,
}

impl SessionArgs {
    pub fn profile_name(&self) -> &str {
        self.profile.as_deref().unwrap_or("default")
    }
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    List,
    Get {
        /// Session short id or unique prefix.
        id: String,
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
    },
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    /// Provider agent to manage. Omit for Codex.
    #[arg(
        id = "config-agent",
        long = "agent",
        value_name = "AGENT",
        value_enum,
        global = true
    )]
    pub agent: Option<AgentKind>,

    /// Config profile name. Use `host` to manage real host agent config.
    #[arg(
        id = "config-profile",
        short = 'p',
        long = "profile",
        value_name = "PROFILE",
        value_parser = parse_profile,
        global = true
    )]
    pub profile: Option<String>,

    #[command(subcommand)]
    pub command: ConfigCommand,
}

impl ConfigArgs {
    pub fn profile_name(&self) -> &str {
        self.profile.as_deref().unwrap_or("default")
    }
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
        /// Provider name. Accepts many; none means all.
        #[arg(value_name = "PROVIDER", value_parser = parse_provider)]
        providers: Vec<String>,
        /// Delete all providers explicitly.
        #[arg(long)]
        all: bool,
        /// Skip delete confirmations.
        #[arg(short, long)]
        yes: bool,
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
    /// Profile home name.
    #[arg(
        id = "run-profile",
        short = 'p',
        long = "profile",
        value_name = "PROFILE",
        value_parser = parse_ordinary_profile
    )]
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
    use clap::error::ErrorKind;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn help(args: &[&str]) -> String {
        let err = Cli::try_parse_from(args).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::DisplayHelp);
        err.to_string()
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
        let cli = Cli::try_parse_from(["aibox", "config", "--profile", "host", "apply", "openai"])
            .unwrap();
        assert_eq!(cli.agent, None);
        assert_eq!(cli.run.profile_name(), "default");
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                agent,
                profile,
                command: ConfigCommand::Apply { provider },
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(profile.as_deref(), Some("host"));
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
                profile,
                command: ConfigCommand::Edit { provider, auth },
            }) => {
                assert_eq!(*agent, Some(AgentKind::Claude));
                assert_eq!(*profile, None);
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
                profile,
                command: ConfigCommand::List,
            }) => {
                assert_eq!(*agent, Some(AgentKind::Claude));
                assert_eq!(*profile, None);
            }
            _ => panic!("expected config list"),
        }

        let cli =
            Cli::try_parse_from(["aibox", "config", "get", "openai", "--agent", "claude"]).unwrap();
        assert_eq!(cli.agent, None);
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                agent,
                profile,
                command: ConfigCommand::Get { provider },
            }) => {
                assert_eq!(*agent, Some(AgentKind::Claude));
                assert_eq!(*profile, None);
                assert_eq!(provider, "openai");
            }
            _ => panic!("expected config get"),
        }

        let cli = Cli::try_parse_from([
            "aibox",
            "config",
            "create",
            "openai",
            "--agent",
            "codex",
            "--profile",
            "host",
        ])
        .unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                agent,
                profile,
                command: ConfigCommand::Create { provider },
            }) => {
                assert_eq!(*agent, Some(AgentKind::Codex));
                assert_eq!(profile.as_deref(), Some("host"));
                assert_eq!(provider, "openai");
            }
            _ => panic!("expected config create"),
        }
    }

    #[test]
    fn parses_session_delete() {
        let cli = Cli::try_parse_from([
            "aibox", "session", "--agent", "claude", "-p", "host", "delete", "-y", "abc",
        ])
        .unwrap();
        assert_eq!(cli.agent, None);
        match cli.command.as_ref().unwrap() {
            Command::Session(SessionArgs {
                agent,
                profile,
                command: Some(SessionCommand::Delete { ids, all, yes }),
            }) => {
                assert_eq!(*agent, Some(AgentKind::Claude));
                assert_eq!(profile.as_deref(), Some("host"));
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
                profile,
                command: Some(SessionCommand::Delete { ids, all, yes }),
            }) => {
                assert_eq!(*agent, Some(AgentKind::Claude));
                assert_eq!(*profile, None);
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
                profile,
                command: Some(SessionCommand::Delete { ids, all, yes }),
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*profile, None);
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
                profile,
                command: Some(SessionCommand::Delete { ids, all, yes }),
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*profile, None);
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
                profile,
                command: None,
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*profile, None);
            }
            _ => panic!("expected session"),
        }

        let cli = Cli::try_parse_from(["aibox", "config", "delete", "--yes", "openai"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs {
                agent,
                profile,
                command:
                    ConfigCommand::Delete {
                        providers,
                        all,
                        yes,
                    },
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*profile, None);
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
                agent,
                profile,
                command: ConfigCommand::Edit { provider, auth },
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*profile, None);
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
    fn command_scoped_profile_option_can_cross_config_and_session_boundaries() {
        let cli = Cli::try_parse_from(["aibox", "session", "-p", "host", "list"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Session(SessionArgs { profile, .. }) => {
                assert_eq!(profile.as_deref(), Some("host"));
            }
            _ => panic!("expected session"),
        }

        let cli = Cli::try_parse_from(["aibox", "config", "-p", "host", "list"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs { profile, .. }) => {
                assert_eq!(profile.as_deref(), Some("host"));
            }
            _ => panic!("expected config"),
        }

        let cli =
            Cli::try_parse_from(["aibox", "config", "get", "--profile", "host", "openai"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs { profile, .. }) => {
                assert_eq!(profile.as_deref(), Some("host"));
            }
            _ => panic!("expected config"),
        }

        let cli =
            Cli::try_parse_from(["aibox", "config", "get", "openai", "--profile", "host"]).unwrap();
        match cli.command.as_ref().unwrap() {
            Command::Config(ConfigArgs { profile, .. }) => {
                assert_eq!(profile.as_deref(), Some("host"));
            }
            _ => panic!("expected config"),
        }
    }

    #[test]
    fn root_run_options_cannot_cross_command_boundaries() {
        for argv in [
            &["aibox", "--agent", "claude", "config", "list"][..],
            &["aibox", "-p", "work", "config", "list"][..],
            &["aibox", "--exec", "config", "list"][..],
            &["aibox", "--work", ".", "config", "list"][..],
            &["aibox", "--mount", "/tmp:/tmp", "config", "list"][..],
            &["aibox", "--agent", "claude", "build"][..],
            &["aibox", "-p", "work", "build"][..],
            &["aibox", "--agent", "claude", "profile", "list"][..],
            &["aibox", "-p", "work", "profile", "list"][..],
        ] {
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "{argv:?} should reject root run options before a subcommand"
            );
        }

        assert!(Cli::try_parse_from(["aibox", "--force", "build"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "--agent", "claude", "list"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "list", "--agent", "claude"]).is_err());
    }

    #[test]
    fn scoped_options_reject_duplicates() {
        for argv in [
            &["aibox", "--agent", "claude", "--agent", "claude"][..],
            &["aibox", "-p", "work", "--profile", "work"][..],
            &[
                "aibox", "config", "--agent", "claude", "get", "openai", "--agent", "claude",
            ][..],
            &[
                "aibox",
                "config",
                "--profile",
                "host",
                "get",
                "openai",
                "--profile",
                "host",
            ][..],
            &[
                "aibox", "session", "--agent", "claude", "list", "--agent", "claude",
            ][..],
            &[
                "aibox",
                "session",
                "--profile",
                "host",
                "list",
                "--profile",
                "host",
            ][..],
        ] {
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "{argv:?} should reject duplicate scoped options"
            );
        }
    }

    #[test]
    fn help_shows_only_options_for_the_current_scope() {
        let build = help(&["aibox", "build", "--help"]);
        assert!(!build.contains("--profile"), "{build}");
        assert!(!build.contains("--agent"), "{build}");

        let profile = help(&["aibox", "profile", "--help"]);
        assert!(!profile.contains("--profile"), "{profile}");
        assert!(!profile.contains("--agent"), "{profile}");

        let config_get = help(&["aibox", "config", "get", "--help"]);
        assert!(config_get.contains("--profile"), "{config_get}");
        assert!(config_get.contains("--agent"), "{config_get}");

        let session_delete = help(&["aibox", "session", "delete", "--help"]);
        assert!(session_delete.contains("--profile"), "{session_delete}");
        assert!(session_delete.contains("--agent"), "{session_delete}");
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
        assert!(Cli::try_parse_from(["aibox", "-p", "host"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "config", "-p", "bad.name", "list"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "config", "get", "bad.name"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "create", "bad.name"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "create", "host"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "delete", "host"]).is_err());
    }
}
