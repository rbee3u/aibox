//! Transient Run and Debug Shell orchestration plus Docker specification assembly.

pub(crate) mod runspec;

use crate::agent::AgentKind;
use crate::cli::{DebugArgs, RunArgs};
use crate::component;
use crate::docker;
use crate::tenant::ManagedTenant;
use crate::tenant::environment as tenant_environment;
use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::Path;

pub(crate) enum DockerSource {
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

pub(crate) fn run(
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

    require_runtime_image(docker, image)?;
    tenant.ensure_initialized()?;
    component::require_agent_component(agent, &tenant.home_dir)?;
    let home_dir = canonical_tenant_home(&tenant)?;
    let components = tenant_environment_components(&home_dir);

    let invocation = agent.invocation(Path::new(tenant_environment::CONTAINER_HOME), passthrough);
    let agent_command = tenant_environment::build_agent_command(&invocation, components);
    let run_args = runspec::assemble_run_args(&workspace, &home_dir, &mounts);

    docker.run(&run_args, image, &agent_command)
}

pub(crate) fn debug(debug: &DebugArgs, root: &Path, docker: &DockerSource) -> Result<i32> {
    let image = docker::IMAGE;
    let tenant = ManagedTenant::resolve(root, debug.tenant_name())?;

    require_runtime_image(docker, image)?;
    tenant.ensure_initialized()?;
    let home_dir = canonical_tenant_home(&tenant)?;
    let components = tenant_environment_components(&home_dir);

    let run_args = runspec::assemble_debug_args(&home_dir);
    let command = tenant_environment::build_debug_command(components);
    docker.run(&run_args, image, &command)
}

fn require_runtime_image(docker: &DockerSource, image: &str) -> Result<()> {
    if !docker.image_exists(image)? {
        anyhow::bail!(
            "{image} is not present locally; start `aibox console` and build the Runtime Image from Console Overview"
        );
    }
    Ok(())
}

fn canonical_tenant_home(tenant: &ManagedTenant) -> Result<std::path::PathBuf> {
    let home_dir = std::fs::canonicalize(&tenant.home_dir)
        .with_context(|| format!("resolve tenant home {}", tenant.home_dir.display()))?;
    runspec::reject_colon_in_bind_source("tenant home", &home_dir)?;
    Ok(home_dir)
}

fn tenant_environment_components(home: &Path) -> component::InstalledComponentSnapshot {
    let (components, warnings) = component::inspect_tenant_environment_components(home);
    for warning in warnings {
        eprintln!("!! {warning}");
    }
    components
}

#[cfg(test)]
pub(crate) fn injected_docker(docker: docker::DockerCli) -> DockerSource {
    DockerSource::Injected(docker)
}
