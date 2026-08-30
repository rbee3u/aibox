//! Coding Agent Run orchestration.

use super::{DockerSource, canonical_tenant_home, require_runtime_image, tenant_capabilities};
use crate::agent::AgentKind;
use crate::component;
use crate::docker;
use crate::sandbox;
use crate::tenant::{CONTAINER_HOME, ManagedTenant, build_agent_command};
use anyhow::Result;
use std::ffi::OsString;
use std::path::Path;

pub(crate) struct RunCommand {
    pub(crate) agent: AgentKind,
    pub(crate) tenant: String,
    pub(crate) workspace: Option<String>,
    pub(crate) mounts: Vec<String>,
}

/// Resolve every user-controlled input, then start the Coding Agent.
///
/// The Runtime Image check precedes Tenant initialization so a missing image
/// cannot leave freshly created Tenant state behind.
pub(crate) fn run(
    command: RunCommand,
    passthrough: &[OsString],
    root: &Path,
    docker: &DockerSource,
) -> Result<i32> {
    let image = docker::IMAGE;
    let tenant = ManagedTenant::resolve(root, &command.tenant)?;

    let run_spec = sandbox::RunSpec::resolve(command.workspace.as_deref(), &command.mounts, root)?;

    require_runtime_image(docker, image)?;
    tenant.ensure_initialized()?;
    component::require_agent_component(command.agent, tenant.home_dir())?;
    let home_dir = canonical_tenant_home(&tenant)?;
    let components = tenant_capabilities(&home_dir);

    let invocation = command
        .agent
        .invocation(Path::new(CONTAINER_HOME), passthrough);
    let agent_command = build_agent_command(&invocation, components);
    let run_args = run_spec.assemble_run_args(&home_dir);

    docker.run(&run_args, image, &agent_command)
}
