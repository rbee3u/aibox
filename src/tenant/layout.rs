use super::host::require_host_home;
use super::identity::{ManagedTenant, Tenant, TenantSelection};
use crate::agent::AgentKind;
use crate::foundation::safe_fs::{ensure_real_dir, real_dir_exists};
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

/// Collection containing all managed Tenant Homes.
pub(crate) const TENANTS_DIR: &str = "tenants";
/// Name of the protected Managed Tenant used when a Run omits `--tenant`.
pub(crate) const DEFAULT_TENANT_NAME: &str = "default";
/// Storage key used for the Host Tenant Named Config catalog outside valid names.
pub(crate) const HOST_STORAGE_KEY: &str = "__host";

/// One Coding Agent selected within a Tenant.
#[derive(Debug, Clone)]
pub(crate) struct TenantAgent {
    tenant: Tenant,
    agent: AgentKind,
    agent_state_dir: PathBuf,
    named_config_catalog_dir: PathBuf,
}

impl ManagedTenant {
    /// Resolve a Managed Tenant without touching the filesystem.
    pub(crate) fn resolve(root: &Path, name: &str) -> Result<Self> {
        let name = super::identity::ManagedTenantName::parse(name)?;
        Ok(Self {
            home_dir: root.join(TENANTS_DIR).join(name.as_str()),
            name,
            root_dir: root.to_path_buf(),
        })
    }

    /// Select one Coding Agent in this Tenant.
    pub(crate) fn for_agent(&self, agent: AgentKind) -> TenantAgent {
        Tenant::Managed(self.clone()).for_agent(agent)
    }

    /// Validated Managed Tenant name.
    pub(crate) fn name(&self) -> &super::identity::ManagedTenantName {
        &self.name
    }

    /// Persistent Home mounted into a Run.
    pub(crate) fn home_dir(&self) -> &Path {
        &self.home_dir
    }
}

impl TenantSelection {
    /// Resolve this identity against the Service's Root and Host Home.
    pub(crate) fn resolve(&self, root: &Path, host_home: &Path) -> Result<Tenant> {
        match self {
            Self::Host => Ok(Tenant::Host {
                home_dir: host_home.to_path_buf(),
                root_dir: root.to_path_buf(),
            }),
            Self::Managed(name) => Ok(Tenant::Managed(ManagedTenant::resolve(
                root,
                name.as_str(),
            )?)),
        }
    }
}

impl Tenant {
    /// Select one Coding Agent in this Tenant.
    pub(crate) fn for_agent(&self, agent: AgentKind) -> TenantAgent {
        let home = self.home_dir().to_path_buf();
        let named_config_catalog_dir = self.root().join(agent.tag()).join(self.storage_key());
        TenantAgent {
            tenant: self.clone(),
            agent,
            agent_state_dir: home.join(agent.state_dir_name()),
            named_config_catalog_dir,
        }
    }

    /// Home containing native Coding Agent state.
    pub(crate) fn home_dir(&self) -> &Path {
        match self {
            Self::Managed(tenant) => &tenant.home_dir,
            Self::Host { home_dir, .. } => home_dir,
        }
    }

    /// Validate the real Host Home. A missing Managed Tenant Home is empty state.
    pub(crate) fn validate_session_home(&self) -> Result<()> {
        if let Self::Host { home_dir, .. } = self {
            require_host_home(home_dir)?;
        }
        Ok(())
    }

    fn root(&self) -> &Path {
        match self {
            Self::Managed(tenant) => &tenant.root_dir,
            Self::Host { root_dir, .. } => root_dir,
        }
    }

    pub(crate) fn storage_key(&self) -> &str {
        match self {
            Self::Managed(tenant) => tenant.name.as_str(),
            Self::Host { .. } => HOST_STORAGE_KEY,
        }
    }
}

impl TenantAgent {
    pub(crate) fn tenant(&self) -> &Tenant {
        &self.tenant
    }

    pub(crate) fn agent(&self) -> AgentKind {
        self.agent
    }

    pub(crate) fn agent_state_dir(&self) -> &Path {
        &self.agent_state_dir
    }

    pub(crate) fn home_dir(&self) -> &Path {
        self.tenant.home_dir()
    }

    pub(crate) fn ensure_named_config_catalog(&self) -> Result<()> {
        match &self.tenant {
            Tenant::Managed(tenant) => tenant.ensure_initialized()?,
            Tenant::Host { home_dir, .. } => require_host_home(home_dir)?,
        }
        ensure_real_dir(self.tenant.root(), "AIBox Root")?;
        ensure_real_dir(
            &self.tenant.root().join(self.agent.tag()),
            "Named Config catalog collection",
        )?;
        ensure_real_dir(&self.named_config_catalog_dir, "Named Config catalog")
    }

    pub(crate) fn ensure_agent_state_dir(&self) -> Result<()> {
        match &self.tenant {
            Tenant::Managed(tenant) => tenant.ensure_initialized()?,
            Tenant::Host { home_dir, .. } => {
                require_host_home(home_dir)?;
                ensure_real_dir(&self.agent_state_dir, "Agent state directory")?;
            }
        }
        if !real_dir_exists(&self.agent_state_dir, "Agent state directory")? {
            bail!(
                "Agent state directory does not exist: {}",
                self.agent_state_dir.display()
            );
        }
        Ok(())
    }

    pub(crate) fn state_file(&self, file_name: &str) -> PathBuf {
        self.agent_state_dir.join(file_name)
    }

    pub(crate) fn named_config_catalog_dir(&self) -> &Path {
        &self.named_config_catalog_dir
    }

    pub(crate) fn named_config_catalog_exists(&self) -> Result<bool> {
        if matches!(&self.tenant, Tenant::Managed(tenant) if !tenant.exists()?) {
            return Ok(false);
        }
        let collection = self.tenant.root().join(self.agent.tag());
        if !real_dir_exists(&collection, "Named Config catalog collection")? {
            return Ok(false);
        }
        real_dir_exists(&self.named_config_catalog_dir, "Named Config catalog")
    }
}

/// Create the Agent state child beneath an already validated or initialized Home.
pub(crate) fn ensure_agent_state(agent: AgentKind, home: &Path) -> Result<()> {
    let agent_dir = home.join(agent.state_dir_name());
    ensure_real_dir(&agent_dir, "Agent state directory")
}
