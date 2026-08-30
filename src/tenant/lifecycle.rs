use super::identity::{ManagedTenant, is_safe_name, validate_name};
use super::layout::{DEFAULT_TENANT_NAME, TENANTS_DIR, ensure_agent_state};
use crate::agent::AgentKind;
use crate::foundation::safe_fs::{
    ensure_real_dir, real_dir_exists, real_file_exists, remove_real_dir_if_exists, sync_dir,
};
use anyhow::{Context, Result, bail};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const CREATING_PREFIX: &str = "$creating-";
const DELETING_PREFIX: &str = "$deleting-";
const GITCONFIG: &[u8] = b"[url \"https://github.com/\"]\n    insteadOf = git@github.com:\n    insteadOf = ssh://git@github.com/\n";

impl ManagedTenant {
    /// Create or repair the complete Tenant Home baseline.
    pub(crate) fn ensure_initialized(&self) -> Result<()> {
        ensure_real_dir(&self.root_dir, "AIBox Root")?;
        let tenants = self.root_dir.join(TENANTS_DIR);
        ensure_real_dir(&tenants, "Tenant collection")?;

        if real_dir_exists(&self.home_dir, "Tenant Home")? {
            remove_real_dir_if_exists(&self.deleting_dir(), "stale Tenant deletion")?;
            remove_real_dir_if_exists(&self.creating_dir(), "stale Tenant creation")?;
            return ensure_home_baseline(&self.home_dir);
        }

        remove_real_dir_if_exists(&self.deleting_dir(), "stale Tenant deletion")?;
        for agent in AgentKind::ALL {
            let collection = self.root_dir.join(agent.tag());
            if real_dir_exists(&collection, "Named Config catalog collection")? {
                remove_real_dir_if_exists(
                    &collection.join(self.name.as_str()),
                    "orphaned Named Config catalog",
                )?;
            }
        }

        let creating = self.creating_dir();
        ensure_real_dir(&creating, "Tenant creation staging directory")?;
        ensure_home_baseline(&creating)?;
        match fs::rename(&creating, &self.home_dir) {
            Ok(()) => sync_dir(&tenants),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                remove_real_dir_if_exists(&creating, "Tenant creation staging directory")?;
                ensure_home_baseline(&self.home_dir)
            }
            Err(error) => Err(error).with_context(|| {
                format!(
                    "publish Tenant Home {} from {}",
                    self.home_dir.display(),
                    creating.display()
                )
            }),
        }
    }

    /// Whether the authoritative Tenant Home currently exists.
    pub(crate) fn exists(&self) -> Result<bool> {
        if !real_dir_exists(&self.root_dir.join(TENANTS_DIR), "Tenant collection")? {
            return Ok(false);
        }
        real_dir_exists(&self.home_dir, "Tenant Home")
    }

    fn creating_dir(&self) -> PathBuf {
        self.root_dir
            .join(TENANTS_DIR)
            .join(format!("{CREATING_PREFIX}{}", self.name))
    }

    fn deleting_dir(&self) -> PathBuf {
        self.root_dir
            .join(TENANTS_DIR)
            .join(format!("{DELETING_PREFIX}{}", self.name))
    }
}

/// List completed Managed Tenant names without creating data.
pub(crate) fn list_tenants(root: &Path) -> Result<Vec<String>> {
    let collection = root.join(TENANTS_DIR);
    if !real_dir_exists(&collection, "Tenant collection")? {
        return Ok(Vec::new());
    }
    let entries = match fs::read_dir(&collection) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", collection.display())),
    };
    let mut names = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if is_safe_name(&name) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// Delete selected Managed Tenants, or all completed Tenants when explicit.
pub(crate) fn delete_tenants(root: &Path, tenants: &[String], all: bool) -> Result<()> {
    if all && !tenants.is_empty() {
        bail!("--all cannot be combined with Tenant names");
    }
    if !all && tenants.is_empty() {
        bail!("provide at least one Tenant name or use --all");
    }
    if !all && tenants.iter().any(|name| name == DEFAULT_TENANT_NAME) {
        bail!("Default Managed Tenant 'default' is protected and cannot be deleted");
    }
    let targets = if all {
        let mut targets = list_tenants(root)?;
        for name in interrupted_tenant_names(root)? {
            if !targets.contains(&name) {
                targets.push(name);
            }
        }
        targets.retain(|name| name != DEFAULT_TENANT_NAME);
        targets.sort();
        targets
    } else {
        let mut unique = Vec::new();
        for tenant in tenants {
            validate_name("tenant", tenant)?;
            if !unique.contains(tenant) {
                unique.push(tenant.clone());
            }
        }
        unique
    };
    for tenant in targets {
        delete_one(root, &tenant)?;
    }
    Ok(())
}

fn interrupted_tenant_names(root: &Path) -> Result<Vec<String>> {
    let collection = root.join(TENANTS_DIR);
    if !real_dir_exists(&collection, "Tenant collection")? {
        return Ok(Vec::new());
    }
    let entries =
        fs::read_dir(&collection).with_context(|| format!("read {}", collection.display()))?;
    let mut names = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let staged = name
            .strip_prefix(CREATING_PREFIX)
            .or_else(|| name.strip_prefix(DELETING_PREFIX));
        if let Some(staged) = staged
            && is_safe_name(staged)
        {
            names.push(staged.to_string());
        }
    }
    Ok(names)
}

fn delete_one(root: &Path, name: &str) -> Result<()> {
    let tenant = ManagedTenant::resolve(root, name)?;
    let deleting = tenant.deleting_dir();
    let tenants = root.join(TENANTS_DIR);
    let tenants_exist = real_dir_exists(&tenants, "Tenant collection")?;
    if tenants_exist && tenant.exists()? {
        remove_real_dir_if_exists(&deleting, "stale Tenant deletion")?;
        fs::rename(&tenant.home_dir, &deleting).with_context(|| {
            format!(
                "move Tenant Home {} to {} for deletion",
                tenant.home_dir.display(),
                deleting.display()
            )
        })?;
        sync_dir(&tenants)?;
    }
    if tenants_exist {
        remove_real_dir_if_exists(&tenant.creating_dir(), "Tenant creation staging directory")?;
    }
    for agent in AgentKind::ALL {
        let collection = root.join(agent.tag());
        if real_dir_exists(&collection, "Named Config catalog collection")? {
            remove_real_dir_if_exists(&collection.join(name), "Named Config catalog")?;
        }
    }
    if tenants_exist {
        remove_real_dir_if_exists(&deleting, "Tenant deletion staging directory")?;
        sync_dir(&tenants)?;
    }
    Ok(())
}

fn ensure_home_baseline(home: &Path) -> Result<()> {
    ensure_real_dir(home, "Tenant Home")?;
    install_missing_file(
        &home.join(".gitconfig"),
        "Tenant gitconfig",
        GITCONFIG,
        0o644,
    )?;
    for agent in AgentKind::ALL {
        ensure_agent_state(agent, home)?;
    }
    Ok(())
}

fn install_missing_file(path: &Path, kind: &str, content: &[u8], mode: u32) -> Result<()> {
    if real_file_exists(path, kind)? {
        return Ok(());
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("create {kind} {}", path.display()))?;
    if let Err(error) = file.write_all(content) {
        let _ = fs::remove_file(path);
        return Err(error).with_context(|| format!("write {kind} {}", path.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode))?;
    }
    file.sync_all()?;
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}
