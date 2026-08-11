//! Tenant resolution, Managed Tenant Home lifecycle, and filesystem safety.
//!
//! A real `tenants/<name>` directory is the only Managed Tenant existence
//! marker. Homes are container-writable, so host-side operations validate
//! structural entries rather than following symlinks into arbitrary paths.

use crate::agent::AgentKind;
use crate::cli::TenantCommand;
use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Component, Path, PathBuf};

/// Collection containing all managed Tenant Homes.
pub const TENANTS_DIR: &str = "tenants";
/// Storage key used for the Host Tenant Named Config catalog outside valid names.
pub const HOST_STORAGE_KEY: &str = "__host";
const CREATING_PREFIX: &str = "$creating-";
const DELETING_PREFIX: &str = "$deleting-";
const GITCONFIG: &[u8] = b"[url \"https://github.com/\"]\n    insteadOf = git@github.com:\n    insteadOf = ssh://git@github.com/\n";

/// An aibox-managed, runnable Tenant.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ManagedTenant {
    /// Validated Tenant name.
    pub name: String,
    /// Persistent Home mounted into a Run.
    pub home_dir: PathBuf,
    root_dir: PathBuf,
}

/// A persistent Coding Agent state scope.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Tenant {
    /// An aibox-managed, runnable Tenant.
    Managed(ManagedTenant),
    /// The management-only Tenant backed by the real host Home.
    Host {
        /// Real host Home containing native Coding Agent state.
        home_dir: PathBuf,
        /// Root containing host-only aibox state.
        root_dir: PathBuf,
    },
}

/// One Coding Agent selected within a Tenant.
#[derive(Debug, Clone)]
pub struct TenantAgent {
    /// Selected Tenant.
    pub tenant: Tenant,
    /// Selected Coding Agent.
    pub agent: AgentKind,
    /// Native Coding Agent state directory.
    pub agent_state_dir: PathBuf,
    named_config_catalog_dir: PathBuf,
}

impl ManagedTenant {
    /// Resolve a Managed Tenant without touching the filesystem.
    pub fn resolve(root: &Path, name: &str) -> Result<Self> {
        validate_name("tenant", name)?;
        Ok(Self {
            name: name.to_string(),
            home_dir: root.join(TENANTS_DIR).join(name),
            root_dir: root.to_path_buf(),
        })
    }

    /// Select one Coding Agent in this Tenant.
    pub fn for_agent(&self, agent: AgentKind) -> TenantAgent {
        Tenant::Managed(self.clone()).for_agent(agent)
    }

    /// Create or repair the complete Tenant Home baseline.
    pub fn ensure_initialized(&self) -> Result<()> {
        ensure_real_dir(&self.root_dir, "aibox root")?;
        let tenants = self.root_dir.join(TENANTS_DIR);
        ensure_real_dir(&tenants, "Tenant collection")?;

        if real_dir_exists(&self.home_dir, "Tenant Home")? {
            remove_real_dir_if_exists(&self.deleting_dir(), "stale Tenant deletion")?;
            remove_real_dir_if_exists(&self.creating_dir(), "stale Tenant creation")?;
            return ensure_home_baseline(&self.home_dir);
        }

        // A missing authoritative Home makes same-name Named Config catalogs orphaned.
        // Complete any old deletion before establishing a fresh identity.
        remove_real_dir_if_exists(&self.deleting_dir(), "stale Tenant deletion")?;
        for agent in AgentKind::ALL {
            let collection = self.root_dir.join(agent.tag());
            if real_dir_exists(&collection, "Named Config catalog collection")? {
                remove_real_dir_if_exists(
                    &collection.join(&self.name),
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
    pub fn exists(&self) -> Result<bool> {
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

impl Tenant {
    /// Resolve a Managed or Host Tenant without creating data.
    pub fn resolve(root: &Path, host: bool, tenant: &str) -> Result<Self> {
        if host {
            Ok(Self::Host {
                home_dir: host_home()?,
                root_dir: root.to_path_buf(),
            })
        } else {
            Ok(Self::Managed(ManagedTenant::resolve(root, tenant)?))
        }
    }

    #[cfg(test)]
    pub(crate) fn resolve_with_home(
        root: &Path,
        host: bool,
        tenant: &str,
        host_home: &Path,
    ) -> Result<Self> {
        if host {
            Ok(Self::Host {
                home_dir: host_home.to_path_buf(),
                root_dir: root.to_path_buf(),
            })
        } else {
            Ok(Self::Managed(ManagedTenant::resolve(root, tenant)?))
        }
    }

    /// Select one Coding Agent in this Tenant.
    pub fn for_agent(&self, agent: AgentKind) -> TenantAgent {
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
    pub fn home_dir(&self) -> &Path {
        match self {
            Self::Managed(tenant) => &tenant.home_dir,
            Self::Host { home_dir, .. } => home_dir,
        }
    }

    /// Validate the real Host Home. A missing Managed Home is an empty scope.
    pub fn validate_session_home(&self) -> Result<()> {
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

    fn storage_key(&self) -> &str {
        match self {
            Self::Managed(tenant) => &tenant.name,
            Self::Host { .. } => HOST_STORAGE_KEY,
        }
    }
}

impl TenantAgent {
    /// Resolve a Tenant and select a Coding Agent without creating data.
    pub fn resolve(agent: AgentKind, root: &Path, host: bool, tenant: &str) -> Result<Self> {
        Ok(Tenant::resolve(root, host, tenant)?.for_agent(agent))
    }

    #[cfg(test)]
    pub(crate) fn resolve_with_home(
        agent: AgentKind,
        root: &Path,
        host: bool,
        tenant: &str,
        host_home: &Path,
    ) -> Result<Self> {
        Ok(Tenant::resolve_with_home(root, host, tenant, host_home)?.for_agent(agent))
    }

    /// Home containing the selected Current Config and Sessions.
    pub fn home_dir(&self) -> &Path {
        self.tenant.home_dir()
    }

    /// Ensure the Tenant and Tenant-local Named Config catalog exist.
    pub fn ensure_named_config_catalog(&self) -> Result<()> {
        match &self.tenant {
            Tenant::Managed(tenant) => tenant.ensure_initialized()?,
            Tenant::Host { home_dir, .. } => require_host_home(home_dir)?,
        }
        ensure_real_dir(self.tenant.root(), "aibox root")?;
        ensure_real_dir(
            &self.tenant.root().join(self.agent.tag()),
            "Named Config catalog collection",
        )?;
        ensure_real_dir(&self.named_config_catalog_dir, "Named Config catalog")
    }

    /// Ensure the selected native Agent state directory exists.
    pub fn ensure_agent_state_dir(&self) -> Result<()> {
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

    /// File path within the selected native Agent state directory.
    pub fn state_file(&self, file_name: &str) -> PathBuf {
        self.agent_state_dir.join(file_name)
    }

    /// Directory containing Tenant- and Coding Agent-local Configs.
    pub fn named_config_catalog_dir(&self) -> &Path {
        &self.named_config_catalog_dir
    }

    /// One Tenant- and Coding Agent-local Named Config directory.
    pub fn named_config_dir(&self, config: &str) -> PathBuf {
        self.named_config_catalog_dir.join(config)
    }

    /// One file in a Named Config definition.
    pub fn named_config_file(&self, config: &str, file_name: &str) -> PathBuf {
        self.named_config_dir(config).join(file_name)
    }

    /// Whether the Named Config catalog currently exists.
    pub fn named_config_catalog_exists(&self) -> Result<bool> {
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

/// Execute one parsed Tenant management command.
pub fn dispatch(root: &Path, command: &TenantCommand) -> Result<i32> {
    match command {
        TenantCommand::List => {
            for tenant in list_tenants(root)? {
                if !crate::print_line(&tenant)? {
                    break;
                }
            }
        }
        TenantCommand::Create { tenant } => {
            ManagedTenant::resolve(root, tenant)?.ensure_initialized()?;
        }
        TenantCommand::Delete { tenants, all, yes } => delete_tenants(root, tenants, *all, *yes)?,
    }
    Ok(0)
}

/// List completed Managed Tenant names without creating data.
pub fn list_tenants(root: &Path) -> Result<Vec<String>> {
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
pub fn delete_tenants(root: &Path, tenants: &[String], all: bool, yes: bool) -> Result<()> {
    if all && !tenants.is_empty() {
        bail!("--all cannot be combined with Tenant names");
    }
    if !all && tenants.is_empty() {
        bail!("provide at least one Tenant name or use --all");
    }
    let targets = if all {
        list_tenants(root)?
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
    if targets.is_empty() {
        eprintln!(">> no Managed Tenants");
        return Ok(());
    }
    if !yes {
        for tenant in &targets {
            if tenant_has_data(root, tenant)? && !confirm_delete(tenant)? {
                bail!("aborted");
            }
        }
    }
    for tenant in targets {
        delete_one(root, &tenant)?;
    }
    Ok(())
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

fn tenant_has_data(root: &Path, name: &str) -> Result<bool> {
    let tenant = ManagedTenant::resolve(root, name)?;
    let collection = root.join(TENANTS_DIR);
    if real_dir_exists(&collection, "Tenant collection")?
        && (tenant.exists()?
            || real_dir_exists(&tenant.creating_dir(), "Tenant creation staging directory")?
            || real_dir_exists(&tenant.deleting_dir(), "Tenant deletion staging directory")?)
    {
        return Ok(true);
    }
    for agent in AgentKind::ALL {
        let collection = root.join(agent.tag());
        if real_dir_exists(&collection, "Named Config catalog collection")?
            && real_dir_exists(&collection.join(name), "Named Config catalog")?
        {
            return Ok(true);
        }
    }
    Ok(false)
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

/// Resolve `$AIBOX_ROOT`, defaulting to `$HOME/.aibox`.
pub fn aibox_root() -> Result<PathBuf> {
    let root = aibox_root_path(
        std::env::var_os("AIBOX_ROOT").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )?;
    absolutize(&root)
}

fn aibox_root_path(
    configured_root: Option<&OsStr>,
    configured_home: Option<&OsStr>,
) -> Result<PathBuf> {
    match configured_root {
        Some(value) if value.is_empty() => bail!("AIBOX_ROOT is set but empty"),
        Some(value) => Ok(PathBuf::from(value)),
        None => Ok(host_home_path(configured_home)?.join(".aibox")),
    }
}

#[cfg(test)]
pub(crate) fn aibox_root_from(
    configured_root: Option<&OsStr>,
    configured_home: Option<&OsStr>,
    cwd: &Path,
) -> Result<PathBuf> {
    let root = aibox_root_path(configured_root, configured_home)?;
    absolutize_from(&root, cwd)
}

pub(crate) fn host_home() -> Result<PathBuf> {
    let home = host_home_path(std::env::var_os("HOME").as_deref())?;
    absolutize(&home)
}

fn host_home_path(home: Option<&OsStr>) -> Result<PathBuf> {
    let home = home.context("HOME is not set")?;
    if home.is_empty() {
        bail!("HOME is set but empty");
    }
    Ok(PathBuf::from(home))
}

#[cfg(test)]
pub(crate) fn host_home_from(home: Option<&OsStr>, cwd: &Path) -> Result<PathBuf> {
    absolutize_from(&host_home_path(home)?, cwd)
}

fn require_host_home(home: &Path) -> Result<()> {
    if !real_dir_exists(home, "Host Home")? {
        bail!("Host Home does not exist: {}", home.display());
    }
    Ok(())
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        absolutize_from(path, Path::new(""))
    } else {
        absolutize_from(path, &std::env::current_dir()?)
    }
}

fn absolutize_from(path: &Path, cwd: &Path) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                resolved.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !resolved.pop() {
                    bail!("path escapes its filesystem root: {}", absolute.display());
                }
            }
        }
    }
    Ok(resolved)
}

/// Validate a Tenant or Named Config name as a lowercase DNS label.
pub fn validate_name(kind: &str, value: &str) -> Result<()> {
    if is_safe_name(value) {
        Ok(())
    } else {
        bail!("invalid {kind} name '{value}': expected a 1-63 character lowercase DNS label")
    }
}

/// Whether a user-controlled name is a 1-63 character lowercase DNS label.
pub fn is_safe_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=63).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_alphanumeric)
        && bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

/// Return `false` when the path is absent and `true` when its final entry is a
/// real directory; reject any other final entry type.
///
/// Ancestors are not checked; callers below a container-writable Home must
/// validate them separately before relying on this result.
pub(crate) fn real_dir_exists(path: &Path, kind: &str) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => Ok(true),
        Ok(_) => bail!("{kind} is not a real directory: {}", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("inspect {kind} {}", path.display())),
    }
}

/// Return `false` when the path is absent and `true` when its final entry is a
/// regular file; reject any other final entry type.
///
/// Ancestors are not checked; callers below a container-writable Home must
/// validate them separately before relying on this result.
pub(crate) fn real_file_exists(path: &Path, kind: &str) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => Ok(true),
        Ok(_) => bail!("{kind} is not a regular file: {}", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("inspect {kind} {}", path.display())),
    }
}

/// Open an existing regular file without following a final symlink.
///
/// This does not protect symlinked ancestors. Callers below a
/// container-writable Home must validate the complete ancestor chain first.
pub(crate) fn open_real_file(path: &Path, kind: &str) -> Result<fs::File> {
    if !real_file_exists(path, kind)? {
        bail!("{kind} does not exist: {}", path.display());
    }
    let file = open_no_follow(path).with_context(|| format!("open {kind} {}", path.display()))?;
    if !file.metadata()?.file_type().is_file() {
        bail!("{kind} is not a regular file: {}", path.display());
    }
    Ok(file)
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> io::Result<fs::File> {
    fs::File::open(path)
}

/// Create a directory when absent, rejecting a symlink or non-directory final
/// entry.
///
/// This does not protect symlinked ancestors. Callers below a
/// container-writable Home must validate the complete ancestor chain first.
pub(crate) fn ensure_real_dir(path: &Path, kind: &str) -> Result<()> {
    if real_dir_exists(path, kind)? {
        return Ok(());
    }
    fs::create_dir_all(path).with_context(|| format!("create {kind} {}", path.display()))?;
    if real_dir_exists(path, kind)? {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .with_context(|| format!("chmod 0700 {kind} {}", path.display()))?;
        }
        if let Some(parent) = path.parent() {
            sync_dir(parent)?;
        }
        Ok(())
    } else {
        bail!("{kind} disappeared while being created: {}", path.display())
    }
}

pub(crate) fn ensure_agent_state(agent: AgentKind, home: &Path) -> Result<()> {
    let agent_dir = home.join(agent.state_dir_name());
    ensure_real_dir(&agent_dir, "Agent state directory")
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

/// Remove a regular final path entry when present, rejecting symlinks and other
/// entry types.
///
/// This does not protect symlinked ancestors. Callers below a
/// container-writable Home must validate the complete ancestor chain first.
pub(crate) fn remove_real_file_if_exists(path: &Path, kind: &str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {kind} {}", path.display())),
        Ok(meta) if !meta.file_type().is_file() => {
            bail!("{kind} is not a regular file: {}", path.display())
        }
        Ok(_) => {
            fs::remove_file(path).with_context(|| format!("remove {kind} {}", path.display()))?;
            if let Some(parent) = path.parent() {
                sync_dir(parent)?;
            }
            Ok(())
        }
    }
}

/// Remove a directory tree when its final path entry is a real directory,
/// rejecting symlinks and files.
///
/// This does not protect symlinked ancestors. Callers below a
/// container-writable Home must validate the complete ancestor chain first.
pub(crate) fn remove_real_dir_if_exists(path: &Path, kind: &str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {kind} {}", path.display())),
        Ok(meta) if !meta.file_type().is_dir() => {
            bail!("{kind} is not a real directory: {}", path.display())
        }
        Ok(_) => {
            fs::remove_dir_all(path)
                .with_context(|| format!("delete {kind} {}", path.display()))?;
            if let Some(parent) = path.parent() {
                sync_dir(parent)?;
            }
            Ok(())
        }
    }
}

pub(crate) fn sync_dir(path: &Path) -> Result<()> {
    fs::File::open(path)
        .with_context(|| format!("open directory {} for sync", path.display()))?
        .sync_all()
        .with_context(|| format!("sync directory {}", path.display()))
}

fn confirm_delete(tenant: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!("refusing to delete Tenant '{tenant}' without --yes in a non-interactive shell");
    }
    eprint!("Delete Tenant '{tenant}'? [y/N] ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim(), "y" | "Y" | "yes" | "YES"))
}

#[cfg(unix)]
/// Restrict an existing regular file to owner read/write permissions.
#[cfg(test)]
pub fn set_600(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let file = open_real_file(path, "private config file")?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod 600 {}", path.display()))
}

#[cfg(not(unix))]
#[cfg(test)]
pub fn set_600(_path: &Path) -> Result<()> {
    Ok(())
}

/// Size-bounded snapshot of one optional native file.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct FileSnapshot {
    pub present: bool,
    pub content: Vec<u8>,
    pub mode: Option<u32>,
}

impl FileSnapshot {
    /// Capture a size-bounded regular file without following a final symlink.
    pub fn capture_with_limit(path: &Path, limit: u64) -> Result<Self> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self {
                present: false,
                content: Vec::new(),
                mode: None,
            }),
            Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
            Ok(meta) if !meta.file_type().is_file() => {
                bail!(
                    "configuration path is not a regular file: {}",
                    path.display()
                )
            }
            Ok(_) => {
                let file = open_real_file(path, "configuration file")?;
                let metadata = file.metadata()?;
                if metadata.len() > limit {
                    bail!(
                        "configuration file exceeds {limit} bytes: {}",
                        path.display()
                    );
                }
                let mut content = Vec::new();
                file.take(limit.saturating_add(1))
                    .read_to_end(&mut content)?;
                if content.len() as u64 > limit {
                    bail!(
                        "configuration file exceeds {limit} bytes: {}",
                        path.display()
                    );
                }
                #[cfg(unix)]
                let mode = {
                    use std::os::unix::fs::PermissionsExt;
                    Some(metadata.permissions().mode() & 0o7777)
                };
                #[cfg(not(unix))]
                let mode = None;
                Ok(Self {
                    present: true,
                    content,
                    mode,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_and_home_resolution_use_only_explicit_inputs() {
        let cwd = Path::new("/workspace/project");
        assert_eq!(
            aibox_root_from(Some(OsStr::new("../state")), None, cwd).unwrap(),
            Path::new("/workspace/state")
        );
        assert_eq!(
            aibox_root_from(None, Some(OsStr::new("/host/home")), cwd).unwrap(),
            Path::new("/host/home/.aibox")
        );
        assert!(
            aibox_root_from(Some(OsStr::new("")), Some(OsStr::new("/host/home")), cwd)
                .unwrap_err()
                .to_string()
                .contains("AIBOX_ROOT is set but empty")
        );
        assert!(
            host_home_from(None, cwd)
                .unwrap_err()
                .to_string()
                .contains("HOME is not set")
        );
    }

    #[test]
    fn names_are_lowercase_dns_labels() {
        for valid in ["a", "work-1", &"a".repeat(63)] {
            assert!(is_safe_name(valid), "{valid}");
        }
        for invalid in [
            "",
            "Work",
            "work_1",
            "-work",
            "work-",
            HOST_STORAGE_KEY,
            &"a".repeat(64),
        ] {
            assert!(!is_safe_name(invalid), "{invalid}");
        }

        assert_eq!(
            validate_name("tenant", "Work").unwrap_err().to_string(),
            "invalid tenant name 'Work': expected a 1-63 character lowercase DNS label"
        );
    }

    #[test]
    fn file_snapshots_enforce_the_read_limit() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config");
        fs::write(&path, b"12345").unwrap();

        let error = FileSnapshot::capture_with_limit(&path, 4)
            .unwrap_err()
            .to_string();
        assert!(error.contains("exceeds 4 bytes"), "{error}");
        assert_eq!(
            FileSnapshot::capture_with_limit(&path, 5).unwrap().content,
            b"12345"
        );
    }

    #[test]
    fn file_snapshots_distinguish_absence_and_reject_non_files() {
        let root = tempfile::tempdir().unwrap();
        let missing = FileSnapshot::capture_with_limit(&root.path().join("missing"), 16).unwrap();
        assert!(!missing.present);
        assert!(missing.content.is_empty());
        assert_eq!(missing.mode, None);

        let error = FileSnapshot::capture_with_limit(root.path(), 16)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not a regular file"), "{error}");
    }

    #[test]
    fn initialization_publishes_direct_home_layout() {
        let root = tempfile::tempdir().unwrap();
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
        tenant.ensure_initialized().unwrap();
        assert_eq!(tenant.home_dir, root.path().join("tenants/work"));
        assert!(tenant.home_dir.join(".gitconfig").is_file());
        assert!(tenant.home_dir.join(".codex").is_dir());
        assert!(!tenant.home_dir.join(".claude/statusline.sh").exists());
        assert_eq!(list_tenants(root.path()).unwrap(), ["work"]);
    }

    #[test]
    fn initialization_repairs_baseline_without_overwriting_user_files() {
        let root = tempfile::tempdir().unwrap();
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
        tenant.ensure_initialized().unwrap();
        let gitconfig = tenant.home_dir.join(".gitconfig");
        fs::write(&gitconfig, b"[user]\nname = Keep Me\n").unwrap();
        fs::remove_dir(tenant.home_dir.join(".claude")).unwrap();

        tenant.ensure_initialized().unwrap();

        assert_eq!(fs::read(&gitconfig).unwrap(), b"[user]\nname = Keep Me\n");
        assert!(tenant.home_dir.join(".claude").is_dir());
        assert!(tenant.home_dir.join(".codex").is_dir());
    }

    #[test]
    fn initialization_rolls_stale_tenant_transitions_forward() {
        let root = tempfile::tempdir().unwrap();
        let tenants = root.path().join(TENANTS_DIR);
        fs::create_dir(&tenants).unwrap();
        let creating = tenants.join("$creating-work");
        let deleting = tenants.join("$deleting-work");
        fs::create_dir(&creating).unwrap();
        fs::create_dir(&deleting).unwrap();
        fs::write(creating.join("preserved"), b"staged").unwrap();
        fs::write(deleting.join("discarded"), b"old").unwrap();

        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
        tenant.ensure_initialized().unwrap();

        assert!(tenant.home_dir.join("preserved").is_file());
        assert!(!creating.exists());
        assert!(!deleting.exists());
        assert_eq!(list_tenants(root.path()).unwrap(), ["work"]);
    }

    #[cfg(unix)]
    #[test]
    fn newly_created_boundary_directories_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("new-aibox-root");
        let tenant = ManagedTenant::resolve(&root, "work").unwrap();
        tenant.ensure_initialized().unwrap();

        for path in [
            root.clone(),
            root.join(TENANTS_DIR),
            tenant.home_dir.clone(),
            tenant.home_dir.join(".claude"),
            tenant.home_dir.join(".codex"),
        ] {
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o700,
                "{}",
                path.display()
            );
        }
    }

    #[test]
    fn host_and_managed_storage_keys_do_not_collide() {
        let root = tempfile::tempdir().unwrap();
        let managed = ManagedTenant::resolve(root.path(), "host").unwrap();
        assert_eq!(
            managed
                .for_agent(AgentKind::Codex)
                .named_config_catalog_dir(),
            root.path().join("codex/host")
        );
        let host = Tenant::Host {
            home_dir: root.path().to_path_buf(),
            root_dir: root.path().to_path_buf(),
        };
        assert_eq!(
            host.for_agent(AgentKind::Codex).named_config_catalog_dir(),
            root.path().join("codex/__host")
        );
    }

    #[test]
    fn listing_is_read_only_and_ignores_unrecognized_entries() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("unrelated")).unwrap();
        let missing_root = root.path().join("missing");
        assert!(list_tenants(&missing_root).unwrap().is_empty());
        assert!(
            !missing_root.exists(),
            "listing a missing root must not initialize it"
        );
        fs::create_dir(root.path().join(TENANTS_DIR)).unwrap();
        fs::write(root.path().join("tenants/not-a-dir"), b"x").unwrap();
        fs::create_dir(root.path().join("tenants/bad_name")).unwrap();
        assert!(list_tenants(root.path()).unwrap().is_empty());
    }

    #[test]
    fn create_and_delete_are_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
        tenant.ensure_initialized().unwrap();
        tenant.ensure_initialized().unwrap();
        for agent in AgentKind::ALL {
            let catalog = root.path().join(agent.tag()).join("work");
            fs::create_dir_all(&catalog).unwrap();
            fs::write(catalog.join("owned"), b"config data").unwrap();
        }
        delete_tenants(root.path(), &["work".to_string()], false, true).unwrap();
        delete_tenants(root.path(), &["work".to_string()], false, true).unwrap();
        assert!(!tenant.home_dir.exists());
        assert!(!root.path().join("claude/work").exists());
        assert!(!root.path().join("codex/work").exists());
    }

    #[test]
    fn tenant_deletion_requires_explicit_selection_and_confirmation() {
        let root = tempfile::tempdir().unwrap();
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
        tenant.ensure_initialized().unwrap();
        let sentinel = tenant.home_dir.join("keep");
        fs::write(&sentinel, b"tenant data").unwrap();

        let empty = delete_tenants(root.path(), &[], false, true)
            .unwrap_err()
            .to_string();
        assert!(empty.contains("at least one Tenant"), "{empty}");

        let mixed = delete_tenants(root.path(), &["work".to_string()], true, true)
            .unwrap_err()
            .to_string();
        assert!(mixed.contains("--all cannot be combined"), "{mixed}");

        if !io::stdin().is_terminal() {
            let unconfirmed = delete_tenants(root.path(), &["work".to_string()], false, false)
                .unwrap_err()
                .to_string();
            assert!(unconfirmed.contains("without --yes"), "{unconfirmed}");
        }
        assert_eq!(fs::read(&sentinel).unwrap(), b"tenant data");
        assert_eq!(list_tenants(root.path()).unwrap(), ["work"]);
    }

    #[test]
    fn deleting_from_an_absent_root_is_idempotent_and_read_only() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("missing");

        delete_tenants(&root, &["work".to_string()], false, true).unwrap();
        delete_tenants(&root, &["work".to_string()], false, true).unwrap();

        assert!(!root.exists());
    }

    #[cfg(unix)]
    #[test]
    fn tenant_collection_symlinks_are_rejected() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join(TENANTS_DIR)).unwrap();

        let list_error = list_tenants(root.path()).unwrap_err().to_string();
        assert!(list_error.contains("not a real directory"), "{list_error}");
        let delete_error = delete_tenants(root.path(), &["work".to_string()], false, true)
            .unwrap_err()
            .to_string();
        assert!(
            delete_error.contains("not a real directory"),
            "{delete_error}"
        );
        assert!(!outside.path().join("work").exists());
    }

    #[cfg(unix)]
    #[test]
    fn orphaned_config_catalog_symlinks_block_tenant_publication() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("keep"), b"outside").unwrap();
        fs::create_dir(root.path().join("claude")).unwrap();
        symlink(outside.path(), root.path().join("claude/work")).unwrap();
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();

        let error = tenant.ensure_initialized().unwrap_err().to_string();

        assert!(error.contains("not a real directory"), "{error}");
        assert!(
            !tenant.home_dir.exists(),
            "an unsafe orphan must be rejected before publishing a new identity"
        );
        assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
    }

    #[cfg(unix)]
    #[test]
    fn linked_config_collection_is_rejected_before_orphan_cleanup() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(outside.path().join("work")).unwrap();
        fs::write(outside.path().join("work/keep"), b"outside").unwrap();
        symlink(outside.path(), root.path().join("claude")).unwrap();
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();

        let error = tenant.ensure_initialized().unwrap_err().to_string();

        assert!(error.contains("not a real directory"), "{error}");
        assert!(!tenant.home_dir.exists());
        assert_eq!(
            fs::read(outside.path().join("work/keep")).unwrap(),
            b"outside"
        );
    }

    #[cfg(unix)]
    #[test]
    fn interrupted_delete_rejects_linked_config_catalog_and_rolls_forward_safely() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("keep"), b"outside").unwrap();
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
        tenant.ensure_initialized().unwrap();
        fs::create_dir(root.path().join("claude")).unwrap();
        let linked_catalog = root.path().join("claude/work");
        symlink(outside.path(), &linked_catalog).unwrap();

        let error = delete_tenants(root.path(), &["work".to_string()], false, true)
            .unwrap_err()
            .to_string();

        assert!(error.contains("not a real directory"), "{error}");
        assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
        assert!(!tenant.home_dir.exists());
        assert!(root.path().join("tenants/$deleting-work").is_dir());

        fs::remove_file(linked_catalog).unwrap();
        delete_tenants(root.path(), &["work".to_string()], false, true).unwrap();

        assert!(!root.path().join("tenants/$deleting-work").exists());
        assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_tenant_home_is_rejected_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(TENANTS_DIR)).unwrap();
        fs::write(outside.path().join("keep"), b"outside").unwrap();
        symlink(outside.path(), root.path().join("tenants/work")).unwrap();
        let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();

        let init_error = tenant.ensure_initialized().unwrap_err().to_string();
        assert!(init_error.contains("not a real directory"), "{init_error}");
        let delete_error = delete_tenants(root.path(), &["work".to_string()], false, true)
            .unwrap_err()
            .to_string();
        assert!(
            delete_error.contains("not a real directory"),
            "{delete_error}"
        );
        assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
    }
}
