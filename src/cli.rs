//! Command-line surface plus the argv pre-split that keeps pass-through agent
//! args away from clap.

use crate::agent::AgentKind;
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};

/// Parsed aibox command line, excluding agent arguments after `--`.
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

    /// Options accepted only when no subcommand is present.
    #[command(flatten)]
    pub run: RunArgs,

    /// Host-side operation, or `None` for an agent run.
    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    /// Parse the unsplit process command line, printing clap errors before
    /// exiting.
    ///
    /// Use this only when agent pass-through arguments cannot be present.
    /// Production callers must collect argv, call [`split_passthrough`], and
    /// pass its left side to [`Self::parse_from`], as the aibox binary does.
    pub fn parse() -> Self {
        Self::parse_from(std::env::args_os())
    }

    /// Parse a pre-split argument iterator, printing clap errors before exiting.
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

    /// Parse arguments without exiting on an error.
    ///
    /// This also rejects repeated `--agent` or `--profile` options within one
    /// command scope, including forms clap would otherwise accept.
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

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Scope {
    Root,
    Config,
    Session,
    OtherCommand,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum ScopedOption {
    Agent,
    Profile,
}

#[derive(Default)]
struct SeenScopedOptions {
    options: BTreeSet<(Scope, ScopedOption)>,
}

impl ScopedOption {
    fn display(self) -> &'static str {
        match self {
            ScopedOption::Agent => "--agent",
            ScopedOption::Profile => "--profile",
        }
    }
}

impl SeenScopedOptions {
    fn record(&mut self, scope: Scope, option: ScopedOption) -> Result<(), clap::Error> {
        if scope == Scope::OtherCommand {
            return Ok(());
        }
        if self.options.insert((scope, option)) {
            Ok(())
        } else {
            Err(duplicate_scoped_option_error(option.display()))
        }
    }
}

fn scoped_option(token: &str) -> Option<(ScopedOption, bool)> {
    match token {
        "--agent" => Some((ScopedOption::Agent, true)),
        "--profile" | "-p" => Some((ScopedOption::Profile, true)),
        _ if token.starts_with("--agent=") => Some((ScopedOption::Agent, false)),
        _ if token.starts_with("--profile=")
            || (token.starts_with("-p") && !token.starts_with("--")) =>
        {
            Some((ScopedOption::Profile, false))
        }
        _ => None,
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
    let mut seen = SeenScopedOptions::default();
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

        if let Some((option, takes_value)) = scoped_option(token) {
            seen.record(scope, option)?;
            skip_next_value = takes_value;
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
        "build" | "profile" | "completion" => Some(Scope::OtherCommand),
        _ => None,
    }
}

fn takes_value(token: &str) -> bool {
    matches!(
        token,
        "--work" | "-w" | "--mount" | "-m" | "--agent" | "--profile" | "-p"
    )
}

/// Options for launching an agent in Docker.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Ordinary profile name (default: `default`).
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
    /// Selected ordinary profile, defaulting to `default`.
    pub fn profile_name(&self) -> &str {
        self.profile.as_deref().unwrap_or("default")
    }
}

/// Top-level host-side subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Build the aibox Docker image.
    Build(BuildArgs),
    /// Generate a shell completion registration script.
    Completion(CompletionArgs),
    /// Manage shared profile homes.
    Profile(ProfileArgs),
    /// Manage provider configuration overlays.
    Config(ConfigArgs),
    /// Browse this profile's saved chat transcripts (host-side; no container).
    Session(SessionArgs),
}

/// Options for `aibox build`.
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Disable the Docker build cache and pull a fresh Debian base image.
    #[arg(short, long)]
    pub force: bool,
}

/// Arguments for `aibox completion`.
#[derive(Debug, Args)]
pub struct CompletionArgs {
    /// Shell whose dynamic completion registration script to generate.
    #[arg(value_enum)]
    pub shell: CompletionShell,
}

/// Shells with supported aibox dynamic completion adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum CompletionShell {
    /// Bourne Again Shell.
    Bash,
    /// Z shell.
    Zsh,
    /// Friendly Interactive Shell.
    Fish,
}

/// Arguments for profile management.
#[derive(Debug, Args)]
pub struct ProfileArgs {
    /// Profile operation to perform.
    #[command(subcommand)]
    pub command: ProfileCommand,
}

/// Profile-management operations.
#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// List ordinary profiles and the built-in `host` profile.
    List,
    /// Create or initialize an ordinary profile.
    Create {
        /// Profile to create.
        #[arg(value_parser = parse_ordinary_profile)]
        profile: String,
    },
    /// Delete one or more ordinary profiles.
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

/// Agent-scoped arguments for provider configuration management.
#[derive(Debug, Args)]
pub struct ConfigArgs {
    /// Agent whose provider configuration to manage. Omit for Codex.
    #[arg(
        id = "config-agent",
        long = "agent",
        value_name = "AGENT",
        value_enum,
        global = true
    )]
    pub agent: Option<AgentKind>,

    /// Profile name. Use `host` to manage the real host agent configuration.
    #[arg(
        id = "config-profile",
        short = 'p',
        long = "profile",
        value_name = "PROFILE",
        value_parser = parse_profile,
        global = true
    )]
    pub profile: Option<String>,

    /// Provider operation to perform.
    #[command(subcommand)]
    pub command: ConfigCommand,
}

impl ConfigArgs {
    /// Selected profile, defaulting to `default`.
    pub fn profile_name(&self) -> &str {
        self.profile.as_deref().unwrap_or("default")
    }
}

/// Provider configuration operations.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// List providers, marking the last applied one with `*`.
    List,
    /// Print a provider's managed configuration files.
    Get {
        /// Provider to print.
        #[arg(value_parser = parse_provider)]
        provider: String,
    },
    /// Create a provider from the built-in template.
    Create {
        /// Provider to create.
        #[arg(value_parser = parse_provider)]
        provider: String,
    },
    /// Merge a provider into the active agent configuration.
    Apply {
        /// Provider to apply.
        #[arg(value_parser = parse_provider)]
        provider: String,
    },
    /// Open a provider file in `$VISUAL` or `$EDITOR`.
    Edit {
        /// Provider to edit.
        #[arg(value_parser = parse_provider)]
        provider: String,
        /// Edit the auth file. Codex only.
        #[arg(long)]
        auth: bool,
    },
    /// Delete one or more providers.
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

/// Agent-scoped arguments for host-side session browsing.
#[derive(Debug, Args)]
pub struct SessionArgs {
    /// Agent whose sessions to browse. Omit for Codex.
    #[arg(
        id = "session-agent",
        long = "agent",
        value_name = "AGENT",
        value_enum,
        global = true
    )]
    pub agent: Option<AgentKind>,

    /// Profile name. Use `host` to browse real host sessions.
    #[arg(
        id = "session-profile",
        short = 'p',
        long = "profile",
        value_name = "PROFILE",
        value_parser = parse_profile,
        global = true
    )]
    pub profile: Option<String>,

    /// Session operation, or `None` for the default list operation.
    #[command(subcommand)]
    pub command: Option<SessionCommand>,
}

impl SessionArgs {
    /// Selected profile, defaulting to `default`.
    pub fn profile_name(&self) -> &str {
        self.profile.as_deref().unwrap_or("default")
    }
}

/// Saved-session operations.
#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// List sessions, newest first.
    List,
    /// Print the prompts you typed in one session.
    Get {
        /// Session short id or unique prefix.
        id: String,
    },
    /// Delete one or more session transcripts.
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
pub fn split_passthrough<T: AsRef<OsStr>>(argv: Vec<T>) -> (Vec<T>, Vec<T>) {
    match argv.iter().position(|arg| arg.as_ref() == OsStr::new("--")) {
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
    fn split_honors_the_first_boundary_and_preserves_unbounded_argv() {
        let (left, right) = split_passthrough(v(&["aibox", "--exec", "--", "fix", "--", "tests"]));
        assert_eq!(left, v(&["aibox", "--exec"]));
        assert_eq!(right, v(&["fix", "--", "tests"]));

        let argv = v(&["aibox", "--exec", "prompt"]);
        let (left, right) = split_passthrough(argv.clone());
        assert_eq!(left, argv);
        assert!(right.is_empty());

        let (left, right) = split_passthrough(v(&["aibox", "--"]));
        assert_eq!(left, v(&["aibox"]));
        assert!(right.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn split_preserves_non_utf8_passthrough_arguments() {
        use std::os::unix::ffi::OsStringExt;

        let opaque = OsString::from_vec(vec![b'f', 0x80, b'o']);
        let (left, right) = split_passthrough(vec![
            OsString::from("aibox"),
            OsString::from("--"),
            opaque.clone(),
        ]);

        assert_eq!(left, [OsString::from("aibox")]);
        assert_eq!(right, [opaque]);
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
    fn parses_session_options_before_their_positionals() {
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
    }

    #[test]
    fn parses_config_options_before_their_positionals() {
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
    }

    #[test]
    fn parses_profile_options_before_their_positionals() {
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
            &["aibox", "--agent", "claude", "completion", "zsh"][..],
            &["aibox", "-p", "work", "completion", "zsh"][..],
        ] {
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "{argv:?} should reject root run options before a subcommand"
            );
        }

        assert!(Cli::try_parse_from(["aibox", "--force", "build"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "--agent", "claude", "list"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "list", "--agent", "claude"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "completion", "zsh", "--agent", "claude"]).is_err());
    }

    #[test]
    fn scoped_options_reject_duplicates() {
        for argv in [
            &["aibox", "--agent", "claude", "--agent", "claude"][..],
            &["aibox", "-p", "work", "--profile", "work"][..],
            &["aibox", "--profile=work", "-pwork"][..],
            &[
                "aibox", "--agent", "codex", "--work", "config", "--agent", "codex",
            ][..],
            &[
                "aibox", "config", "--agent", "claude", "get", "openai", "--agent", "claude",
            ][..],
            &[
                "aibox",
                "config",
                "--agent=claude",
                "get",
                "openai",
                "--agent",
                "claude",
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

        let completion = help(&["aibox", "completion", "--help"]);
        assert!(!completion.contains("--profile"), "{completion}");
        assert!(!completion.contains("--agent"), "{completion}");
        assert!(completion.contains("bash"), "{completion}");
        assert!(completion.contains("zsh"), "{completion}");
        assert!(completion.contains("fish"), "{completion}");

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
    fn parses_completion_commands() {
        for (name, expected) in [
            ("bash", CompletionShell::Bash),
            ("zsh", CompletionShell::Zsh),
            ("fish", CompletionShell::Fish),
        ] {
            let cli = Cli::try_parse_from(["aibox", "completion", name]).unwrap();
            assert_eq!(cli.agent, None);
            match cli.command.unwrap() {
                Command::Completion(CompletionArgs { shell }) => assert_eq!(shell, expected),
                _ => panic!("expected completion"),
            }
        }

        assert!(Cli::try_parse_from(["aibox", "completion"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "completion", "powershell"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "completion", "nu"]).is_err());
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
