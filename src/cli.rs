//! Command-line surface plus the argv pre-split that keeps pass-through agent
//! arguments away from clap.

use crate::agent::AgentKind;
use clap::{Args, CommandFactory, Parser, Subcommand};
use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::net::SocketAddr;

/// Parsed `aibox` command line, excluding Coding Agent arguments after `--`.
#[derive(Debug, Parser)]
#[command(
    name = "aibox",
    about = "Run Coding Agents and Debug Shells inside a Docker Filesystem Sandbox",
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
    let accepts_agent = command == Some("run");
    if !accepts_agent && command != Some("debug") {
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
            "--agent" if accepts_agent => Some(("--agent", true)),
            "--tenant" => Some(("--tenant", true)),
            token if accepts_agent && token.starts_with("--agent=") => Some(("--agent", false)),
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
    /// Run a Coding Agent inside the AIBox container.
    ///
    /// Pass arguments verbatim after `--`, for example:
    /// `aibox run -- "fix the build"`.
    Run(RunArgs),
    /// Open a Bash shell for a Managed Tenant without starting a Coding Agent.
    Debug(DebugArgs),
    /// Start the local AIBox Console and Request Proxy.
    Console(ConsoleArgs),
}

/// Options for the local AIBox Console.
#[derive(Debug, Args)]
pub struct ConsoleArgs {
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

/// Options for opening a Managed Tenant Debug Shell.
#[derive(Debug, Args)]
pub struct DebugArgs {
    /// Managed Tenant lowercase DNS label (default: `default`).
    #[arg(
        id = "debug-tenant",
        long = "tenant",
        value_name = "TENANT",
        value_parser = parse_tenant
    )]
    pub tenant: Option<String>,
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
#[path = "cli_tests.rs"]
mod tests;
