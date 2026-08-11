//! Command-line surface plus the argv pre-split that keeps pass-through agent
//! arguments away from clap.

use crate::agent::AgentKind;
use crate::component::ComponentSpec;
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::net::SocketAddr;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SelectionOption {
    Agent,
    Tenant,
}

impl SelectionOption {
    pub(crate) fn parse(token: &str) -> Option<(Self, Option<&str>)> {
        match token {
            "--agent" => Some((Self::Agent, None)),
            "--tenant" => Some((Self::Tenant, None)),
            token => token
                .strip_prefix("--agent=")
                .map(|value| (Self::Agent, Some(value)))
                .or_else(|| {
                    token
                        .strip_prefix("--tenant=")
                        .map(|value| (Self::Tenant, Some(value)))
                }),
        }
    }

    fn long_name(self) -> &'static str {
        match self {
            Self::Agent => "--agent",
            Self::Tenant => "--tenant",
        }
    }
}

/// Parsed aibox command line, excluding Coding Agent arguments after `--`.
#[derive(Debug, Parser)]
#[command(
    name = "aibox",
    about = "Run Coding Agents inside a Docker Filesystem Sandbox",
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
    pub fn try_parse_from<I, T>(itr: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let args: Vec<OsString> = itr.into_iter().map(Into::into).collect();
        reject_short_option_clusters(&args)?;
        reject_duplicate_selection_options(&args)?;
        <Self as Parser>::try_parse_from(args)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ShortOptionToken {
    NotShort,
    BareDash,
    Standalone,
    Value,
    Cluster,
}

fn reject_short_option_clusters(args: &[OsString]) -> Result<(), clap::Error> {
    let mut command = Cli::command();
    command.build();
    let end = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    for index in 1..end {
        let active = active_command_at(&command, args, index);
        if classify_short_option(active, &args[index]) == ShortOptionToken::Cluster {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::UnknownArgument,
                format!(
                    "short options cannot be combined in '{}'; pass each option separately",
                    args[index].to_string_lossy()
                ),
            ));
        }
    }
    Ok(())
}

pub(crate) fn short_option_completion_allowed(
    command: &clap::Command,
    args: &[OsString],
    index: usize,
) -> bool {
    if index >= args.len() || args[..index].iter().any(|arg| arg == "--") {
        return true;
    }
    let active = active_command_at(command, args, index);
    matches!(
        classify_short_option(active, &args[index]),
        ShortOptionToken::NotShort | ShortOptionToken::BareDash | ShortOptionToken::Value
    )
}

fn active_command_at<'a>(
    root: &'a clap::Command,
    args: &[OsString],
    target: usize,
) -> &'a clap::Command {
    let mut active = root;
    let mut takes_next_value = false;
    for token in args.iter().take(target).skip(1) {
        if takes_next_value {
            takes_next_value = false;
            continue;
        }
        let Some(token) = token.to_str() else {
            continue;
        };
        if token == "--" {
            break;
        }
        if let Some(long) = token.strip_prefix("--") {
            let (long, inline) = long
                .split_once('=')
                .map_or((long, false), |(name, _)| (name, true));
            takes_next_value =
                !inline && find_long_option(active, long).is_some_and(option_takes_value);
            continue;
        }
        if token.starts_with('-') {
            if classify_short_option(active, OsStr::new(token)) == ShortOptionToken::Value {
                let value = token.strip_prefix('-').unwrap_or_default();
                takes_next_value = value.chars().count() == 1;
            }
            continue;
        }
        if let Some(subcommand) = active.find_subcommand(token) {
            active = subcommand;
        }
    }
    active
}

fn classify_short_option(command: &clap::Command, token: &OsStr) -> ShortOptionToken {
    let Some(token) = token.to_str() else {
        return ShortOptionToken::NotShort;
    };
    let Some(short) = token.strip_prefix('-') else {
        return ShortOptionToken::NotShort;
    };
    if short.starts_with('-') {
        return ShortOptionToken::NotShort;
    }
    let mut characters = short.chars();
    let Some(first) = characters.next() else {
        return ShortOptionToken::BareDash;
    };
    if find_short_option(command, first).is_some_and(option_takes_value) {
        return ShortOptionToken::Value;
    }
    if characters.next().is_some() {
        ShortOptionToken::Cluster
    } else {
        ShortOptionToken::Standalone
    }
}

fn find_short_option(command: &clap::Command, short: char) -> Option<&clap::Arg> {
    command.get_arguments().find(|arg| {
        arg.get_short_and_visible_aliases()
            .is_some_and(|shorts| shorts.contains(&short))
    })
}

fn find_long_option<'a>(command: &'a clap::Command, long: &str) -> Option<&'a clap::Arg> {
    command.get_arguments().find(|arg| {
        arg.get_long_and_visible_aliases()
            .is_some_and(|longs| longs.contains(&long))
    })
}

fn option_takes_value(arg: &clap::Arg) -> bool {
    arg.get_num_args()
        .is_some_and(|values| values.takes_values())
}

fn reject_duplicate_selection_options(args: &[OsString]) -> Result<(), clap::Error> {
    let command = args.get(1).and_then(|value| value.to_str());
    if !matches!(command, Some("run" | "component" | "config" | "session")) {
        return Ok(());
    }
    let mut seen = BTreeSet::new();
    let mut index = 2;
    while index < args.len() {
        if args[index] == "--" {
            break;
        }
        let Some(token) = args[index].to_str() else {
            index += 1;
            continue;
        };
        let (name, takes_next) = match token {
            "--host" => (Some("--host"), false),
            token => match SelectionOption::parse(token) {
                Some((option, inline_value)) => (Some(option.long_name()), inline_value.is_none()),
                None => (None, false),
            },
        };
        if let Some(name) = name
            && !seen.insert(name)
        {
            return Err(clap::Error::raw(
                clap::error::ErrorKind::ArgumentConflict,
                format!("{name} must be provided only once in a command scope"),
            ));
        }
        index += usize::from(takes_next) + 1;
    }
    Ok(())
}

/// Top-level commands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run a Coding Agent inside the aibox container.
    ///
    /// Pass arguments verbatim after `--`, for example:
    /// `aibox run -- "fix the build"`.
    Run(RunArgs),
    /// Build the aibox Docker image.
    Build(BuildArgs),
    /// Generate a shell completion registration script.
    Completion(CompletionArgs),
    /// Manage aibox-managed Tenants.
    Tenant(TenantArgs),
    /// Manage optional components in a Tenant.
    Component(ComponentArgs),
    /// Manage Named Configs, Current Config, and credential propagation.
    Config(ConfigArgs),
    /// Browse saved Sessions on the host without starting Docker.
    Session(SessionArgs),
    /// Record and inspect HTTP/SSE traffic through a local host-side proxy.
    Traffic(TrafficArgs),
}

/// Options for the host-side Traffic Proxy.
#[derive(Debug, Args)]
pub struct TrafficArgs {
    /// IP address and port to listen on.
    #[arg(
        long,
        value_name = "IP:PORT",
        default_value = "127.0.0.1:9923",
        value_parser = parse_traffic_listen
    )]
    pub listen: SocketAddr,

    /// Allow the proxy listener to accept non-loopback connections.
    #[arg(long)]
    pub allow_remote: bool,
}

fn parse_traffic_listen(value: &str) -> Result<SocketAddr, String> {
    let address: SocketAddr = value.parse().map_err(|_| {
        "expected an IP address and nonzero port, for example 127.0.0.1:9923".to_string()
    })?;
    if address.port() == 0 {
        return Err("Traffic Proxy port must not be 0".to_string());
    }
    Ok(address)
}

/// Options for launching a Coding Agent in Docker.
#[derive(Debug, Args)]
pub struct RunArgs {
    /// Coding Agent to run. Omit for Codex.
    #[arg(id = "run-agent", long = "agent", value_name = "AGENT", value_enum)]
    pub agent: Option<AgentKind>,

    /// Managed Tenant lowercase DNS label (default: `default`).
    #[arg(
        id = "run-tenant",
        long = "tenant",
        value_name = "TENANT",
        value_parser = parse_tenant
    )]
    pub tenant: Option<String>,

    /// Workspace mounted at /workspace (default: current directory).
    #[arg(short, long)]
    pub workspace: Option<String>,

    /// Extra bind mount, Docker syntax `host:container[:ro]` (repeatable).
    #[arg(short, long)]
    pub mount: Vec<String>,
}

impl RunArgs {
    /// Selected Managed Tenant, defaulting to `default`.
    pub fn tenant_name(&self) -> &str {
        self.tenant.as_deref().unwrap_or("default")
    }
}

/// Options for `aibox build`.
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Disable Docker's build cache and pull a fresh Debian base image.
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

/// Arguments for Managed Tenant management.
#[derive(Debug, Args)]
pub struct TenantArgs {
    /// Managed Tenant operation to perform.
    #[command(subcommand)]
    pub command: TenantCommand,
}

/// Managed Tenant management operations.
#[derive(Debug, Subcommand)]
pub enum TenantCommand {
    /// List Managed Tenants.
    List,
    /// Create or repair a Managed Tenant.
    Create {
        /// Managed Tenant lowercase DNS label to create.
        #[arg(value_parser = parse_tenant)]
        tenant: String,
    },
    /// Delete one or more Managed Tenants.
    Delete {
        /// Managed Tenant lowercase DNS labels to delete.
        #[arg(
            value_name = "TENANT",
            value_parser = parse_tenant,
            required_unless_present = "all",
            conflicts_with = "all"
        )]
        tenants: Vec<String>,
        /// Delete every Managed Tenant.
        #[arg(long)]
        all: bool,
        /// Skip delete confirmations.
        #[arg(short, long)]
        yes: bool,
    },
}

/// Tenant Component arguments.
#[derive(Debug, Args)]
pub struct ComponentArgs {
    /// Tenant whose Components to manage.
    #[command(flatten)]
    pub tenant: TenantSelection,

    /// Component operation to perform.
    #[command(subcommand)]
    pub command: ComponentCommand,
}

/// Tenant Component operations.
#[derive(Debug, Subcommand)]
pub enum ComponentCommand {
    /// List available Components and their state in the selected Tenant.
    List,
    /// Install or replace one Component in the selected Tenant.
    Install {
        /// Component name, optionally followed by a stable toolchain version.
        #[arg(value_name = "COMPONENT[@X.Y.Z]")]
        component: ComponentSpec,
    },
    /// Remove one Component from the selected Tenant.
    Remove {
        /// Component to remove.
        #[arg(value_name = "COMPONENT")]
        component: crate::component::ComponentKind,
        /// Skip the removal confirmation.
        #[arg(short, long)]
        yes: bool,
    },
}

/// Mutually exclusive selection of a Managed Tenant or the Host Tenant.
#[derive(Debug, Args)]
pub struct TenantSelection {
    /// Managed Tenant lowercase DNS label (default: `default`).
    #[arg(
        long = "tenant",
        value_name = "TENANT",
        value_parser = parse_tenant,
        global = true,
        conflicts_with = "host"
    )]
    pub tenant: Option<String>,

    /// Operate on the real host Coding Agent state.
    #[arg(long, global = true, conflicts_with = "tenant")]
    pub host: bool,
}

impl TenantSelection {
    /// Selected Managed Tenant name when the Host Tenant was not requested.
    pub fn tenant_name(&self) -> &str {
        self.tenant.as_deref().unwrap_or("default")
    }
}

/// Config management and credential propagation arguments.
#[derive(Debug, Args)]
pub struct ConfigArgs {
    /// Coding Agent whose Named Config catalog and Current Config to manage.
    #[arg(long = "agent", value_name = "AGENT", value_enum, global = true)]
    pub agent: Option<AgentKind>,

    /// Tenant whose Named Config catalog and Current Config to manage.
    #[command(flatten)]
    pub tenant: TenantSelection,

    /// Named Config operation to perform.
    #[command(subcommand)]
    pub command: ConfigCommand,
}

/// Named and Current Config operations.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// List Named Configs in the selected Tenant and Coding Agent.
    List,
    /// Print a Named Config or the Current Config.
    Get {
        /// Named Config lowercase DNS label to print.
        #[arg(
            value_name = "NAME",
            value_parser = parse_config,
            required_unless_present = "current",
            conflicts_with = "current"
        )]
        config: Option<String>,
        /// Print the Current Config instead of a Named Config.
        #[arg(long)]
        current: bool,
    },
    /// Create a Named Config from the built-in native template.
    Create {
        /// Named Config lowercase DNS label to create.
        #[arg(value_parser = parse_config)]
        config: String,
    },
    /// Edit Config files; interactively offer to apply successful Named Config edits.
    Edit {
        /// Named Config lowercase DNS label to edit.
        #[arg(
            value_name = "NAME",
            value_parser = parse_config,
            required_unless_present = "current",
            conflicts_with = "current"
        )]
        config: Option<String>,
        /// Edit the Current Config instead of a Named Config.
        #[arg(long)]
        current: bool,
    },
    /// Delete one or more Named Configs.
    Delete {
        /// Named Config lowercase DNS labels to delete.
        #[arg(
            value_name = "CONFIG",
            value_parser = parse_config,
            required_unless_present = "all",
            conflicts_with = "all"
        )]
        configs: Vec<String>,
        /// Delete every Named Config.
        #[arg(long)]
        all: bool,
        /// Skip delete confirmations.
        #[arg(short, long)]
        yes: bool,
    },
    /// Apply every fixed Config Field to the Current Config once.
    Apply {
        /// Named Config lowercase DNS label to apply.
        #[arg(value_parser = parse_config)]
        config: String,
    },
    /// Propagate newer Host ChatGPT credentials to matching Codex Configs.
    #[command(override_help = PROPAGATE_AUTH_HELP)]
    PropagateAuth {
        /// Explicitly select the Current Config source (the default and only source).
        #[arg(long)]
        current: bool,
    },
}

const PROPAGATE_AUTH_HELP: &str = "\
Propagate newer Host ChatGPT credentials to matching Codex Configs

Usage: aibox config propagate-auth [OPTIONS]

Options:
      --agent codex  Explicitly select the Codex credential source
      --current      Explicitly select the Current Config source
      --host         Explicitly select the Host Tenant source
  -h, --help         Print help
";

/// Agent- and Tenant-scoped Session browsing arguments.
#[derive(Debug, Args)]
pub struct SessionArgs {
    /// Coding Agent whose Sessions to browse. Omit for Codex.
    #[arg(long = "agent", value_name = "AGENT", value_enum, global = true)]
    pub agent: Option<AgentKind>,

    /// Tenant whose Sessions to browse.
    #[command(flatten)]
    pub tenant: TenantSelection,

    /// Session operation, or no operation for `list`.
    #[command(subcommand)]
    pub command: Option<SessionCommand>,
}

/// Saved Session operations.
#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// List Sessions, newest first.
    List,
    /// Print the prompts typed in one Session.
    Get {
        /// Full Session id or unique suffix.
        id: String,
    },
    /// Delete one or more Session transcripts.
    Delete {
        /// Full Session ids or unique suffixes.
        #[arg(
            value_name = "ID",
            required_unless_present = "all",
            conflicts_with = "all"
        )]
        ids: Vec<String>,
        /// Delete every Session transcript.
        #[arg(long)]
        all: bool,
        /// Skip delete confirmations.
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

fn parse_tenant(value: &str) -> Result<String, String> {
    crate::tenant::validate_name("tenant", value)
        .map(|()| value.to_string())
        .map_err(|error| error.to_string())
}

fn parse_config(value: &str) -> Result<String, String> {
    crate::tenant::validate_name("config", value)
        .map(|()| value.to_string())
        .map_err(|error| error.to_string())
}

/// Split argv at the first `--`. The boundary itself is dropped.
pub fn split_passthrough<T: AsRef<OsStr>>(argv: Vec<T>) -> (Vec<T>, Vec<T>) {
    match argv.iter().position(|arg| arg.as_ref() == OsStr::new("--")) {
        Some(index) => {
            let mut left = argv;
            let right = left.split_off(index + 1);
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

    #[track_caller]
    fn assert_parse_error(args: &[&str], expected: ErrorKind) {
        let error = Cli::try_parse_from(args).unwrap_err();
        assert_eq!(error.kind(), expected, "{args:?}: {error}");
    }

    #[test]
    fn passthrough_uses_the_first_boundary() {
        let args = ["aibox", "run", "--tenant", "work", "--", "exec", "--"]
            .map(String::from)
            .to_vec();
        let (left, right) = split_passthrough(args);
        assert_eq!(left, ["aibox", "run", "--tenant", "work"]);
        assert_eq!(right, ["exec", "--"]);
    }

    #[test]
    fn host_tenant_is_distinct_from_managed_tenant_named_host() {
        let cli = Cli::try_parse_from(["aibox", "config", "--tenant", "host", "list"]).unwrap();
        let Command::Config(args) = cli.command else {
            panic!("expected config command");
        };
        assert_eq!(args.tenant.tenant.as_deref(), Some("host"));
        assert!(!args.tenant.host);

        let cli = Cli::try_parse_from(["aibox", "config", "--host", "list"]).unwrap();
        let Command::Config(args) = cli.command else {
            panic!("expected config command");
        };
        assert!(args.tenant.host);
        assert!(args.tenant.tenant.is_none());
    }

    #[test]
    fn tenant_selectors_conflict() {
        let error =
            Cli::try_parse_from(["aibox", "session", "--host", "--tenant", "default", "list"])
                .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    fn session_help_describes_unique_suffix_selection() {
        for command in ["get", "delete"] {
            let help = Cli::try_parse_from(["aibox", "session", command, "--help"]).unwrap_err();
            assert_eq!(help.kind(), ErrorKind::DisplayHelp);
            let help = help.to_string();
            assert!(help.contains("unique suffix"), "{command}: {help}");
            assert!(!help.contains("unique prefix"), "{command}: {help}");
        }
    }

    #[test]
    fn destructive_commands_require_explicit_selections() {
        for args in [
            vec!["aibox", "tenant", "delete"],
            vec!["aibox", "config", "delete"],
            vec!["aibox", "session", "delete"],
        ] {
            let error = Cli::try_parse_from(args).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
        }
    }

    #[test]
    fn destructive_all_selection_conflicts_with_explicit_targets() {
        for args in [
            &["aibox", "tenant", "delete", "work", "--all"][..],
            &["aibox", "config", "delete", "custom", "--all"][..],
            &["aibox", "session", "delete", "session-id", "--all"][..],
        ] {
            let error = Cli::try_parse_from(args).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ArgumentConflict, "{args:?}");
        }
    }

    #[test]
    fn combined_short_options_are_rejected_without_blocking_attached_values() {
        for args in [
            &["aibox", "build", "-ff"][..],
            &["aibox", "tenant", "delete", "work", "-yy"][..],
            &["aibox", "config", "delete", "custom", "-yh"][..],
            &["aibox", "session", "delete", "session-id", "-xy"][..],
        ] {
            let error = Cli::try_parse_from(args).unwrap_err();
            assert_eq!(
                error.kind(),
                ErrorKind::UnknownArgument,
                "{args:?}: {error}"
            );
            assert!(
                error
                    .to_string()
                    .contains("short options cannot be combined"),
                "{args:?}: {error}"
            );
        }

        Cli::try_parse_from(["aibox", "build", "-f"]).unwrap();
        Cli::try_parse_from(["aibox", "tenant", "delete", "work", "-y"]).unwrap();

        let cli = Cli::try_parse_from(["aibox", "run", "-w.", "-msrc:/src:ro"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.workspace.as_deref(), Some("."));
        assert_eq!(args.mount, ["src:/src:ro"]);
    }

    #[test]
    fn selection_and_run_options_are_scoped_to_their_own_commands() {
        for args in [
            &["aibox", "build", "--agent", "codex"][..],
            &["aibox", "completion", "zsh", "--tenant", "work"][..],
            &["aibox", "tenant", "list", "--host"][..],
            &["aibox", "component", "list", "--agent", "claude"][..],
            &["aibox", "config", "list", "--workspace", "."][..],
            &["aibox", "session", "list", "--mount", ".:/data"][..],
        ] {
            assert_parse_error(args, ErrorKind::UnknownArgument);
        }

        Cli::try_parse_from(["aibox", "run", "--agent", "claude", "--tenant", "work"]).unwrap();
        Cli::try_parse_from(["aibox", "config", "list", "--agent", "claude", "--host"]).unwrap();
        Cli::try_parse_from([
            "aibox", "session", "--tenant", "work", "list", "--agent", "claude",
        ])
        .unwrap();
        Cli::try_parse_from(["aibox", "component", "list", "--tenant", "work"]).unwrap();
    }

    #[test]
    fn duplicate_selection_detection_accepts_agent_passthrough_lookalikes() {
        for args in [
            &["aibox", "run", "--tenant", "one", "--tenant=two"][..],
            &["aibox", "config", "--host", "list", "--host"][..],
            &[
                "aibox",
                "session",
                "--agent=claude",
                "list",
                "--agent",
                "codex",
            ][..],
        ] {
            let error = Cli::try_parse_from(args).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::ArgumentConflict, "{args:?}");
        }

        let args = [
            "aibox",
            "run",
            "--tenant",
            "work",
            "--",
            "--tenant",
            "agent-value",
        ];
        let (aibox_args, passthrough) = split_passthrough(args.to_vec());
        Cli::try_parse_from(aibox_args).unwrap();
        assert_eq!(passthrough, ["--tenant", "agent-value"]);
    }

    #[test]
    fn config_apply_and_credential_propagation_are_supported() {
        let cli = Cli::try_parse_from(["aibox", "config", "apply", "custom"]).unwrap();
        let Command::Config(args) = cli.command else {
            panic!("expected config command");
        };
        assert!(matches!(
            args.command,
            ConfigCommand::Apply { config } if config == "custom"
        ));

        for args in [
            &["aibox", "config", "propagate-auth"][..],
            &["aibox", "config", "--host", "propagate-auth", "--current"][..],
            &["aibox", "config", "--agent", "codex", "propagate-auth"][..],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            let Command::Config(args) = cli.command else {
                panic!("expected config command");
            };
            assert!(matches!(args.command, ConfigCommand::PropagateAuth { .. }));
        }

        let help =
            Cli::try_parse_from(["aibox", "config", "propagate-auth", "--help"]).unwrap_err();
        assert_eq!(help.kind(), ErrorKind::DisplayHelp);
        let help = help.to_string();
        assert!(help.contains("--agent codex"), "{help}");
        assert!(!help.contains("--tenant"), "{help}");
        assert!(!help.contains("claude"), "{help}");

        for command in ["activate", "deactivate", "status", "diff", "reconcile"] {
            assert_parse_error(&["aibox", "config", command], ErrorKind::InvalidSubcommand);
        }
        assert_parse_error(&["aibox", "provider", "list"], ErrorKind::InvalidSubcommand);
        assert_parse_error(&["aibox", "profile", "list"], ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn config_get_and_edit_require_exactly_one_target() {
        for command in ["get", "edit"] {
            assert_parse_error(
                &["aibox", "config", command],
                ErrorKind::MissingRequiredArgument,
            );
            assert_parse_error(
                &["aibox", "config", command, "custom", "--current"],
                ErrorKind::ArgumentConflict,
            );
            assert_parse_error(
                &["aibox", "config", command, "custom", "--auth"],
                ErrorKind::UnknownArgument,
            );
        }

        let named = Cli::try_parse_from(["aibox", "config", "get", "custom"]).unwrap();
        let Command::Config(named) = named.command else {
            panic!("expected config command");
        };
        assert!(matches!(
            named.command,
            ConfigCommand::Get {
                config: Some(config),
                current: false
            } if config == "custom"
        ));

        let current = Cli::try_parse_from(["aibox", "config", "edit", "--current"]).unwrap();
        let Command::Config(current) = current.command else {
            panic!("expected config command");
        };
        assert!(matches!(
            current.command,
            ConfigCommand::Edit {
                config: None,
                current: true
            }
        ));

        let help = Cli::try_parse_from(["aibox", "config", "edit", "--help"]).unwrap_err();
        assert_eq!(help.kind(), ErrorKind::DisplayHelp);
        assert!(
            help.to_string()
                .contains("interactively offer to apply successful Named Config edits"),
            "{help}"
        );
    }

    #[test]
    fn run_rejects_host_selector() {
        assert_parse_error(&["aibox", "run", "--host"], ErrorKind::UnknownArgument);
        let cli = Cli::try_parse_from(["aibox", "run", "--tenant", "host"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.tenant_name(), "host");
    }

    #[test]
    fn component_scope_supports_host_statuslines() {
        let cli = Cli::try_parse_from([
            "aibox",
            "component",
            "install",
            "rust@1.90.0",
            "--tenant",
            "work",
        ])
        .unwrap();
        let Command::Component(args) = cli.command else {
            panic!("expected component command");
        };
        assert_eq!(args.tenant.tenant_name(), "work");
        assert!(!args.tenant.host);
        let ComponentCommand::Install { component } = args.command else {
            panic!("expected component install");
        };
        assert_eq!(component.to_string(), "rust@1.90.0");

        let cli = Cli::try_parse_from(["aibox", "component", "--host", "list"]).unwrap();
        let Command::Component(args) = cli.command else {
            panic!("expected component command");
        };
        assert!(args.tenant.host);
        assert_eq!(args.tenant.tenant_name(), "default");
        assert!(matches!(args.command, ComponentCommand::List));

        let cli = Cli::try_parse_from([
            "aibox",
            "component",
            "install",
            "codex-statusline",
            "--host",
        ])
        .unwrap();
        let Command::Component(args) = cli.command else {
            panic!("expected component command");
        };
        assert!(args.tenant.host);

        assert_parse_error(
            &["aibox", "component", "--host", "--tenant", "work", "list"],
            ErrorKind::ArgumentConflict,
        );
        assert_parse_error(
            &["aibox", "component", "--agent", "codex", "list"],
            ErrorKind::UnknownArgument,
        );

        assert_parse_error(
            &["aibox", "component", "remove", "rust", "--discard-changes"],
            ErrorKind::UnknownArgument,
        );
        let cli = Cli::try_parse_from(["aibox", "component", "remove", "rust", "--yes"]).unwrap();
        let Command::Component(args) = cli.command else {
            panic!("expected component command");
        };
        assert!(matches!(
            args.command,
            ComponentCommand::Remove {
                component: crate::component::ComponentKind::Rust,
                yes: true,
            }
        ));
        assert_parse_error(
            &["aibox", "component", "remove", "rust@1.90.0"],
            ErrorKind::ValueValidation,
        );
    }

    #[test]
    fn traffic_has_only_host_listener_options() {
        let cli = Cli::try_parse_from(["aibox", "traffic"]).unwrap();
        let Command::Traffic(args) = cli.command else {
            panic!("expected traffic command");
        };
        assert_eq!(args.listen, "127.0.0.1:9923".parse().unwrap());
        assert!(!args.allow_remote);

        let cli = Cli::try_parse_from([
            "aibox",
            "traffic",
            "--listen",
            "[::1]:8080",
            "--allow-remote",
        ])
        .unwrap();
        let Command::Traffic(args) = cli.command else {
            panic!("expected traffic command");
        };
        assert_eq!(args.listen, "[::1]:8080".parse().unwrap());
        assert!(args.allow_remote);

        for args in [
            &["aibox", "traffic", "--listen", "127.0.0.1:0"][..],
            &["aibox", "traffic", "--listen", "localhost:9923"][..],
        ] {
            assert_parse_error(args, ErrorKind::ValueValidation);
        }
        for args in [
            &["aibox", "traffic", "--agent", "codex"][..],
            &["aibox", "traffic", "--tenant", "work"][..],
            &["aibox", "traffic", "--host"][..],
        ] {
            assert_parse_error(args, ErrorKind::UnknownArgument);
        }
    }
}
