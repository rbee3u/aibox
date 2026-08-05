//! CLI application entry point for `aibox`.
//!
//! The crate deliberately exposes only [`main_entry`]; command parsing and
//! orchestration remain private implementation details rather than an
//! embedding API.

#![warn(missing_docs)]

mod agent;
mod cli;
mod completion;
mod component;
mod creds;
mod docker;
mod platform;
mod profile;
mod profile_model;
mod profile_state;
mod runspec;
mod session;
mod session_claude;
mod session_codex;
mod tenant;
#[cfg(test)]
mod testutil;

#[cfg(test)]
pub(crate) fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

use agent::AgentKind;
use anyhow::{Context, Result};
use cli::{BuildArgs, Cli, Command, RunArgs, SessionArgs};
use docker::BuildCache;
use std::ffi::OsString;
use std::process::ExitCode;
use tenant::{ManagedTenant, Tenant, TenantAgent};

pub(crate) fn env_override(name: &str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) if value.is_empty() => anyhow::bail!("{name} is set but empty"),
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("{name} is not valid UTF-8")
        }
    }
}

/// Run the `aibox` command-line application and return its process status.
pub fn main_entry() -> ExitCode {
    completion::handle_env();
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

fn image_for(image_override: Option<&str>) -> Result<String> {
    let image = image_override.unwrap_or(docker::IMAGE);
    validate_image_ref(image)?;
    Ok(image.to_string())
}

fn validate_image_ref(image: &str) -> Result<()> {
    if image.is_empty() {
        anyhow::bail!("Docker image reference is empty");
    }
    if image.starts_with('-') {
        anyhow::bail!("Docker image reference must not start with '-': {image:?}");
    }
    if image
        .chars()
        .any(|character| character.is_ascii_control() || character.is_ascii_whitespace())
    {
        anyhow::bail!(
            "Docker image reference must not contain whitespace/control characters: {image:?}"
        );
    }
    Ok(())
}

pub(crate) fn print_line(line: &str) -> Result<bool> {
    write_line(&mut std::io::stdout().lock(), line)
}

pub(crate) fn print_text(text: &str) -> Result<bool> {
    write_text(&mut std::io::stdout().lock(), text)
}

fn write_line(out: &mut impl std::io::Write, line: &str) -> Result<bool> {
    if !write_text(out, line)? {
        return Ok(false);
    }
    match out.write_all(b"\n") {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(e) => Err(e).context("write to stdout"),
    }
}

fn write_text(out: &mut impl std::io::Write, text: &str) -> Result<bool> {
    match out.write_all(text.as_bytes()) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(e) => Err(e).context("write to stdout"),
    }
}

/// Execute one parsed aibox command with UTF-8 agent pass-through arguments.
///
/// Use [`run_os`] when arguments collected from the operating system must be
/// forwarded without requiring UTF-8.
#[cfg(test)]
fn run(cli: Cli, passthrough: Vec<String>) -> Result<i32> {
    let passthrough: Vec<_> = passthrough.into_iter().map(OsString::from).collect();
    run_os(cli, &passthrough)
}

/// Execute one parsed aibox command, preserving opaque operating-system
/// strings after the pass-through boundary.
///
/// `passthrough` must contain only the arguments after the first `--`; they are
/// forwarded unchanged for the `run` command and rejected for other commands.
/// The returned value is the process exit code to expose to the caller.
fn run_os(cli: Cli, passthrough: &[OsString]) -> Result<i32> {
    match cli.command {
        Command::Run(args) => run_agent(args.agent.unwrap_or(AgentKind::Codex), &args, passthrough),
        Command::Build(args) => {
            reject_passthrough("build takes no pass-through args", passthrough)?;
            run_build(&args)
        }
        Command::Completion(args) => {
            reject_passthrough("completion takes no pass-through args", passthrough)?;
            completion::dispatch(&args)
        }
        Command::Tenant(args) => {
            reject_passthrough("tenant takes no pass-through args", passthrough)?;
            tenant::dispatch(&args.command)
        }
        Command::Component(args) => {
            reject_passthrough("component takes no pass-through args", passthrough)?;
            component::dispatch(&args)
        }
        Command::Profile(args) => {
            let agent = args.agent.unwrap_or(AgentKind::Codex);
            reject_passthrough("profile takes no pass-through args", passthrough)?;
            let root = tenant::aibox_root()?;
            let selected =
                TenantAgent::resolve(agent, &root, args.tenant.host, args.tenant.tenant_name())?;
            profile::dispatch(&selected, &args.command)
        }
        Command::Session(args) => {
            let agent = args.agent.unwrap_or(AgentKind::Codex);
            run_session_command(agent, &args, passthrough)
        }
    }
}

fn run_session_command(
    agent: AgentKind,
    args: &SessionArgs,
    passthrough: &[OsString],
) -> Result<i32> {
    reject_passthrough("session takes no pass-through args", passthrough)?;
    let root = tenant::aibox_root()?;
    let tenant = Tenant::resolve(&root, args.tenant.host, args.tenant.tenant_name())?;
    tenant.validate_session_home()?;
    let selected = tenant.for_agent(agent);
    session::dispatch(agent, selected.home_dir(), args.command.as_ref())
}

fn reject_passthrough(restriction: &str, passthrough: &[OsString]) -> Result<()> {
    if !passthrough.is_empty() {
        anyhow::bail!("`-- <args>` applies only to a run; {restriction}");
    }
    Ok(())
}

fn run_build(args: &BuildArgs) -> Result<i32> {
    let image_override = env_override("AIBOX_IMAGE")?;
    let image = image_for(image_override.as_deref())?;
    let cache = if args.force {
        BuildCache::NoCachePull
    } else {
        BuildCache::Cached
    };
    if args.force {
        eprintln!(">> building {image} (no cache, pulling fresh Debian base) ...");
    } else {
        eprintln!(">> building {image} (cache enabled) ...");
    }
    docker::build_image(docker::DOCKERFILE, &image, cache).context("build aibox image")?;

    Ok(0)
}

fn run_agent(agent: AgentKind, run: &RunArgs, passthrough: &[OsString]) -> Result<i32> {
    let image_override = env_override("AIBOX_IMAGE")?;
    let image = image_for(image_override.as_deref())?;
    if image_override.is_some() {
        eprintln!(">> image overridden by $AIBOX_IMAGE: {image}");
    }

    let root = tenant::aibox_root()?;
    let tenant = ManagedTenant::resolve(&root, run.tenant_name())?;
    let selected = tenant.for_agent(agent);

    let workspace = runspec::resolve_workspace(run.workspace.as_deref())?;
    let mounts = runspec::resolve_mounts(&run.mount)?;
    runspec::validate_extra_mount_targets(&mounts)?;
    runspec::validate_aibox_mount_sources(&workspace, &mounts, &root)?;

    if !docker::image_exists(&image)? {
        anyhow::bail!("{image} is not present locally; build it first with `aibox build`");
    }

    tenant.ensure_initialized()?;
    profile::recover_pending(&selected)?;
    match profile::has_divergence(&selected) {
        Ok(true) => eprintln!(
            "!! Active Agent Profile has source or working changes; continuing without reapplying it"
        ),
        Ok(false) => {}
        Err(error) => eprintln!(
            "!! could not inspect Active Agent Profile state; continuing with native Agent Configuration: {error:#}"
        ),
    }
    let home_dir = std::fs::canonicalize(&tenant.home_dir)
        .with_context(|| format!("resolve tenant home {}", tenant.home_dir.display()))?;
    runspec::reject_colon_in_bind_source("tenant home", &home_dir)?;

    let agent_command = agent.build_command(passthrough);
    let run_args = runspec::assemble_run_args(&workspace, &home_dir, &mounts);

    docker::run(&run_args, &image, &agent_command, || {})
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
