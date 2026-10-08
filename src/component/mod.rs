//! Optional capabilities derived from native state in a Tenant.
//!
//! Statusline Components can edit native Current Config in a Managed Tenant
//! Home or the Host Home, while runtime Components own Managed Tenant-local
//! executables and SDK directories. There is no Component registry, so
//! inspection derives state directly from native files.

mod catalog;
mod links;
mod model;
mod native;
mod node_agent;
mod python;
mod runtime;
mod rust_go;
mod statusline;
mod updates;

pub(crate) use catalog::{
    inspect_catalog, inspect_tenant_environment_components, require_agent_component,
};
pub(crate) use model::{
    ComponentInspection, ComponentKind, ComponentSpec, ComponentStatus,
    TenantEnvironmentCapabilities, validate_stable_version,
};

pub(crate) use updates::{LatestProvider, LatestSnapshot, OfficialLatestProvider, check_snapshot};

/// Release entries constructed by contract samples and fixture providers.
#[cfg(test)]
pub(crate) use updates::{LatestEntry, LatestEntryState, LatestResult};

use crate::tenant::Tenant;
use anyhow::{Result, bail};

// Native Component files are untrusted and must be bounded before parsing.
use crate::foundation::MAX_NATIVE_CONFIG_BYTES as MAX_CONFIG_BYTES;

/// Install one Component into the selected Tenant's native state.
///
/// `log` streams container output to a Management Operation; statusline
/// Components never start a container and ignore it.
pub(crate) fn install_component(
    selected: &Tenant,
    component: &ComponentSpec,
    log: Option<crate::docker::LogCallback>,
) -> Result<i32> {
    reject_host_runtime_component(selected, component.kind)?;
    match component.kind {
        ComponentKind::ClaudeStatusline => statusline::install_claude_statusline(selected),
        ComponentKind::CodexStatusline => statusline::install_codex_statusline(selected),
        ComponentKind::Node
        | ComponentKind::Codex
        | ComponentKind::Claude
        | ComponentKind::Python
        | ComponentKind::Rust
        | ComponentKind::Go => {
            let Tenant::Managed(tenant) = selected else {
                unreachable!("Host runtime Components are rejected above")
            };
            runtime::install_runtime_component(
                tenant,
                component,
                &crate::docker::DockerCli::system(),
                log,
            )
        }
    }
}

pub(crate) fn remove_component(selected: &Tenant, kind: ComponentKind) -> Result<i32> {
    reject_host_runtime_component(selected, kind)?;
    if !catalog::tenant_home_exists(selected)? {
        if matches!(selected, Tenant::Host { .. }) {
            bail!(
                "Host Home does not exist: {}",
                selected.home_dir().display()
            );
        }
        return Ok(0);
    }
    let status = catalog::inspect(kind, selected.home_dir())?;
    if status == ComponentStatus::NotInstalled {
        return Ok(0);
    }
    if status == ComponentStatus::Unmanaged {
        bail!(
            "{} has unmanaged Component state; refusing to remove foreign files",
            kind.name()
        );
    }
    match kind {
        ComponentKind::Node => node_agent::remove_node(selected.home_dir())?,
        ComponentKind::Codex => node_agent::remove_codex(selected.home_dir())?,
        ComponentKind::Claude => node_agent::remove_claude(selected.home_dir())?,
        ComponentKind::Python => python::remove_python(selected.home_dir())?,
        ComponentKind::ClaudeStatusline => statusline::remove_claude_statusline(selected)?,
        ComponentKind::CodexStatusline => statusline::remove_codex_statusline(selected)?,
        ComponentKind::Rust => rust_go::remove_rust(selected.home_dir())?,
        ComponentKind::Go => rust_go::remove_go(selected.home_dir())?,
    }
    Ok(0)
}

fn reject_host_runtime_component(selected: &Tenant, kind: ComponentKind) -> Result<()> {
    if matches!(selected, Tenant::Host { .. }) && !kind.is_statusline() {
        bail!(
            "{} is unavailable to the Host Tenant; it supports only claude-statusline and codex-statusline",
            kind.name()
        );
    }
    Ok(())
}

#[cfg(test)]
#[path = "component_tests.rs"]
mod tests;
