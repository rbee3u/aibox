//! Transient Run and Debug Shell orchestration.
//!
//! [`run()`] and [`debug()`] own their command shapes; this module holds only
//! the Docker source seam and the preflight steps both share.

mod debug;
mod run;

use crate::component;
use crate::docker;
use crate::sandbox;
use crate::tenant::{ManagedTenant, TenantEnvironmentCapabilities};
use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub(crate) use debug::{DebugCommand, debug};
pub(crate) use run::{RunCommand, run};

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

fn require_runtime_image(docker: &DockerSource, image: &str) -> Result<()> {
    if !docker.image_exists(image)? {
        anyhow::bail!(
            "{image} is not present locally; start `aibox console` and build the Runtime Image from Console Overview"
        );
    }
    Ok(())
}

fn canonical_tenant_home(tenant: &ManagedTenant) -> Result<PathBuf> {
    let home_dir = std::fs::canonicalize(tenant.home_dir())
        .with_context(|| format!("resolve tenant home {}", tenant.home_dir().display()))?;
    sandbox::reject_colon_in_bind_source("tenant home", &home_dir)?;
    Ok(home_dir)
}

/// Snapshot Component-owned environment defaults, reporting inspection
/// failures as warnings so one damaged Component cannot block a launch.
fn tenant_capabilities(home: &Path) -> TenantEnvironmentCapabilities {
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
