//! Profile layout, initialization, and host-side path validation.
//!
//! Ordinary profiles keep the container-visible home under `<profile>/home`
//! and provider metadata under `<profile>/config`. The built-in `host` profile
//! points at the real host home for config/session commands but is never
//! runnable or deletable.

use crate::agent::AgentKind;
use crate::cli::ProfileCommand;
use anyhow::{bail, Context, Result};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Component, Path, PathBuf};

/// Reserved profile name that points to the real host agent state.
pub const HOST_PROFILE: &str = "host";
/// Container-visible home subtree of an ordinary profile.
pub const PROFILE_HOME_DIR: &str = "home";
/// Host-only provider-management subtree of a profile.
pub const PROFILE_CONFIG_DIR: &str = "config";
/// Reserved host-only tracing subtree of an ordinary profile.
pub const PROFILE_TRACING_DIR: &str = "tracing";
const HOST_PROFILE_LIST_ENTRY: &str = "host [external-home]";
const CLAUDE_STATUSLINE_SCRIPT: &[u8] = include_bytes!("../assets/claude-status.sh");
// Profile homes do not receive host SSH keys. Preserve common GitHub clone URLs
// by routing the SSH forms through HTTPS unless the user supplies a gitconfig.
const GITCONFIG: &[u8] = b"[url \"https://github.com/\"]\n    insteadOf = git@github.com:\n    insteadOf = ssh://git@github.com/\n";

/// Resolved paths and agent selection for one ordinary or host profile.
#[derive(Debug, Clone)]
pub struct Profile {
    /// Agent whose active and managed configuration paths are selected.
    pub agent: AgentKind,
    /// Validated profile name.
    pub name: String,
    /// Mounted profile home, or the real `$HOME` for the host profile.
    pub home_dir: PathBuf,
    /// Selected agent's state directory beneath [`Self::home_dir`].
    pub active_agent_dir: PathBuf,
    root_dir: PathBuf,
    management_dir: PathBuf,
    is_host: bool,
}

impl Profile {
    /// Resolve a validated profile name without creating any directories.
    ///
    /// The reserved `host` profile uses the process home for active state but
    /// still stores provider metadata beneath `root/host/config`.
    pub fn resolve(agent: AgentKind, root: &Path, profile: &str) -> Result<Self> {
        validate_name("profile", profile)?;
        let is_host = profile == HOST_PROFILE;
        let profile_dir = root.join(profile);
        let home_dir = if is_host {
            host_home()?
        } else {
            profile_dir.join(PROFILE_HOME_DIR)
        };
        let active_agent_dir = home_dir.join(agent.active_dir_name());
        let management_dir = profile_dir.join(PROFILE_CONFIG_DIR).join(agent.tag());
        Ok(Self {
            agent,
            name: profile.to_string(),
            home_dir,
            active_agent_dir,
            root_dir: root.to_path_buf(),
            management_dir,
            is_host,
        })
    }

    /// Whether this is the management-only host profile.
    pub fn is_host(&self) -> bool {
        self.is_host
    }

    /// Initialize an ordinary profile and reject the host profile.
    pub fn ensure_runnable_profile(&self) -> Result<()> {
        if self.is_host {
            bail!("profile 'host' is only valid for config/session commands, not Docker runs");
        }
        self.ensure_ordinary_initialized()
    }

    /// Ensure the selected active agent directory is ready for config apply.
    pub fn ensure_active_agent_dir(&self) -> Result<()> {
        if self.is_host {
            validate_profile_layout(&self.root_dir, &self.name)?;
            if !real_dir_exists(&self.home_dir, "host home")? {
                bail!("host home does not exist: {}", self.home_dir.display());
            }
            ensure_agent_state(self.agent, &self.home_dir)
        } else {
            self.ensure_ordinary_initialized()
        }
    }

    /// Reject unsafe existing profile and agent-state path entries.
    ///
    /// Missing ordinary-profile paths are allowed because apply initializes
    /// them only after provider content has been validated.
    pub fn validate_existing_active_agent_dir(&self) -> Result<()> {
        validate_profile_layout(&self.root_dir, &self.name)?;
        if self.is_host {
            if !real_dir_exists(&self.home_dir, "host home")? {
                bail!("host home does not exist: {}", self.home_dir.display());
            }
        } else {
            real_dir_exists(&self.home_dir, "profile home")?;
        }

        let kind = match self.agent {
            AgentKind::Claude => "Claude state directory",
            AgentKind::Codex => "Codex state directory",
        };
        real_dir_exists(&self.active_agent_dir, kind)?;
        Ok(())
    }

    /// Initialize this profile as a complete ordinary profile.
    ///
    /// This creates the shared home, both agent state directories, seed files,
    /// and both provider-management directories.
    pub fn ensure_ordinary_initialized(&self) -> Result<()> {
        if self.is_host {
            bail!("profile 'host' is only valid for config/session commands, not profile creation");
        }
        ensure_ordinary_profile_initialized(&self.root_dir, &self.name)
    }

    /// Validate the existing home path before host-side transcript access.
    ///
    /// A missing ordinary profile is treated as an empty session source.
    pub fn validate_session_home(&self) -> Result<()> {
        validate_profile_layout(&self.root_dir, &self.name)?;
        if self.is_host {
            if !real_dir_exists(&self.home_dir, "host home")? {
                bail!("host home does not exist: {}", self.home_dir.display());
            }
            return Ok(());
        }
        real_dir_exists(&self.home_dir, "profile home")?;
        Ok(())
    }

    /// Path to one active managed file.
    ///
    /// `file_name` must be a single trusted file name, normally one returned by
    /// [`AgentKind::managed_config_files`].
    pub fn active_file(&self, file_name: &str) -> PathBuf {
        self.active_agent_dir.join(file_name)
    }

    /// Root containing provider directories for the selected agent.
    pub fn provider_root_dir(&self) -> PathBuf {
        self.management_dir.clone()
    }

    /// Directory for one provider snapshot.
    ///
    /// The caller must first validate `provider` with [`validate_name`].
    pub fn provider_dir(&self, provider: &str) -> PathBuf {
        self.provider_root_dir().join(provider)
    }

    /// Path to one managed file inside a provider snapshot.
    ///
    /// `provider` must be validated and `file_name` must be a single trusted
    /// file name.
    pub fn provider_file(&self, provider: &str, file_name: &str) -> PathBuf {
        self.provider_dir(provider).join(file_name)
    }

    /// Directory containing timestamped active-config backups.
    pub fn backups_dir(&self) -> PathBuf {
        self.management_dir.join(".backup")
    }

    /// Path to the last-applied provider marker.
    pub fn state_path(&self) -> PathBuf {
        self.management_dir.join(".state.json")
    }

    /// Create the selected agent's provider-management directory safely.
    pub fn ensure_management_dir(&self) -> Result<()> {
        if self.is_host {
            ensure_agent_management_dir(&self.root_dir, &self.name, self.agent)
        } else {
            self.ensure_ordinary_initialized()
        }
    }

    /// Whether the selected agent's real management directory exists.
    pub fn management_dir_exists(&self) -> Result<bool> {
        validate_profile_layout(&self.root_dir, &self.name)?;
        agent_management_dir_exists(&self.root_dir, &self.name, self.agent)
    }
}

/// Execute one parsed profile-management command.
pub fn dispatch(command: &ProfileCommand) -> Result<i32> {
    let root = config_root()?;
    match command {
        ProfileCommand::List => {
            for profile in profile_list_entries(&root)? {
                if !crate::print_line(&profile)? {
                    return Ok(0);
                }
            }
        }
        ProfileCommand::Create { profile } => create_ordinary_profile(&root, profile)?,
        ProfileCommand::Delete { profiles, all, yes } => {
            delete_ordinary_profiles(&root, profiles, *all, *yes)?;
        }
    }
    Ok(0)
}

fn profile_list_entries(root: &Path) -> Result<Vec<String>> {
    let mut profiles = list_profiles(root)?;
    // The host row only describes an external home. With no usable `$HOME`
    // there is no host profile to manage, so drop the row instead of failing a
    // listing of ordinary profiles that is otherwise complete — `AIBOX_ROOT`
    // alone is enough to create, run, and delete those.
    if host_home_is_usable() {
        profiles.push(HOST_PROFILE_LIST_ENTRY.to_string());
    }
    Ok(profiles)
}

pub(crate) fn host_home_is_usable() -> bool {
    host_home()
        .and_then(|home| real_dir_exists(&home, "host home"))
        .unwrap_or(false)
}

/// List ordinary profile names in lexical order.
///
/// The complete root layout is validated before any names are returned. The
/// built-in host profile is intentionally not included.
pub fn list_profiles(root: &Path) -> Result<Vec<String>> {
    validate_root_layout(root)?;
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", root.display())),
    };

    let mut profiles = Vec::new();
    for entry in entries {
        let entry = entry.with_context(|| format!("read entry in {}", root.display()))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            bail!(
                "unsupported entry in aibox root: {}; only the profile-centric layout is supported",
                entry.path().display()
            );
        };
        if name == HOST_PROFILE {
            continue;
        }
        profiles.push(name.to_string());
    }
    profiles.sort();
    Ok(profiles)
}

/// Create or finish initializing one ordinary profile.
pub fn create_ordinary_profile(root: &Path, profile: &str) -> Result<()> {
    validate_ordinary_profile_name(profile)?;
    ensure_ordinary_profile_initialized(root, profile)
}

/// Delete one ordinary profile, optionally skipping confirmation.
pub fn delete_ordinary_profile(root: &Path, profile: &str, yes: bool) -> Result<()> {
    delete_ordinary_profiles(root, &[profile.to_string()], false, yes)
}

/// Delete selected ordinary profiles, or all of them when `all` or an empty
/// slice selects all.
///
/// Deletion removes the complete profile tree, including both agents' state,
/// sessions, provider data, backups, and reserved tracing data. `all` and
/// explicit names are mutually exclusive. Every target is resolved before
/// deletion begins, and each requires interactive confirmation unless `yes`
/// is set.
pub fn delete_ordinary_profiles(
    root: &Path,
    profiles: &[String],
    all: bool,
    yes: bool,
) -> Result<()> {
    let targets = delete_profile_targets(root, profiles, all)?;
    if targets.is_empty() {
        eprintln!(">> no ordinary profiles");
        return Ok(());
    }

    if !yes {
        for profile in &targets {
            if !confirm_delete(profile)? {
                bail!("aborted");
            }
        }
    }

    for profile in targets {
        delete_ordinary_profile_dirs(root, &profile)?;
    }
    Ok(())
}

fn delete_profile_targets(root: &Path, profiles: &[String], all: bool) -> Result<Vec<String>> {
    if all && !profiles.is_empty() {
        bail!("--all cannot be combined with profile names");
    }

    if all || profiles.is_empty() {
        return list_deletable_profiles(root);
    }

    let mut targets = Vec::new();
    for profile in profiles {
        validate_ordinary_profile_name(profile)?;
        if !profile_exists(root, profile)? {
            bail!("profile '{profile}' does not exist");
        }
        if !targets.contains(profile) {
            targets.push(profile.clone());
        }
    }
    Ok(targets)
}

fn list_deletable_profiles(root: &Path) -> Result<Vec<String>> {
    list_profiles(root)
}

fn profile_exists(root: &Path, profile: &str) -> Result<bool> {
    validate_ordinary_profile_name(profile)?;
    validate_profile_layout(root, profile)?;
    real_dir_exists(&profile_dir(root, profile), "profile directory")
}

fn delete_ordinary_profile_dirs(root: &Path, profile: &str) -> Result<()> {
    let dir = profile_dir(root, profile);
    if profile_exists(root, profile)? {
        fs::remove_dir_all(&dir)
            .with_context(|| format!("delete profile directory {}", dir.display()))?;
    }
    Ok(())
}

/// Validate a profile name and reject the reserved host profile.
pub fn validate_ordinary_profile_name(profile: &str) -> Result<()> {
    validate_name("profile", profile)?;
    if profile == HOST_PROFILE {
        bail!("profile 'host' is only valid for config/session commands");
    }
    Ok(())
}

/// Safely initialize the complete directory structure shared by both agents.
///
/// Existing regular seed files are preserved; symlinked or unexpected layout
/// entries are rejected.
pub fn ensure_ordinary_profile_initialized(root: &Path, profile: &str) -> Result<()> {
    validate_ordinary_profile_name(profile)?;
    preflight_ordinary_profile_paths(root, profile)?;
    ensure_real_dir(root, "aibox root")?;
    let profile_dir = profile_dir(root, profile);
    ensure_real_dir(&profile_dir, "profile directory")?;
    let home_dir = profile_home_dir(root, profile);
    ensure_real_dir(&home_dir, "profile home")?;
    ensure_agent_state(AgentKind::Codex, &home_dir)?;
    ensure_agent_state(AgentKind::Claude, &home_dir)?;
    install_profile_gitconfig(&home_dir)?;
    let management_dir = ensure_profile_management_dir(root, profile)?;
    for agent in [AgentKind::Codex, AgentKind::Claude] {
        ensure_real_dir(
            &management_dir.join(agent.tag()),
            "config management directory",
        )?;
    }
    Ok(())
}

fn preflight_ordinary_profile_paths(root: &Path, profile: &str) -> Result<()> {
    validate_profile_layout(root, profile)?;
    let management_dir = profile_management_dir(root, profile);
    if real_dir_exists(&management_dir, "profile management directory")? {
        for agent in [AgentKind::Codex, AgentKind::Claude] {
            real_dir_exists(
                &management_dir.join(agent.tag()),
                "config management directory",
            )?;
        }
    }

    let home_dir = profile_home_dir(root, profile);
    if real_dir_exists(&home_dir, "profile home")? {
        real_dir_exists(&home_dir.join(".codex"), "Codex state directory")?;
        let claude_dir = home_dir.join(".claude");
        real_dir_exists(&claude_dir, "Claude state directory")?;
        real_file_exists(&home_dir.join(".gitconfig"), "profile gitconfig")?;
        real_file_exists(&claude_dir.join("statusline.sh"), "Claude status line")?;
    }
    Ok(())
}

fn profile_dir(root: &Path, profile: &str) -> PathBuf {
    root.join(profile)
}

fn profile_home_dir(root: &Path, profile: &str) -> PathBuf {
    profile_dir(root, profile).join(PROFILE_HOME_DIR)
}

fn profile_management_dir(root: &Path, profile: &str) -> PathBuf {
    profile_dir(root, profile).join(PROFILE_CONFIG_DIR)
}

fn ensure_profile_management_dir(root: &Path, profile: &str) -> Result<PathBuf> {
    ensure_real_dir(root, "aibox root")?;
    ensure_real_dir(&profile_dir(root, profile), "profile directory")?;
    let management_dir = profile_management_dir(root, profile);
    ensure_real_dir(&management_dir, "profile management directory")?;
    Ok(management_dir)
}

fn ensure_agent_management_dir(root: &Path, profile: &str, agent: AgentKind) -> Result<()> {
    validate_profile_layout(root, profile)?;
    let management_dir = ensure_profile_management_dir(root, profile)?;
    ensure_real_dir(
        &management_dir.join(agent.tag()),
        "config management directory",
    )
}

fn profile_management_dir_exists(root: &Path, profile: &str) -> Result<bool> {
    real_dir_exists(
        &profile_management_dir(root, profile),
        "profile management directory",
    )
}

fn agent_management_dir_exists(root: &Path, profile: &str, agent: AgentKind) -> Result<bool> {
    if !profile_management_dir_exists(root, profile)? {
        return Ok(false);
    }
    real_dir_exists(
        &profile_management_dir(root, profile).join(agent.tag()),
        "config management directory",
    )
}

fn validate_root_layout(root: &Path) -> Result<()> {
    if !real_dir_exists(root, "aibox root")? {
        return Ok(());
    }
    reject_legacy_management_root(root)?;
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", root.display()))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return invalid_layout(&entry.path(), "non-UTF-8 profile name");
        };
        if validate_name("profile", name).is_err() {
            return invalid_layout(&entry.path(), "unknown aibox root entry");
        }
        validate_profile_layout_inner(root, name)?;
    }
    Ok(())
}

pub(crate) fn validate_profile_layout(root: &Path, profile: &str) -> Result<()> {
    validate_name("profile", profile)?;
    if !real_dir_exists(root, "aibox root")? {
        return Ok(());
    }
    reject_legacy_management_root(root)?;
    validate_profile_layout_inner(root, profile)
}

fn reject_legacy_management_root(root: &Path) -> Result<()> {
    let legacy = root.join(".config");
    match fs::symlink_metadata(&legacy) {
        Ok(_) => invalid_layout(&legacy, "legacy provider management root"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {}", legacy.display())),
    }
}

fn validate_profile_layout_inner(root: &Path, profile: &str) -> Result<()> {
    validate_name("profile", profile)?;
    let dir = profile_dir(root, profile);
    if !layout_dir_exists(&dir, "profile directory")? {
        return Ok(());
    }

    for entry in fs::read_dir(&dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", dir.display()))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return invalid_layout(&entry.path(), "non-UTF-8 profile entry");
        };
        let allowed = match name {
            PROFILE_CONFIG_DIR | PROFILE_TRACING_DIR => true,
            PROFILE_HOME_DIR => profile != HOST_PROFILE,
            _ => false,
        };
        if !allowed {
            return invalid_layout(&entry.path(), "unknown profile entry");
        }
        layout_dir_exists(&entry.path(), "profile layout entry")?;
    }

    let config_dir = profile_management_dir(root, profile);
    if layout_dir_exists(&config_dir, "profile management directory")? {
        for entry in
            fs::read_dir(&config_dir).with_context(|| format!("read {}", config_dir.display()))?
        {
            let entry = entry.with_context(|| format!("read entry in {}", config_dir.display()))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return invalid_layout(&entry.path(), "non-UTF-8 agent config entry");
            };
            if !matches!(name, "codex" | "claude") {
                return invalid_layout(&entry.path(), "unknown agent config entry");
            }
            layout_dir_exists(&entry.path(), "config management directory")?;
        }
    }
    Ok(())
}

fn layout_dir_exists(path: &Path, kind: &str) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => Ok(true),
        Ok(_) => invalid_layout(path, &format!("{kind} is not a real directory")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("inspect {kind} {}", path.display())),
    }
}

fn invalid_layout<T>(path: &Path, reason: &str) -> Result<T> {
    bail!(
        "{reason}: {}; only the profile-centric layout is supported",
        path.display()
    )
}

/// Resolve the aibox root from `AIBOX_ROOT` or `$HOME/.aibox`.
///
/// A relative override is anchored to the process working directory.
pub fn config_root() -> Result<PathBuf> {
    let root = if let Some(root) = crate::env_override("AIBOX_ROOT")? {
        PathBuf::from(root)
    } else {
        host_home()?.join(".aibox")
    };
    absolutize(root)
}

fn host_home() -> Result<PathBuf> {
    let home = crate::env_override("HOME")?
        .map(PathBuf::from)
        .context("$HOME is not set")?;
    absolutize(home).context("resolve $HOME")
}

fn absolutize(path: PathBuf) -> Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .context("get current dir for config root")?
            .join(path)
    };

    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                resolved.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                let mut existing = fs::canonicalize(&resolved).with_context(|| {
                    format!(
                        "resolve parent component in path {}; {} must exist first",
                        absolute.display(),
                        resolved.display()
                    )
                })?;
                let metadata = fs::metadata(&existing)
                    .with_context(|| format!("inspect {}", existing.display()))?;
                if !metadata.is_dir() {
                    bail!(
                        "cannot resolve parent component through non-directory {} in path {}",
                        existing.display(),
                        absolute.display()
                    );
                }
                existing.pop();
                resolved = existing;
            }
        }
    }
    Ok(resolved)
}

/// Validate a profile or provider name as a single safe ASCII path segment.
pub fn validate_name(kind: &str, value: &str) -> Result<()> {
    if is_safe_name(value) {
        Ok(())
    } else {
        bail!("invalid {kind} name '{value}': use only letters, numbers, '_' and '-'")
    }
}

/// Whether a name is non-empty and contains only ASCII alphanumerics, `_`, or
/// `-`.
pub fn is_safe_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// Whether the final entry of `path` is an actual directory rather than a
/// symlink to one.
///
/// This does not validate ancestor components. Callers below
/// container-writable trees must establish that ancestor chain separately.
pub(crate) fn real_dir_exists(path: &Path, kind: &str) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => Ok(true),
        Ok(_) => bail!("{kind} is not a real directory: {}", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("inspect {kind} {}", path.display())),
    }
}

fn real_file_exists(path: &Path, kind: &str) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => Ok(true),
        Ok(_) => bail!("{kind} is not a regular file: {}", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("inspect {kind} {}", path.display())),
    }
}

/// Open an existing regular file without following a final symlink. The
/// symlink check before open gives a clear error for stable bad paths; the
/// no-follow open closes the race where a container-writable file is swapped
/// after that check. Ancestor directories must already have been validated.
pub(crate) fn open_real_file(path: &Path, kind: &str) -> Result<fs::File> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => {}
        Ok(_) => bail!("{kind} is not a regular file: {}", path.display()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Err(e).with_context(|| format!("open {kind} {}", path.display()));
        }
        Err(e) => return Err(e).with_context(|| format!("inspect {kind} {}", path.display())),
    }

    let file = open_no_follow(path).with_context(|| format!("open {kind} {}", path.display()))?;
    let meta = file
        .metadata()
        .with_context(|| format!("inspect opened {kind} {}", path.display()))?;
    if meta.file_type().is_file() {
        Ok(file)
    } else {
        bail!("{kind} is not a regular file: {}", path.display())
    }
}

#[cfg(unix)]
fn open_no_follow(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        // A regular file ignores O_NONBLOCK. A FIFO or device swapped in
        // after the type check opens without hanging, then the descriptor
        // metadata check in `open_real_file` rejects it.
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_no_follow(path: &Path) -> io::Result<fs::File> {
    fs::File::open(path)
}

/// Create `path` when absent, then require its final directory entry to be a
/// real directory.
///
/// `create_dir_all` may traverse existing ancestor symlinks, so callers must
/// validate any untrusted ancestor chain before using this helper.
pub(crate) fn ensure_real_dir(path: &Path, kind: &str) -> Result<()> {
    if real_dir_exists(path, kind)? {
        return Ok(());
    }
    fs::create_dir_all(path).with_context(|| format!("create {kind} {}", path.display()))?;
    if real_dir_exists(path, kind)? {
        Ok(())
    } else {
        bail!("{kind} disappeared while being created: {}", path.display())
    }
}

pub(crate) fn ensure_agent_state(agent: AgentKind, home_dir: &Path) -> Result<()> {
    let agent_dir = home_dir.join(agent.active_dir_name());
    let kind = match agent {
        AgentKind::Claude => "Claude state directory",
        AgentKind::Codex => "Codex state directory",
    };
    ensure_real_dir(&agent_dir, kind)?;
    if agent == AgentKind::Claude {
        install_claude_statusline(&agent_dir)?;
    }
    Ok(())
}

fn install_profile_gitconfig(home_dir: &Path) -> Result<()> {
    install_missing_file(
        &home_dir.join(".gitconfig"),
        "profile gitconfig",
        GITCONFIG,
        0o644,
    )
}

fn install_claude_statusline(agent_dir: &Path) -> Result<()> {
    install_missing_file(
        &agent_dir.join("statusline.sh"),
        "Claude status line",
        CLAUDE_STATUSLINE_SCRIPT,
        0o755,
    )
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

    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return match fs::symlink_metadata(path) {
                Ok(meta) if meta.file_type().is_file() => Ok(()),
                Ok(_) => bail!("{kind} is not a regular file: {}", path.display()),
                Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
            };
        }
        Err(error) => return Err(error).with_context(|| format!("create {}", path.display())),
    };
    if let Err(error) = file.write_all(content) {
        let _ = fs::remove_file(path);
        return Err(error).with_context(|| format!("write {}", path.display()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Anchor chmod to the file we created: the mounted home can be changed
        // concurrently, so resolving `path` again could follow a replacement.
        if let Err(error) = file.set_permissions(fs::Permissions::from_mode(mode)) {
            let _ = fs::remove_file(path);
            return Err(error).with_context(|| format!("chmod {:o} {}", mode, path.display()));
        }
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

fn confirm_delete(profile: &str) -> Result<bool> {
    if !io::stdin().is_terminal() {
        bail!("refusing to delete profile '{profile}' without --yes in a non-interactive shell");
    }

    eprint!("Delete profile '{profile}'? [y/N] ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(matches!(input.trim(), "y" | "Y" | "yes" | "YES"))
}

#[cfg(unix)]
/// Restrict an existing regular file to owner read/write permissions on Unix.
///
/// The final path entry is opened without following symlinks. Callers below a
/// container-writable home must validate ancestor directories first.
pub fn set_600(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let file = open_real_file(path, "private config file")?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod 600 {}", path.display()))
}

#[cfg(not(unix))]
/// No-op counterpart to Unix permission hardening.
pub fn set_600(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::EnvGuard;
    use std::fs;

    #[test]
    fn names_are_restricted_to_simple_ascii_segments() {
        for good in ["default", "test_1", "a-box", HOST_PROFILE] {
            validate_name("profile", good).unwrap();
        }
        for bad in ["", ".", "..", "a/b", "a.b", "中文", "bad\nname"] {
            assert!(validate_name("profile", bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn config_root_uses_aibox_root_without_agent_suffix() {
        let _env_lock = crate::test_env_lock();
        let cwd = std::env::current_dir().unwrap();

        let _root = EnvGuard::set("AIBOX_ROOT", "relative-root");
        assert_eq!(config_root().unwrap(), cwd.join("relative-root"));
    }

    #[test]
    fn config_root_resolves_parent_components_only_through_existing_directories() {
        let _env_lock = crate::test_env_lock();
        let scratch = tempfile::tempdir().unwrap();
        let existing = scratch.path().join("existing");
        fs::create_dir(&existing).unwrap();

        {
            let configured = existing.join("../resolved-root");
            let _root = EnvGuard::set("AIBOX_ROOT", configured.as_os_str());
            assert_eq!(
                config_root().unwrap(),
                fs::canonicalize(scratch.path())
                    .unwrap()
                    .join("resolved-root")
            );
        }

        let unresolved = scratch.path().join("future/../unexpected-root");
        let _root = EnvGuard::set("AIBOX_ROOT", unresolved.as_os_str());
        let err = config_root().unwrap_err().to_string();

        assert!(err.contains("must exist first"), "{err}");
        assert!(
            !scratch.path().join("future").exists(),
            "resolving a config root must not create an intermediate path"
        );
        assert!(
            !scratch.path().join("unexpected-root").exists(),
            "an unresolved parent component must not redirect later profile creation"
        );
    }

    #[test]
    fn config_root_requires_home_without_override() {
        let _env_lock = crate::test_env_lock();
        let _root = EnvGuard::remove("AIBOX_ROOT");
        let _home = EnvGuard::remove("HOME");

        let err = config_root().unwrap_err().to_string();

        assert!(err.contains("$HOME is not set"), "{err}");
    }

    #[test]
    fn relative_home_is_anchored_to_the_current_directory() {
        let _env_lock = crate::test_env_lock();
        let cwd = std::env::current_dir().unwrap();
        let _home = EnvGuard::set("HOME", "relative-home");

        let profile =
            Profile::resolve(AgentKind::Codex, Path::new("/aibox"), HOST_PROFILE).unwrap();

        assert_eq!(profile.home_dir, cwd.join("relative-home"));
        assert_eq!(profile.active_agent_dir, cwd.join("relative-home/.codex"));
    }

    #[test]
    fn ordinary_profile_is_shared_agent_home() {
        let root = Path::new("/aibox");
        let codex = Profile::resolve(AgentKind::Codex, root, "default").unwrap();
        let claude = Profile::resolve(AgentKind::Claude, root, "default").unwrap();

        assert_eq!(codex.home_dir, Path::new("/aibox/default/home"));
        assert_eq!(claude.home_dir, Path::new("/aibox/default/home"));
        assert_eq!(
            codex.active_agent_dir,
            Path::new("/aibox/default/home/.codex")
        );
        assert_eq!(
            claude.active_agent_dir,
            Path::new("/aibox/default/home/.claude")
        );
        assert_eq!(
            codex.provider_dir("openai"),
            Path::new("/aibox/default/config/codex/openai")
        );
        assert_eq!(
            codex.backups_dir(),
            Path::new("/aibox/default/config/codex/.backup")
        );
        assert_eq!(
            codex.state_path(),
            Path::new("/aibox/default/config/codex/.state.json")
        );
    }

    #[test]
    fn host_profile_uses_host_home_but_aibox_management_dir() {
        let _env_lock = crate::test_env_lock();
        let _home = EnvGuard::set("HOME", "/host-home");
        let p = Profile::resolve(AgentKind::Codex, Path::new("/aibox"), HOST_PROFILE).unwrap();

        assert!(p.is_host());
        assert_eq!(p.home_dir, Path::new("/host-home"));
        assert_eq!(p.active_agent_dir, Path::new("/host-home/.codex"));
        assert_eq!(
            p.provider_dir("openai"),
            Path::new("/aibox/host/config/codex/openai")
        );
        assert!(p.ensure_runnable_profile().is_err());
    }

    #[test]
    fn host_config_and_session_operations_reject_a_missing_home() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let missing_home = root.path().join("missing-home");
        let _home = EnvGuard::set("HOME", missing_home.as_os_str());
        let p = Profile::resolve(AgentKind::Codex, root.path(), HOST_PROFILE).unwrap();

        for error in [
            p.validate_existing_active_agent_dir().unwrap_err(),
            p.ensure_active_agent_dir().unwrap_err(),
            p.validate_session_home().unwrap_err(),
        ] {
            let error = error.to_string();
            assert!(error.contains("host home does not exist"), "{error}");
        }
        assert!(
            !missing_home.exists(),
            "validating the external host profile must not create the configured home"
        );
        assert!(
            !root.path().join(HOST_PROFILE).exists(),
            "a failed host operation must not create management state"
        );
    }

    #[test]
    fn create_ordinary_profile_creates_full_baseline() {
        let root = tempfile::tempdir().unwrap();

        create_ordinary_profile(root.path(), "work").unwrap();

        let home = root.path().join("work/home");
        assert!(home.join(".codex").is_dir());
        assert!(home.join(".claude").is_dir());
        assert!(fs::read_to_string(home.join(".claude/statusline.sh"))
            .unwrap()
            .contains("context_window"));
        assert_eq!(
            fs::read_to_string(home.join(".gitconfig")).unwrap(),
            "[url \"https://github.com/\"]\n    insteadOf = git@github.com:\n    insteadOf = ssh://git@github.com/\n"
        );
        assert!(root.path().join("work/config/codex").is_dir());
        assert!(root.path().join("work/config/claude").is_dir());
        assert!(!root.path().join("work/tracing").exists());
    }

    #[cfg(unix)]
    #[test]
    fn claude_statusline_is_compatible_with_bash_without_mapfile() {
        use std::os::unix::fs::PermissionsExt;
        use std::process::{Command, Stdio};

        let scratch = tempfile::tempdir().unwrap();
        let fake_bin = scratch.path().join("bin");
        let workspace = scratch.path().join("workspace");
        fs::create_dir(&fake_bin).unwrap();
        fs::create_dir(&workspace).unwrap();

        let fake_jq = fake_bin.join("jq");
        fs::write(
            &fake_jq,
            "#!/bin/sh\nprintf '%s\\n' 'Opus' '' \"$AIBOX_TEST_STATUS_WORKSPACE\" '42' '200000' '84000'\n",
        )
        .unwrap();
        fs::set_permissions(&fake_jq, fs::Permissions::from_mode(0o755)).unwrap();

        let script = scratch.path().join("statusline.sh");
        fs::write(&script, CLAUDE_STATUSLINE_SCRIPT).unwrap();

        let mut paths = vec![fake_bin];
        if let Some(path) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&path));
        }
        let path = std::env::join_paths(paths).unwrap();

        let mut child = Command::new("bash")
            .arg(&script)
            .env("PATH", path)
            .env("AIBOX_TEST_STATUS_WORKSPACE", &workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(b"{}\n").unwrap();
        drop(stdin);

        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();

        assert!(
            output.status.success(),
            "stdout:\n{stdout}\nstderr:\n{stderr}"
        );
        assert!(stderr.is_empty(), "{stderr}");
        assert!(stdout.contains("workspace | [Opus]"), "{stdout}");
        assert!(stdout.contains("42% (84k/200k)"), "{stdout}");
    }

    #[test]
    fn create_ordinary_profile_is_idempotent_and_preserves_regular_files() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("work/home");
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::write(home.join(".gitconfig"), "[user]\n    name = Existing\n").unwrap();
        fs::write(
            home.join(".claude/statusline.sh"),
            "#!/bin/sh\necho existing\n",
        )
        .unwrap();

        create_ordinary_profile(root.path(), "work").unwrap();
        create_ordinary_profile(root.path(), "work").unwrap();

        assert_eq!(
            fs::read_to_string(home.join(".gitconfig")).unwrap(),
            "[user]\n    name = Existing\n"
        );
        assert_eq!(
            fs::read_to_string(home.join(".claude/statusline.sh")).unwrap(),
            "#!/bin/sh\necho existing\n"
        );
        assert!(home.join(".codex").is_dir());
        assert!(root.path().join("work/config/codex").is_dir());
        assert!(root.path().join("work/config/claude").is_dir());
    }

    #[test]
    fn create_ordinary_profile_rejects_wrong_type_seed_files() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("work/home");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir(home.join(".gitconfig")).unwrap();

        let err = create_ordinary_profile(root.path(), "work")
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("profile gitconfig is not a regular file"),
            "{err}"
        );
    }

    #[test]
    fn list_profiles_returns_sorted_valid_ordinary_profiles_only() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("zeta/home")).unwrap();
        fs::create_dir_all(root.path().join("alpha/config")).unwrap();
        fs::create_dir_all(root.path().join("host/config")).unwrap();

        assert_eq!(
            list_profiles(root.path()).unwrap(),
            vec!["alpha".to_string(), "zeta".to_string()]
        );
    }

    #[test]
    fn listing_a_missing_root_is_empty_and_does_not_create_it() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("not-initialized");

        assert!(list_profiles(&root).unwrap().is_empty());
        assert!(!root.exists());
    }

    #[test]
    fn profile_list_appends_marked_host_after_sorted_ordinary_profiles() {
        let _env_lock = crate::test_env_lock();
        let host_home = tempfile::tempdir().unwrap();
        let _home = EnvGuard::set("HOME", host_home.path().as_os_str());
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("zeta/home")).unwrap();
        fs::create_dir_all(root.path().join("alpha/home")).unwrap();

        assert_eq!(
            profile_list_entries(root.path()).unwrap(),
            vec![
                "alpha".to_string(),
                "zeta".to_string(),
                HOST_PROFILE_LIST_ENTRY.to_string()
            ]
        );
    }

    #[test]
    fn profile_list_omits_the_host_row_without_a_usable_home() {
        let _env_lock = crate::test_env_lock();
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("work/home")).unwrap();

        for home in [None, Some(root.path().join("missing-home"))] {
            let _home = match home {
                Some(home) => EnvGuard::set("HOME", home.as_os_str()),
                None => EnvGuard::remove("HOME"),
            };
            assert_eq!(
                profile_list_entries(root.path()).unwrap(),
                vec!["work".to_string()],
                "an unusable $HOME removes only the external host row"
            );
        }
    }

    #[test]
    fn delete_ordinary_profile_removes_home_and_management_dirs() {
        let root = tempfile::tempdir().unwrap();
        create_ordinary_profile(root.path(), "default").unwrap();

        delete_ordinary_profile(root.path(), "default", true).unwrap();

        assert!(!root.path().join("default").exists());
    }

    #[test]
    fn delete_ordinary_profiles_accepts_many() {
        let root = tempfile::tempdir().unwrap();
        create_ordinary_profile(root.path(), "default").unwrap();
        create_ordinary_profile(root.path(), "work").unwrap();
        create_ordinary_profile(root.path(), "keep").unwrap();

        delete_ordinary_profiles(
            root.path(),
            &["default".to_string(), "work".to_string()],
            false,
            true,
        )
        .unwrap();

        assert!(!root.path().join("default").exists());
        assert!(!root.path().join("work").exists());
        assert!(root.path().join("keep").exists());
    }

    #[test]
    fn delete_ordinary_profiles_dedupes_repeated_names() {
        let root = tempfile::tempdir().unwrap();
        create_ordinary_profile(root.path(), "default").unwrap();

        delete_ordinary_profiles(
            root.path(),
            &["default".to_string(), "default".to_string()],
            false,
            true,
        )
        .unwrap();

        assert!(!root.path().join("default").exists());
    }

    #[test]
    fn delete_ordinary_profiles_empty_or_all_flag_selects_every_deletable_profile() {
        for (target, all) in [(Vec::new(), false), (Vec::new(), true)] {
            let root = tempfile::tempdir().unwrap();
            create_ordinary_profile(root.path(), "default").unwrap();
            create_ordinary_profile(root.path(), "work").unwrap();
            fs::create_dir_all(root.path().join("orphan/config/codex")).unwrap();
            fs::create_dir_all(root.path().join("host/config/codex")).unwrap();

            delete_ordinary_profiles(root.path(), &target, all, true).unwrap();

            assert!(!root.path().join("default").exists());
            assert!(!root.path().join("work").exists());
            assert!(!root.path().join("orphan").exists());
            assert!(root.path().join("host").exists());
        }
    }

    #[test]
    fn delete_ordinary_profiles_treats_all_as_a_profile_name_without_all_flag() {
        let root = tempfile::tempdir().unwrap();
        create_ordinary_profile(root.path(), "all").unwrap();
        create_ordinary_profile(root.path(), "default").unwrap();

        delete_ordinary_profiles(root.path(), &["all".to_string()], false, true).unwrap();

        assert!(!root.path().join("all").exists());
        assert!(root.path().join("default").exists());
    }

    #[test]
    fn delete_ordinary_profiles_resolves_every_name_before_deleting() {
        let root = tempfile::tempdir().unwrap();
        create_ordinary_profile(root.path(), "default").unwrap();

        let err = delete_ordinary_profiles(
            root.path(),
            &["default".to_string(), "missing".to_string()],
            false,
            true,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("profile 'missing' does not exist"), "{err}");
        assert!(root.path().join("default").exists());
    }

    #[test]
    fn delete_ordinary_profiles_rejects_all_flag_mixed_with_names() {
        let root = tempfile::tempdir().unwrap();
        create_ordinary_profile(root.path(), "default").unwrap();

        let err = delete_ordinary_profiles(root.path(), &["default".to_string()], true, true)
            .unwrap_err()
            .to_string();

        assert!(err.contains("--all cannot be combined"), "{err}");
        assert!(root.path().join("default").exists());
    }

    #[cfg(unix)]
    #[test]
    fn delete_all_profiles_validates_the_complete_layout_before_removing_anything() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        create_ordinary_profile(root.path(), "keep").unwrap();
        fs::write(outside.path().join("sentinel"), "outside\n").unwrap();
        symlink(outside.path(), root.path().join("unsafe")).unwrap();

        let err = delete_ordinary_profiles(root.path(), &[], true, true)
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("profile directory is not a real directory"),
            "{err}"
        );
        assert!(
            root.path().join("keep").is_dir(),
            "bulk deletion must not remove a valid profile before discovering an unsafe one"
        );
        assert_eq!(
            fs::read_to_string(outside.path().join("sentinel")).unwrap(),
            "outside\n",
            "bulk deletion must never traverse a symlinked profile"
        );
    }

    #[test]
    fn delete_ordinary_profile_handles_config_only_leftover() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("work/config/codex")).unwrap();

        delete_ordinary_profile(root.path(), "work", true).unwrap();

        assert!(!root.path().join("work").exists());
    }

    #[test]
    fn delete_ordinary_profile_rejects_host_and_missing_profiles() {
        let root = tempfile::tempdir().unwrap();

        let err = delete_ordinary_profile(root.path(), HOST_PROFILE, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("profile 'host' is only valid"), "{err}");

        let err = delete_ordinary_profile(root.path(), "missing", true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("profile 'missing' does not exist"), "{err}");
    }

    #[test]
    fn delete_ordinary_profile_refuses_noninteractive_delete_without_yes() {
        if io::stdin().is_terminal() {
            return;
        }
        let root = tempfile::tempdir().unwrap();
        create_ordinary_profile(root.path(), "work").unwrap();

        let err = delete_ordinary_profile(root.path(), "work", false)
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("without --yes in a non-interactive shell"),
            "{err}"
        );
        assert!(root.path().join("work").exists());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_runnable_profile_rejects_symlinked_home() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let p = Profile::resolve(AgentKind::Codex, root.path(), "default").unwrap();
        fs::create_dir(root.path().join("default")).unwrap();
        symlink(outside.path(), &p.home_dir).unwrap();

        let err = p.ensure_runnable_profile().unwrap_err().to_string();
        assert!(err.contains("profile layout entry is not a real directory"));
    }

    #[cfg(unix)]
    #[test]
    fn create_ordinary_profile_rejects_bad_home_before_management_writes() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("work")).unwrap();
        symlink(outside.path(), root.path().join("work/home")).unwrap();

        let err = create_ordinary_profile(root.path(), "work")
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("profile layout entry is not a real directory"),
            "{err}"
        );
        assert!(
            !root.path().join("work/config").exists(),
            "profile creation must not leave management state after rejecting the home"
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_ordinary_profile_rejects_bad_agent_dir_before_management_writes() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let home = root.path().join("work/home");
        fs::create_dir_all(&home).unwrap();
        symlink(outside.path(), home.join(".codex")).unwrap();

        let err = create_ordinary_profile(root.path(), "work")
            .unwrap_err()
            .to_string();

        assert!(err.contains("Codex state directory is not a real directory"));
        assert!(
            !root.path().join("work/config").exists(),
            "profile creation must not leave management state after rejecting an agent dir"
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_ordinary_profile_rejects_symlinked_statusline() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let claude = root.path().join("work/home/.claude");
        fs::create_dir_all(&claude).unwrap();
        symlink(outside.path(), claude.join("statusline.sh")).unwrap();

        let err = create_ordinary_profile(root.path(), "work")
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("Claude status line is not a regular file"),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_ordinary_profile_rejects_symlinked_gitconfig() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let home = root.path().join("work/home");
        fs::create_dir_all(&home).unwrap();
        symlink(outside.path(), home.join(".gitconfig")).unwrap();

        let err = create_ordinary_profile(root.path(), "work")
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("profile gitconfig is not a regular file"),
            "{err}"
        );
        assert!(
            !home.join(".codex").exists(),
            "profile creation must fail before writing agent state beside a symlinked seed file"
        );
    }

    #[cfg(unix)]
    #[test]
    fn open_real_file_rejects_symlinks() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        let link = dir.path().join("link");
        fs::write(&real, "contents\n").unwrap();
        symlink(&real, &link).unwrap();

        let err = open_real_file(&link, "test file").unwrap_err().to_string();

        assert!(err.contains("test file is not a regular file"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn set_600_rejects_symlinks_without_changing_the_target() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        let link = dir.path().join("link");
        fs::write(&real, "secret\n").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o644)).unwrap();
        symlink(&real, &link).unwrap();

        let err = set_600(&link).unwrap_err().to_string();

        assert!(
            err.contains("private config file is not a regular file"),
            "{err}"
        );
        assert_eq!(
            fs::metadata(&real).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_ordinary_profile_rejects_legacy_management_root() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), root.path().join(".config")).unwrap();

        let err = create_ordinary_profile(root.path(), "work")
            .unwrap_err()
            .to_string();

        assert!(err.contains("legacy provider management root"), "{err}");
        assert!(err.contains("profile-centric layout"), "{err}");
        assert!(
            !outside.path().join("work").exists(),
            "provider management data must not be created through legacy .config"
        );
        assert!(
            !root.path().join("work").exists(),
            "profile home should not be created after rejecting the management root"
        );
    }

    #[cfg(unix)]
    #[test]
    fn selected_profile_operations_reject_a_symlinked_aibox_root() {
        use std::os::unix::fs::symlink;

        let parent = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        create_ordinary_profile(outside.path(), "work").unwrap();
        let linked_root = parent.path().join("root");
        symlink(outside.path(), &linked_root).unwrap();

        let err = delete_ordinary_profile(&linked_root, "work", true)
            .unwrap_err()
            .to_string();

        assert!(err.contains("aibox root is not a real directory"), "{err}");
        assert!(
            outside.path().join("work").is_dir(),
            "profile deletion must not traverse a symlinked AIBOX_ROOT"
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_ordinary_profile_rejects_symlinked_agent_management_dir_before_home_writes() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let management = root.path().join("work/config");
        fs::create_dir_all(&management).unwrap();
        symlink(outside.path(), management.join("codex")).unwrap();

        let err = create_ordinary_profile(root.path(), "work")
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("config management directory is not a real directory"),
            "{err}"
        );
        assert!(
            !root.path().join("work/home").exists(),
            "profile home should not be created after rejecting agent management state"
        );
        assert!(
            !outside.path().join("claude").exists(),
            "profile creation must not write through a symlinked agent management dir"
        );
    }

    #[cfg(unix)]
    #[test]
    fn delete_ordinary_profile_rejects_symlinked_config_directory() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("work/home")).unwrap();
        fs::create_dir_all(outside.path().join("codex")).unwrap();
        symlink(outside.path(), root.path().join("work/config")).unwrap();

        let err = delete_ordinary_profile(root.path(), "work", true)
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("profile layout entry is not a real directory"),
            "{err}"
        );
        assert!(root.path().join("work").exists());
        assert!(
            outside.path().join("codex").exists(),
            "delete must not follow a symlinked config directory and remove outside data"
        );
    }

    #[test]
    fn delete_ordinary_profile_removes_reserved_tracing_tree() {
        let root = tempfile::tempdir().unwrap();
        create_ordinary_profile(root.path(), "work").unwrap();
        fs::create_dir(root.path().join("work/tracing")).unwrap();
        fs::write(root.path().join("work/tracing/traffic.log"), "trace\n").unwrap();

        delete_ordinary_profile(root.path(), "work", true).unwrap();

        assert!(!root.path().join("work").exists());
    }

    #[test]
    fn profile_layout_rejects_unknown_entries_and_host_home() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("work/home")).unwrap();
        fs::write(root.path().join("work/.gitconfig"), "legacy\n").unwrap();

        let err = list_profiles(root.path()).unwrap_err().to_string();
        assert!(err.contains("unknown profile entry"), "{err}");
        assert!(err.contains("profile-centric layout"), "{err}");

        fs::remove_file(root.path().join("work/.gitconfig")).unwrap();
        fs::create_dir_all(root.path().join("host/home")).unwrap();
        let err = list_profiles(root.path()).unwrap_err().to_string();
        assert!(err.contains("unknown profile entry"), "{err}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn profile_layout_rejects_non_utf8_names_at_every_untrusted_level() {
        use std::os::unix::ffi::OsStringExt;

        let invalid_name = || std::ffi::OsString::from_vec(vec![b'b', b'a', b'd', 0xff]);

        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(invalid_name())).unwrap();
        let err = list_profiles(root.path()).unwrap_err().to_string();
        assert!(err.contains("non-UTF-8 profile name"), "{err}");

        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("work")).unwrap();
        fs::create_dir(root.path().join("work").join(invalid_name())).unwrap();
        let err = validate_profile_layout(root.path(), "work")
            .unwrap_err()
            .to_string();
        assert!(err.contains("non-UTF-8 profile entry"), "{err}");

        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("work/config")).unwrap();
        fs::create_dir(root.path().join("work/config").join(invalid_name())).unwrap();
        let err = validate_profile_layout(root.path(), "work")
            .unwrap_err()
            .to_string();
        assert!(err.contains("non-UTF-8 agent config entry"), "{err}");
    }
}
