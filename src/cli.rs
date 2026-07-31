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
    subcommand_required = true,
    arg_required_else_help = true,
    version
)]
pub struct Cli {
    /// Operation to perform.
    #[command(subcommand)]
    pub command: Command,
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
    Run,
    Provider,
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
        if matches!(scope, Scope::Root | Scope::OtherCommand) {
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
        "--profile" => Some((ScopedOption::Profile, true)),
        _ if token.starts_with("--agent=") => Some((ScopedOption::Agent, false)),
        _ if token.starts_with("--profile=") => Some((ScopedOption::Profile, false)),
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
        "run" => Some(Scope::Run),
        "provider" => Some(Scope::Provider),
        "session" => Some(Scope::Session),
        "build" | "profile" | "completion" => Some(Scope::OtherCommand),
        _ => None,
    }
}

fn takes_value(token: &str) -> bool {
    matches!(
        token,
        "--work" | "-w" | "--mount" | "-m" | "--agent" | "--profile"
    )
}

/// Options for launching an agent in Docker.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Agent to run. Omit for Codex.
    #[arg(id = "run-agent", long = "agent", value_name = "AGENT", value_enum)]
    pub agent: Option<AgentKind>,

    /// Ordinary profile name (default: `default`).
    #[arg(
        id = "run-profile",
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
}

impl RunArgs {
    /// Selected ordinary profile, defaulting to `default`.
    pub fn profile_name(&self) -> &str {
        self.profile.as_deref().unwrap_or("default")
    }
}

/// Top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run a coding agent inside the aibox container.
    ///
    /// Pass arguments straight to the agent after `--`, for example:
    /// `aibox run -- "fix the build"`.
    Run(RunArgs),
    /// Build the aibox Docker image.
    Build(BuildArgs),
    /// Generate a shell completion registration script.
    Completion(CompletionArgs),
    /// Manage shared profile homes.
    Profile(ProfileArgs),
    /// Manage provider configuration overlays.
    Provider(ProviderArgs),
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
pub struct ProviderArgs {
    /// Agent whose provider configuration to manage. Omit for Codex.
    #[arg(
        id = "provider-agent",
        long = "agent",
        value_name = "AGENT",
        value_enum,
        global = true
    )]
    pub agent: Option<AgentKind>,

    /// Profile name. Use `host` to manage the real host agent configuration.
    #[arg(
        id = "provider-profile",
        long = "profile",
        value_name = "PROFILE",
        value_parser = parse_profile,
        global = true
    )]
    pub profile: Option<String>,

    /// Provider operation to perform.
    #[command(subcommand)]
    pub command: ProviderCommand,
}

impl ProviderArgs {
    /// Selected profile, defaulting to `default`.
    pub fn profile_name(&self) -> &str {
        self.profile.as_deref().unwrap_or("default")
    }
}

/// Provider configuration operations.
#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
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
        /// Full session id or unique prefix.
        id: String,
    },
    /// Delete one or more session transcripts.
    Delete {
        /// Full session id or unique prefix. Accepts many; none means all.
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
        let (left, right) = split_passthrough(v(&[
            "aibox",
            "run",
            "--profile",
            "work",
            "--",
            "exec",
            "fix",
            "--",
            "tests",
        ]));
        assert_eq!(left, v(&["aibox", "run", "--profile", "work"]));
        assert_eq!(right, v(&["exec", "fix", "--", "tests"]));

        let argv = v(&["aibox", "prompt"]);
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
            OsString::from("run"),
            OsString::from("--"),
            opaque.clone(),
        ]);

        assert_eq!(left, [OsString::from("aibox"), OsString::from("run")]);
        assert_eq!(right, [opaque]);
    }

    #[test]
    fn parses_default_codex_run() {
        let cli = Cli::try_parse_from(["aibox", "run"]).unwrap();
        match cli.command {
            Command::Run(args) => {
                assert_eq!(args.agent, None);
                assert_eq!(args.profile_name(), "default");
                assert_eq!(args.work, None);
                assert!(args.mount.is_empty());
            }
            _ => panic!("expected run"),
        }
    }

    #[test]
    fn bare_command_displays_help_as_an_error() {
        let error = Cli::try_parse_from(["aibox"]).unwrap_err();
        assert_eq!(
            error.kind(),
            ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        assert_ne!(error.exit_code(), 0);
        assert!(error.to_string().contains("Usage: aibox <COMMAND>"));
    }

    #[test]
    fn parses_claude_run_and_passthrough() {
        let (left, right) =
            split_passthrough(v(&["aibox", "run", "--agent", "claude", "--", "fix"]));
        let cli = Cli::try_parse_from(left).unwrap();
        match cli.command {
            Command::Run(args) => assert_eq!(args.agent, Some(AgentKind::Claude)),
            _ => panic!("expected run"),
        }
        assert_eq!(right, v(&["fix"]));
    }

    #[test]
    fn parses_provider_commands() {
        let cli =
            Cli::try_parse_from(["aibox", "provider", "--profile", "host", "apply", "openai"])
                .unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs {
                agent,
                profile,
                command: ProviderCommand::Apply { provider },
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(profile.as_deref(), Some("host"));
                assert_eq!(provider, "openai");
            }
            _ => panic!("expected provider apply"),
        }

        let cli = Cli::try_parse_from([
            "aibox", "provider", "--agent", "claude", "edit", "openai", "--auth",
        ])
        .unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs {
                agent,
                profile,
                command: ProviderCommand::Edit { provider, auth },
            }) => {
                assert_eq!(*agent, Some(AgentKind::Claude));
                assert_eq!(*profile, None);
                assert_eq!(provider, "openai");
                assert!(*auth);
            }
            _ => panic!("expected provider edit"),
        }

        let cli = Cli::try_parse_from(["aibox", "provider", "list", "--agent", "claude"]).unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs {
                agent,
                profile,
                command: ProviderCommand::List,
            }) => {
                assert_eq!(*agent, Some(AgentKind::Claude));
                assert_eq!(*profile, None);
            }
            _ => panic!("expected provider list"),
        }

        let cli = Cli::try_parse_from(["aibox", "provider", "get", "openai", "--agent", "claude"])
            .unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs {
                agent,
                profile,
                command: ProviderCommand::Get { provider },
            }) => {
                assert_eq!(*agent, Some(AgentKind::Claude));
                assert_eq!(*profile, None);
                assert_eq!(provider, "openai");
            }
            _ => panic!("expected provider get"),
        }

        let cli = Cli::try_parse_from([
            "aibox",
            "provider",
            "create",
            "openai",
            "--agent",
            "codex",
            "--profile",
            "host",
        ])
        .unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs {
                agent,
                profile,
                command: ProviderCommand::Create { provider },
            }) => {
                assert_eq!(*agent, Some(AgentKind::Codex));
                assert_eq!(profile.as_deref(), Some("host"));
                assert_eq!(provider, "openai");
            }
            _ => panic!("expected provider create"),
        }
    }

    #[test]
    fn parses_session_delete() {
        let cli = Cli::try_parse_from([
            "aibox",
            "session",
            "--agent",
            "claude",
            "--profile",
            "host",
            "delete",
            "-y",
            "abc",
        ])
        .unwrap();
        match &cli.command {
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
        match &cli.command {
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
        match &cli.command {
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
        match &cli.command {
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
        match &cli.command {
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
        match &cli.command {
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
    fn parses_provider_options_before_their_positionals() {
        let cli = Cli::try_parse_from(["aibox", "provider", "delete", "--yes", "openai"]).unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs {
                agent,
                profile,
                command:
                    ProviderCommand::Delete {
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
            _ => panic!("expected provider delete"),
        }

        let cli = Cli::try_parse_from([
            "aibox",
            "provider",
            "delete",
            "openai",
            "anthropic",
            "--yes",
        ])
        .unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs {
                command:
                    ProviderCommand::Delete {
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
            _ => panic!("expected provider delete"),
        }

        let cli = Cli::try_parse_from(["aibox", "provider", "delete", "--yes"]).unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs {
                command:
                    ProviderCommand::Delete {
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
            _ => panic!("expected provider delete"),
        }

        let cli = Cli::try_parse_from(["aibox", "provider", "delete", "--all", "--yes"]).unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs {
                command:
                    ProviderCommand::Delete {
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
            _ => panic!("expected provider delete"),
        }

        let cli = Cli::try_parse_from(["aibox", "provider", "edit", "--auth", "openai"]).unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs {
                agent,
                profile,
                command: ProviderCommand::Edit { provider, auth },
            }) => {
                assert_eq!(*agent, None);
                assert_eq!(*profile, None);
                assert_eq!(provider, "openai");
                assert!(*auth);
            }
            _ => panic!("expected provider edit"),
        }
    }

    #[test]
    fn parses_profile_options_before_their_positionals() {
        let cli = Cli::try_parse_from(["aibox", "profile", "delete", "--yes", "default"]).unwrap();
        match &cli.command {
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
        match &cli.command {
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
        match &cli.command {
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
        match &cli.command {
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
    fn command_scoped_profile_option_can_cross_provider_and_session_boundaries() {
        let cli = Cli::try_parse_from(["aibox", "session", "--profile", "host", "list"]).unwrap();
        match &cli.command {
            Command::Session(SessionArgs { profile, .. }) => {
                assert_eq!(profile.as_deref(), Some("host"));
            }
            _ => panic!("expected session"),
        }

        let cli = Cli::try_parse_from(["aibox", "provider", "--profile", "host", "list"]).unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs { profile, .. }) => {
                assert_eq!(profile.as_deref(), Some("host"));
            }
            _ => panic!("expected provider"),
        }

        let cli = Cli::try_parse_from(["aibox", "provider", "get", "--profile", "host", "openai"])
            .unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs { profile, .. }) => {
                assert_eq!(profile.as_deref(), Some("host"));
            }
            _ => panic!("expected provider"),
        }

        let cli = Cli::try_parse_from(["aibox", "provider", "get", "openai", "--profile", "host"])
            .unwrap();
        match &cli.command {
            Command::Provider(ProviderArgs { profile, .. }) => {
                assert_eq!(profile.as_deref(), Some("host"));
            }
            _ => panic!("expected provider"),
        }
    }

    #[test]
    fn root_rejects_run_options() {
        assert_eq!(
            Cli::try_parse_from(["aibox", "--agent", "claude"])
                .unwrap_err()
                .kind(),
            ErrorKind::UnknownArgument
        );

        for argv in [
            &["aibox", "--agent", "claude", "provider", "list"][..],
            &["aibox", "--profile", "work", "provider", "list"][..],
            &["aibox", "--work", ".", "provider", "list"][..],
            &["aibox", "--mount", "/tmp:/tmp", "provider", "list"][..],
            &["aibox", "--agent", "claude", "build"][..],
            &["aibox", "--profile", "work", "build"][..],
            &["aibox", "--agent", "claude", "profile", "list"][..],
            &["aibox", "--profile", "work", "profile", "list"][..],
            &["aibox", "--agent", "claude", "completion", "zsh"][..],
            &["aibox", "--profile", "work", "completion", "zsh"][..],
        ] {
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "{argv:?} should reject run options at the root"
            );
        }

        assert!(Cli::try_parse_from(["aibox", "--exec"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "--force", "build"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "--agent", "claude", "list"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "list", "--agent", "claude"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "completion", "zsh", "--agent", "claude"]).is_err());

        let (left, passthrough) = split_passthrough(v(&["aibox", "--", "fix"]));
        assert!(Cli::try_parse_from(left).is_err());
        assert_eq!(passthrough, v(&["fix"]));
    }

    #[test]
    fn scoped_options_reject_duplicates() {
        for argv in [
            &["aibox", "run", "--agent", "claude", "--agent", "claude"][..],
            &["aibox", "run", "--profile", "work", "--profile", "work"][..],
            &["aibox", "run", "--profile=work", "--profile=work"][..],
            &[
                "aibox", "run", "--agent", "codex", "--work", "provider", "--agent", "codex",
            ][..],
            &[
                "aibox", "provider", "--agent", "claude", "get", "openai", "--agent", "claude",
            ][..],
            &[
                "aibox",
                "provider",
                "--agent=claude",
                "get",
                "openai",
                "--agent",
                "claude",
            ][..],
            &[
                "aibox",
                "provider",
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
        let root = help(&["aibox", "--help"]);
        assert!(root.contains("run"), "{root}");
        assert!(!root.contains("--profile"), "{root}");
        assert!(!root.contains("--agent"), "{root}");
        assert!(!root.contains("--work"), "{root}");
        assert!(!root.contains("--mount"), "{root}");

        let run = help(&["aibox", "run", "--help"]);
        assert!(run.contains("--profile"), "{run}");
        assert!(run.contains("--agent"), "{run}");
        assert!(run.contains("--work"), "{run}");
        assert!(run.contains("--mount"), "{run}");

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

        let provider_get = help(&["aibox", "provider", "get", "--help"]);
        assert!(provider_get.contains("--profile"), "{provider_get}");
        assert!(provider_get.contains("--agent"), "{provider_get}");

        let session_delete = help(&["aibox", "session", "delete", "--help"]);
        assert!(session_delete.contains("--profile"), "{session_delete}");
        assert!(session_delete.contains("--agent"), "{session_delete}");
    }

    #[test]
    fn parses_profile_commands() {
        let cli = Cli::try_parse_from(["aibox", "profile", "list"]).unwrap();
        match cli.command {
            Command::Profile(ProfileArgs {
                command: ProfileCommand::List,
            }) => {}
            _ => panic!("expected profile list"),
        }

        let cli = Cli::try_parse_from(["aibox", "profile", "create", "work"]).unwrap();
        match cli.command {
            Command::Profile(ProfileArgs {
                command: ProfileCommand::Create { profile },
            }) => assert_eq!(profile, "work"),
            _ => panic!("expected profile create"),
        }

        let cli = Cli::try_parse_from(["aibox", "profile", "delete", "default", "--yes"]).unwrap();
        match cli.command {
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
        match cli.command {
            Command::Build(BuildArgs { force }) => assert!(!force),
            _ => panic!("expected build"),
        }

        let cli = Cli::try_parse_from(["aibox", "build", "--force"]).unwrap();
        match cli.command {
            Command::Build(BuildArgs { force }) => assert!(force),
            _ => panic!("expected build"),
        }

        let cli = Cli::try_parse_from(["aibox", "build", "-f"]).unwrap();
        match cli.command {
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
            match cli.command {
                Command::Completion(CompletionArgs { shell }) => assert_eq!(shell, expected),
                _ => panic!("expected completion"),
            }
        }

        assert!(Cli::try_parse_from(["aibox", "completion"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "completion", "powershell"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "completion", "nu"]).is_err());
    }

    #[test]
    fn invalid_names_are_rejected_by_parser() {
        assert!(Cli::try_parse_from(["aibox", "run", "--profile", "bad.name"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "run", "--profile", "host"]).is_err());
        assert!(
            Cli::try_parse_from(["aibox", "provider", "--profile", "bad.name", "list"]).is_err()
        );
        assert!(Cli::try_parse_from(["aibox", "provider", "get", "bad.name"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "create", "bad.name"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "create", "host"]).is_err());
        assert!(Cli::try_parse_from(["aibox", "profile", "delete", "host"]).is_err());
    }
}
