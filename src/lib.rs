//! CLI application entry point for `aibox`.
//!
//! The crate deliberately exposes only [`main_entry`]; command parsing and
//! orchestration remain private implementation details rather than an
//! embedding API.

#![warn(missing_docs)]
#![warn(clippy::undocumented_unsafe_blocks)]

mod agent;
mod cli;
mod completion;
mod component;
mod config;
mod config_model;
mod control_web;
mod docker;
mod metadata;
mod operation;
mod platform;
mod request;
mod request_assessment;
mod request_console;
mod request_interpretation;
mod request_proxy;
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
#[cfg(test)]
mod testutil;

use agent::AgentKind;
use anyhow::{Context, Result};
use cli::{BuildArgs, Cli, Command, RunArgs, SessionArgs};
use docker::BuildCache;
use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;
use std::process::ExitCode;
use tenant::{ManagedTenant, Tenant, TenantAgent};

enum DockerSource {
    System,
    #[cfg(test)]
    Injected(docker::DockerCli),
}

impl DockerSource {
    fn build(&self, dockerfile: &str, image: &str, cache: BuildCache) -> Result<()> {
        match self {
            Self::System => docker::build_image(dockerfile, image, cache),
            #[cfg(test)]
            Self::Injected(docker) => docker::build_image_with(docker, dockerfile, image, cache),
        }
    }

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

    fn image_override(&self) -> Result<Option<String>> {
        match self {
            Self::System => env_override("AIBOX_IMAGE"),
            #[cfg(test)]
            Self::Injected(context) => {
                env_override_from("AIBOX_IMAGE", context.image_override.as_deref())
            }
        }
    }

    fn docker(&self) -> DockerSource {
        match self {
            Self::System => DockerSource::System,
            #[cfg(test)]
            Self::Injected(context) => DockerSource::Injected(context.docker.clone()),
        }
    }

    fn resolve_tenant(&self, root: &Path, host: bool, name: &str) -> Result<Tenant> {
        match self {
            Self::System => Tenant::resolve(root, host, name),
            #[cfg(test)]
            Self::Injected(context) => {
                Tenant::resolve_with_home(root, host, name, &context.host_home)
            }
        }
    }

    fn resolve_tenant_agent(
        &self,
        agent: AgentKind,
        root: &Path,
        host: bool,
        name: &str,
    ) -> Result<TenantAgent> {
        match self {
            Self::System => TenantAgent::resolve(agent, root, host, name),
            #[cfg(test)]
            Self::Injected(context) => {
                TenantAgent::resolve_with_home(agent, root, host, name, &context.host_home)
            }
        }
    }

    fn dispatch_component(&self, args: &cli::ComponentArgs) -> Result<i32> {
        match self {
            Self::System => component::dispatch(args),
            #[cfg(test)]
            Self::Injected(context) => {
                let image_override = self.image_override()?;
                component::dispatch_with(
                    args,
                    &context.root,
                    &context.host_home,
                    image_override.as_deref(),
                    &context.docker,
                )
            }
        }
    }

    fn propagate_auth(&self, root: &Path) -> Result<i32> {
        match self {
            Self::System => config::propagate_auth(root),
            #[cfg(test)]
            Self::Injected(context) => config::propagate_auth_from(root, &context.host_home),
        }
    }
}

pub(crate) fn env_override(name: &str) -> Result<Option<String>> {
    env_override_from(name, std::env::var_os(name).as_deref())
}

pub(crate) fn env_override_from(name: &str, value: Option<&OsStr>) -> Result<Option<String>> {
    match value {
        Some(value) if value.is_empty() => anyhow::bail!("{name} is set but empty"),
        Some(value) => Ok(Some(
            value
                .to_str()
                .with_context(|| format!("{name} is not valid UTF-8"))?
                .to_string(),
        )),
        None => Ok(None),
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

pub(crate) fn print_bytes(bytes: &[u8]) -> Result<bool> {
    write_bytes(&mut std::io::stdout().lock(), bytes)
}

fn write_line(out: &mut impl std::io::Write, line: &str) -> Result<bool> {
    if !write_text(out, line)? {
        return Ok(false);
    }
    match out.write_all(b"\n") {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(error) => Err(error).context("write to stdout"),
    }
}

fn write_text(out: &mut impl std::io::Write, text: &str) -> Result<bool> {
    write_bytes(out, text.as_bytes())
}

fn write_bytes(out: &mut impl std::io::Write, bytes: &[u8]) -> Result<bool> {
    match out.write_all(bytes) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(error) => Err(error).context("write to stdout"),
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
            let image_override = context.image_override()?;
            let root = context.root()?;
            run_agent_with(
                args.agent.unwrap_or(AgentKind::Codex),
                &args,
                passthrough,
                &root,
                image_override.as_deref(),
                &context.docker(),
            )
        }
        Command::Serve(args) => {
            reject_passthrough("serve takes no pass-through args", passthrough)?;
            service::dispatch(&args)
        }
        Command::Build(args) => {
            deprecated_command("build");
            reject_passthrough("build takes no pass-through args", passthrough)?;
            let image_override = context.image_override()?;
            run_build_with(&args, image_override.as_deref(), &context.docker())
        }
        Command::Completion(args) => {
            deprecated_command("completion");
            reject_passthrough("completion takes no pass-through args", passthrough)?;
            completion::dispatch(&args)
        }
        Command::Tenant(args) => {
            deprecated_command("tenant");
            reject_passthrough("tenant takes no pass-through args", passthrough)?;
            tenant::dispatch(&context.root()?, &args.command)
        }
        Command::Component(args) => {
            deprecated_command("component");
            reject_passthrough("component takes no pass-through args", passthrough)?;
            context.dispatch_component(&args)
        }
        Command::Config(args) => {
            deprecated_command("config");
            reject_passthrough("config takes no pass-through args", passthrough)?;
            run_config_command(&args, context)
        }
        Command::Session(args) => {
            deprecated_command("session");
            let agent = args.agent.unwrap_or(AgentKind::Codex);
            run_session_command(agent, &args, passthrough, context)
        }
    }
}

fn deprecated_command(command: &str) {
    eprintln!(
        "warning: `aibox {command}` is deprecated; use `aibox serve` and the Console instead"
    );
}

/// Credential Propagation is global, so it rejects the Tenant and Coding Agent
/// selectors that every other `config` subcommand resolves first.
fn run_config_command(args: &cli::ConfigArgs, context: &CommandContext) -> Result<i32> {
    let root = context.root()?;
    if matches!(&args.command, cli::ConfigCommand::PropagateAuth { .. }) {
        if args.tenant.tenant.is_some() {
            anyhow::bail!("config propagate-auth does not accept --tenant");
        }
        if args.agent == Some(AgentKind::Claude) {
            anyhow::bail!("config propagate-auth supports only --agent codex");
        }
        return context.propagate_auth(&root);
    }
    let agent = args.agent.unwrap_or(AgentKind::Codex);
    let selected =
        context.resolve_tenant_agent(agent, &root, args.tenant.host, args.tenant.tenant_name())?;
    config::dispatch(&selected, &args.command)
}

fn run_session_command(
    agent: AgentKind,
    args: &SessionArgs,
    passthrough: &[OsString],
    context: &CommandContext,
) -> Result<i32> {
    reject_passthrough("session takes no pass-through args", passthrough)?;
    let root = context.root()?;
    let tenant = context.resolve_tenant(&root, args.tenant.host, args.tenant.tenant_name())?;
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

fn run_build_with(
    args: &BuildArgs,
    image_override: Option<&str>,
    docker: &DockerSource,
) -> Result<i32> {
    let image = image_for(image_override)?;
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
    docker
        .build(docker::DOCKERFILE, &image, cache)
        .context("build aibox image")?;

    Ok(0)
}

fn run_agent_with(
    agent: AgentKind,
    run: &RunArgs,
    passthrough: &[OsString],
    root: &Path,
    image_override: Option<&str>,
    docker: &DockerSource,
) -> Result<i32> {
    let image = image_for(image_override)?;
    if image_override.is_some() {
        eprintln!(">> image overridden by $AIBOX_IMAGE: {image}");
    }

    let tenant = ManagedTenant::resolve(root, run.tenant_name())?;

    let workspace = runspec::resolve_workspace(run.workspace.as_deref())?;
    let mounts = runspec::resolve_mounts(&run.mount)?;
    runspec::validate_extra_mount_targets(&mounts)?;
    runspec::validate_aibox_mount_sources(&workspace, &mounts, root)?;

    if !docker.image_exists(&image)? {
        anyhow::bail!("{image} is not present locally; build it first from Console Overview");
    }

    tenant.ensure_initialized()?;
    let home_dir = std::fs::canonicalize(&tenant.home_dir)
        .with_context(|| format!("resolve tenant home {}", tenant.home_dir.display()))?;
    runspec::reject_colon_in_bind_source("tenant home", &home_dir)?;

    let agent_command = agent.build_command(passthrough);
    let run_args = runspec::assemble_run_args(&workspace, &home_dir, &mounts);

    docker.run(&run_args, &image, &agent_command)
}

#[cfg(test)]
struct TestCommandContext {
    root: PathBuf,
    host_home: PathBuf,
    image_override: Option<OsString>,
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
