//! CLI application entry point for `aibox`.
//!
//! The crate deliberately exposes only [`main_entry`]; command parsing and
//! orchestration remain private implementation details rather than an
//! embedding API.

#![warn(missing_docs)]
#![warn(clippy::undocumented_unsafe_blocks)]

mod agent;
mod cli;
mod component;
mod component_updates;
mod config;
mod config_model;
mod control_web;
mod docker;
mod metadata;
mod operation;
mod platform;
mod request;
mod request_assessment;
mod request_interpretation;
mod request_proxy;
mod request_reporter;
mod request_sse;
mod request_store;
mod request_web;
mod runspec;
mod service;
mod session;
mod session_claude;
mod session_codex;
mod sync;
mod tenant;
mod tenant_environment;
#[cfg(test)]
mod testutil;

use agent::AgentKind;
use anyhow::{Context, Result};
use cli::{Cli, Command, DebugArgs, RunArgs};
use std::borrow::Cow;
use std::ffi::OsString;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::process::ExitCode;
use tenant::ManagedTenant;

enum DockerSource {
    System,
    #[cfg(test)]
    Injected(docker::DockerCli),
}

impl DockerSource {
    fn image_exists(&self, image: &str) -> Result<bool> {
        match self {
            Self::System => docker::image_exists(image),
            #[cfg(test)]
            Self::Injected(docker) => docker::image_exists_with(docker, image),
        }
    }

    fn run(&self, run_args: &[String], image: &str, command: &[OsString]) -> Result<i32> {
        match self {
            Self::System => docker::run(run_args, image, command, || {}),
            #[cfg(test)]
            Self::Injected(docker) => docker::run_with(docker, run_args, image, command, || {}),
        }
    }
}

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

    fn docker(&self) -> DockerSource {
        match self {
            Self::System => DockerSource::System,
            #[cfg(test)]
            Self::Injected(context) => DockerSource::Injected(context.docker.clone()),
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

/// Execute one parsed aibox command, preserving opaque operating-system
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
            run_agent_with(
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
            debug_tenant_with(&args, &root, &context.docker())
        }
        Command::Console(args) => {
            reject_passthrough("console takes no pass-through args", passthrough)?;
            service::dispatch(&args)
        }
    }
}

fn debug_tenant_with(debug: &DebugArgs, root: &Path, docker: &DockerSource) -> Result<i32> {
    let image = docker::IMAGE;
    let tenant = ManagedTenant::resolve(root, debug.tenant_name())?;

    if !docker.image_exists(image)? {
        anyhow::bail!(
            "{image} is not present locally; start `aibox console` and build the Runtime Image from Console Overview"
        );
    }

    tenant.ensure_initialized()?;
    let home_dir = std::fs::canonicalize(&tenant.home_dir)
        .with_context(|| format!("resolve tenant home {}", tenant.home_dir.display()))?;
    runspec::reject_colon_in_bind_source("tenant home", &home_dir)?;
    let components = tenant_environment_components(&home_dir);

    let run_args = runspec::assemble_debug_args(&home_dir);
    let command = tenant_environment::build_debug_command(components);
    docker.run(&run_args, image, &command)
}

fn tenant_environment_components(home: &Path) -> component::TenantEnvironmentComponents {
    let (components, warnings) = component::inspect_tenant_environment_components(home);
    for warning in warnings {
        eprintln!("!! {warning}");
    }
    components
}

fn reject_passthrough(restriction: &str, passthrough: &[OsString]) -> Result<()> {
    if !passthrough.is_empty() {
        anyhow::bail!("`-- <args>` applies only to a run; {restriction}");
    }
    Ok(())
}

fn run_agent_with(
    agent: AgentKind,
    run: &RunArgs,
    passthrough: &[OsString],
    root: &Path,
    docker: &DockerSource,
) -> Result<i32> {
    let image = docker::IMAGE;

    let tenant = ManagedTenant::resolve(root, run.tenant_name())?;

    let workspace = runspec::resolve_workspace(run.workspace.as_deref())?;
    let mounts = runspec::resolve_mounts(&run.mount)?;
    runspec::validate_extra_mount_targets(&mounts)?;
    runspec::validate_aibox_mount_sources(&workspace, &mounts, root)?;

    if !docker.image_exists(image)? {
        anyhow::bail!(
            "{image} is not present locally; start `aibox console` and build the Runtime Image from Console Overview"
        );
    }

    tenant.ensure_initialized()?;
    component::require_agent_component(agent, &tenant.home_dir)?;
    let home_dir = std::fs::canonicalize(&tenant.home_dir)
        .with_context(|| format!("resolve tenant home {}", tenant.home_dir.display()))?;
    runspec::reject_colon_in_bind_source("tenant home", &home_dir)?;
    let components = tenant_environment_components(&home_dir);

    let agent_command = agent.build_command(passthrough, components);
    let run_args = runspec::assemble_run_args(&workspace, &home_dir, &mounts);

    docker.run(&run_args, image, &agent_command)
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
