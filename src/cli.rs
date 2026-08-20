//! Command-line surface plus the argv pre-split that keeps pass-through agent
//! arguments away from clap.

use crate::agent::AgentKind;
use clap::{Args, CommandFactory, Parser, Subcommand};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::net::SocketAddr;

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
        reject_duplicate_run_selection_options(&args)?;
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

fn reject_duplicate_run_selection_options(args: &[OsString]) -> Result<(), clap::Error> {
    if args.get(1).and_then(|value| value.to_str()) != Some("run") {
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
        let selection = match token {
            "--agent" => Some(("--agent", true)),
            "--tenant" => Some(("--tenant", true)),
            token if token.starts_with("--agent=") => Some(("--agent", false)),
            token if token.starts_with("--tenant=") => Some(("--tenant", false)),
            _ => None,
        };
        if let Some((name, takes_next)) = selection {
            if !seen.insert(name) {
                return Err(clap::Error::raw(
                    clap::error::ErrorKind::ArgumentConflict,
                    format!("{name} must be provided only once in a command scope"),
                ));
            }
            index += usize::from(takes_next);
        }
        index += 1;
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
    /// Start the local aibox Service, Console, and Request Proxy.
    Serve(ServeArgs),
    /// Build the shared aibox Runtime Image.
    Build(BuildArgs),
}

/// Options for the local aibox Service.
#[derive(Debug, Args)]
pub struct ServeArgs {
    /// IP address and port to listen on.
    #[arg(
        long,
        value_name = "IP:PORT",
        default_value = "127.0.0.1:9923",
        value_parser = parse_listen
    )]
    pub listen: SocketAddr,
}

fn parse_listen(value: &str) -> Result<SocketAddr, String> {
    let address: SocketAddr = value.parse().map_err(|_| {
        "expected an IP address and nonzero port, for example 127.0.0.1:9923".to_string()
    })?;
    if address.port() == 0 {
        return Err("listener port must not be 0".to_string());
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
        self.tenant
            .as_deref()
            .unwrap_or(crate::tenant::DEFAULT_TENANT_NAME)
    }
}

/// Options for building the shared Runtime Image.
#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Disable Docker's build cache and pull a fresh Debian base image.
    #[arg(short, long)]
    pub force: bool,
}

fn parse_tenant(value: &str) -> Result<String, String> {
    crate::tenant::validate_name("tenant", value)
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
    fn help_exposes_only_supported_commands() {
        let help = Cli::try_parse_from(["aibox", "--help"]).unwrap_err();
        assert_eq!(help.kind(), ErrorKind::DisplayHelp);
        let help = help.to_string();
        for command in ["run", "serve", "build"] {
            assert!(help.contains(command), "{command}: {help}");
        }
        for command in ["completion", "tenant", "component", "config", "session"] {
            assert!(!help.contains(command), "{command}: {help}");
        }
    }

    #[test]
    fn removed_management_commands_are_unknown() {
        for command in ["completion", "tenant", "component", "config", "session"] {
            assert_parse_error(&["aibox", command], ErrorKind::InvalidSubcommand);
        }
    }

    #[test]
    fn combined_short_options_are_rejected_without_blocking_attached_values() {
        assert_parse_error(&["aibox", "build", "-ff"], ErrorKind::UnknownArgument);
        assert_parse_error(&["aibox", "run", "-xy"], ErrorKind::UnknownArgument);

        Cli::try_parse_from(["aibox", "build", "-f"]).unwrap();
        let cli = Cli::try_parse_from(["aibox", "run", "-w.", "-msrc:/src:ro"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.workspace.as_deref(), Some("."));
        assert_eq!(args.mount, ["src:/src:ro"]);
    }

    #[test]
    fn selection_and_options_stay_in_their_command_scopes() {
        for args in [
            &["aibox", "build", "--agent", "codex"][..],
            &["aibox", "serve", "--tenant", "work"][..],
            &["aibox", "run", "--host"][..],
            &["aibox", "build", "--listen", "127.0.0.1:9000"][..],
        ] {
            assert_parse_error(args, ErrorKind::UnknownArgument);
        }

        Cli::try_parse_from(["aibox", "run", "--agent", "claude", "--tenant", "work"]).unwrap();
        Cli::try_parse_from(["aibox", "serve", "--listen", "0.0.0.0:8080"]).unwrap();
        Cli::try_parse_from(["aibox", "build", "--force"]).unwrap();
    }

    #[test]
    fn duplicate_run_selection_options_are_rejected_before_passthrough() {
        for args in [
            &["aibox", "run", "--tenant", "one", "--tenant=two"][..],
            &["aibox", "run", "--agent=claude", "--agent", "codex"][..],
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
    fn managed_tenant_named_host_remains_runnable() {
        let cli = Cli::try_parse_from(["aibox", "run", "--tenant", "host"]).unwrap();
        let Command::Run(args) = cli.command else {
            panic!("expected run command");
        };
        assert_eq!(args.tenant_name(), "host");
    }

    #[test]
    fn serve_requires_a_nonzero_ip_socket() {
        for value in ["localhost:9923", "127.0.0.1:0"] {
            assert_parse_error(
                &["aibox", "serve", "--listen", value],
                ErrorKind::ValueValidation,
            );
        }
        let cli = Cli::try_parse_from(["aibox", "serve", "--listen", "0.0.0.0:8080"]).unwrap();
        let Command::Serve(args) = cli.command else {
            panic!("expected serve command");
        };
        assert_eq!(args.listen, "0.0.0.0:8080".parse().unwrap());
    }

    #[test]
    fn build_retains_force_mode() {
        let cli = Cli::try_parse_from(["aibox", "build", "--force"]).unwrap();
        let Command::Build(args) = cli.command else {
            panic!("expected build command");
        };
        assert!(args.force);
    }
}
