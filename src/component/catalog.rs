//! Component catalog composition and Tenant Environment capability projection.

use super::{ComponentInspection, ComponentKind, ComponentStatus};
use super::{node_agent, python, rust_go, statusline};
use crate::agent::AgentKind;
use crate::tenant::{Tenant, TenantEnvironmentCapabilities};
use anyhow::{Result, bail};
use std::path::Path;

pub(crate) fn inspect_catalog(selected: &Tenant) -> Result<Vec<ComponentInspection>> {
    let exists = tenant_home_exists(selected)?;
    Ok(component_catalog(selected)
        .iter()
        .copied()
        .map(|kind| {
            if !exists {
                return ComponentInspection {
                    kind,
                    status: Some(ComponentStatus::NotInstalled),
                    error: None,
                };
            }
            match inspect(kind, selected.home_dir()) {
                Ok(status) => ComponentInspection {
                    kind,
                    status: Some(status),
                    error: None,
                },
                Err(error) => ComponentInspection {
                    kind,
                    status: None,
                    error: Some(format!("{error:#}")),
                },
            }
        })
        .collect())
}

/// Snapshot healthy Components that own Tenant Environment defaults.
///
/// Inspection failures are returned as warnings rather than failing the
/// caller, so an unrelated damaged Component cannot block a Run or Debug
/// Shell. Recognized non-installed states are intentionally quiet.
pub(crate) fn inspect_tenant_environment_components(
    home: &Path,
) -> (TenantEnvironmentCapabilities, Vec<String>) {
    let mut capabilities = TenantEnvironmentCapabilities::default();
    let mut warnings = Vec::new();
    for (kind, installed) in [
        (ComponentKind::Node, &mut capabilities.node),
        (ComponentKind::Claude, &mut capabilities.claude),
        (ComponentKind::Python, &mut capabilities.python),
        (ComponentKind::Rust, &mut capabilities.rust),
        (ComponentKind::Go, &mut capabilities.go),
    ] {
        match inspect(kind, home) {
            Ok(ComponentStatus::Installed { .. }) => *installed = true,
            Ok(
                ComponentStatus::Modified
                | ComponentStatus::Incomplete
                | ComponentStatus::Unmanaged
                | ComponentStatus::NotInstalled,
            ) => {}
            Err(error) => warnings.push(format!(
                "could not inspect {} Component; skipping its environment defaults: {error}",
                kind.name()
            )),
        }
    }
    (capabilities, warnings)
}

/// Require the selected Coding Agent's Tenant-local executable before a Run.
pub(crate) fn require_agent_component(agent: AgentKind, home: &Path) -> Result<()> {
    let kind = ComponentKind::for_agent(agent);
    match inspect(kind, home)? {
        ComponentStatus::Installed { .. } => Ok(()),
        ComponentStatus::NotInstalled => bail!(
            "{} Component is not installed for this Managed Tenant; install it from Console Tenants > Components",
            kind.name()
        ),
        ComponentStatus::Incomplete => bail!(
            "{} Component is incomplete for this Managed Tenant; repair it from Console Tenants > Components",
            kind.name()
        ),
        ComponentStatus::Unmanaged => bail!(
            "{} has unmanaged executable state for this Managed Tenant; resolve it from Console Tenants > Components",
            kind.name()
        ),
        ComponentStatus::Modified => unreachable!("runtime Components never report modified"),
    }
}

fn component_catalog(selected: &Tenant) -> &'static [ComponentKind] {
    match selected {
        Tenant::Managed(_) => &ComponentKind::ALL,
        Tenant::Host { .. } => &ComponentKind::STATUSLINES,
    }
}

pub(super) fn tenant_home_exists(selected: &Tenant) -> Result<bool> {
    match selected {
        Tenant::Managed(tenant) => tenant.exists(),
        Tenant::Host { home_dir, .. } => {
            crate::foundation::safe_fs::real_dir_exists(home_dir, "Host Home")
        }
    }
}

pub(super) fn inspect(kind: ComponentKind, home: &Path) -> Result<ComponentStatus> {
    match kind {
        ComponentKind::Node => node_agent::inspect_node(home),
        ComponentKind::Codex => node_agent::inspect_codex(home),
        ComponentKind::Claude => node_agent::inspect_claude(home),
        ComponentKind::Python => python::inspect_python(home),
        ComponentKind::ClaudeStatusline => statusline::inspect_claude_statusline(home),
        ComponentKind::CodexStatusline => statusline::inspect_codex_statusline(home),
        ComponentKind::Rust => rust_go::inspect_rust(home),
        ComponentKind::Go => rust_go::inspect_go(home),
    }
}
