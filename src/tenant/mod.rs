//! Tenant identity, layout, lifecycle, and Host path discovery facade.

mod environment;
mod host;
mod identity;
mod layout;
mod lifecycle;

pub(crate) use environment::{
    CONTAINER_HOME, TenantEnvironmentCapabilities, build_agent_command, build_debug_command,
};
pub(crate) use host::{aibox_root, host_home};
pub(crate) use identity::{ManagedTenant, Tenant, TenantSelection, is_safe_name, validate_name};
pub(crate) use layout::{DEFAULT_TENANT_NAME, TENANTS_DIR, TenantAgent, ensure_agent_state};
pub(crate) use lifecycle::{delete_tenants, list_tenants};

#[cfg(test)]
#[path = "tenant_tests.rs"]
mod tests;
