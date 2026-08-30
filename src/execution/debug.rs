//! Managed Tenant Debug Shell orchestration.

use super::{DockerSource, canonical_tenant_home, require_runtime_image, tenant_capabilities};
use crate::docker;
use crate::sandbox;
use crate::tenant::{ManagedTenant, build_debug_command};
use anyhow::Result;
use std::path::Path;

pub(crate) struct DebugCommand {
    pub(crate) tenant: String,
}

/// Open a Bash shell in one Managed Tenant Home without a Workspace.
pub(crate) fn debug(command: DebugCommand, root: &Path, docker: &DockerSource) -> Result<i32> {
    let image = docker::IMAGE;
    let tenant = ManagedTenant::resolve(root, &command.tenant)?;

    require_runtime_image(docker, image)?;
    tenant.ensure_initialized()?;
    let home_dir = canonical_tenant_home(&tenant)?;
    let components = tenant_capabilities(&home_dir);

    let run_args = sandbox::assemble_debug_args(&home_dir);
    let command = build_debug_command(components);
    docker.run(&run_args, image, &command)
}
