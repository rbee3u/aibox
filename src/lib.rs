//! CLI application entry point for `aibox`.
//!
//! The crate deliberately exposes only [`main_entry`]; command parsing and
//! orchestration remain private implementation details rather than an
//! embedding API.

#![warn(missing_docs)]
#![warn(clippy::undocumented_unsafe_blocks)]

mod agent;
mod application_error;
mod cli;
mod component;
mod config;
#[cfg(test)]
mod console_contract;
mod docker;
mod execution;
mod foundation;
mod metadata;
mod request;
mod service;
mod session;
mod tenant;
#[cfg(test)]
mod testutil;

use agent::AgentKind;
use anyhow::Result;
use cli::{Cli, Command};
use std::borrow::Cow;
use std::ffi::OsString;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::process::ExitCode;
#[cfg(test)]
use tenant::ManagedTenant;

enum CommandContext {
    System,
    #[cfg(test)]
    Injected(TestCommandContext),
}

impl CommandContext {
    fn root(&self) -> Result<Cow<'_, Path>> {
        match self {
            Self::System => tenant::aibox_root().map(Cow::Owned),
            #[cfg(test)]
            Self::Injected(context) => Ok(Cow::Borrowed(&context.root)),
        }
    }

    fn docker(&self) -> execution::DockerSource {
        match self {
            Self::System => execution::DockerSource::System,
            #[cfg(test)]
            Self::Injected(context) => execution::injected_docker(context.docker.clone()),
        }
    }
}

/// Run the `aibox` command-line application and return its process status.
pub fn main_entry() -> ExitCode {
    let (left, passthrough) = cli::split_passthrough(std::env::args_os().collect());
    let cli = Cli::parse_from(left);
    match run_os(cli, &passthrough) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(error) => {
            eprintln!("!! {error:#}");
            ExitCode::from(1)
        }
    }
}

/// Execute one parsed `aibox` command, preserving opaque operating-system
/// strings after the pass-through boundary.
///
/// `passthrough` must contain only the arguments after the first `--`; they are
/// forwarded unchanged for the `run` command and rejected for other commands.
/// The returned value is the process exit code to expose to the caller.
fn run_os(cli: Cli, passthrough: &[OsString]) -> Result<i32> {
    dispatch_command(cli, passthrough, &CommandContext::System)
}

fn dispatch_command(cli: Cli, passthrough: &[OsString], context: &CommandContext) -> Result<i32> {
    match cli.command {
        Command::Run(args) => {
            let root = context.root()?;
            execution::run(
                args.agent.unwrap_or(AgentKind::Codex),
                &args,
                passthrough,
                &root,
                &context.docker(),
            )
        }
        Command::Debug(args) => {
            reject_passthrough("debug takes no pass-through args", passthrough)?;
            let root = context.root()?;
            execution::debug(&args, &root, &context.docker())
        }
        Command::Console(args) => {
            reject_passthrough("console takes no pass-through args", passthrough)?;
            service::dispatch(&args)
        }
    }
}

fn reject_passthrough(restriction: &str, passthrough: &[OsString]) -> Result<()> {
    if !passthrough.is_empty() {
        anyhow::bail!("`-- <args>` applies only to a run; {restriction}");
    }
    Ok(())
}

#[cfg(test)]
struct TestCommandContext {
    root: PathBuf,
    docker: docker::DockerCli,
}

#[cfg(test)]
fn run_with_context(
    cli: Cli,
    passthrough: &[OsString],
    context: TestCommandContext,
) -> Result<i32> {
    dispatch_command(cli, passthrough, &CommandContext::Injected(context))
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
