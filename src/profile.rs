//! Profile and config-management path layout.

use crate::agent::AgentKind;
use crate::cli::ProfileCommand;
use anyhow::{bail, Context, Result};
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

pub const HOST_PROFILE: &str = "host";
const CLAUDE_STATUSLINE_SCRIPT: &[u8] = include_bytes!("../assets/claude-status.sh");
const GITCONFIG: &[u8] = b"[url \"https://github.com/\"]\n    insteadOf = git@github.com:\n    insteadOf = ssh://git@github.com/\n";

#[derive(Debug, Clone)]
pub struct Profile {
    pub agent: AgentKind,
    pub name: String,
    pub home_dir: PathBuf,
    pub active_agent_dir: PathBuf,
    root_dir: PathBuf,
    management_dir: PathBuf,
    is_host: bool,
}

impl Profile {
    pub fn resolve(agent: AgentKind, root: &Path, profile: &str) -> Result<Self> {
        validate_name("profile", profile)?;
        let is_host = profile == HOST_PROFILE;
        let home_dir = if is_host {
            host_home()?
        } else {
            root.join(profile)
        };
        let active_agent_dir = home_dir.join(agent.active_dir_name());
        let management_dir = root.join(".config").join(profile).join(agent.tag());
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

    pub fn is_host(&self) -> bool {
        self.is_host
    }

    pub fn ensure_runnable_profile(&self) -> Result<()> {
        if self.is_host {
            bail!("profile 'host' is only valid for config/session commands, not Docker runs");
        }
        self.ensure_ordinary_initialized()
    }

    pub fn ensure_active_agent_dir(&self) -> Result<()> {
        if self.is_host {
            if !real_dir_exists(&self.home_dir, "host home")? {
                bail!("host home does not exist: {}", self.home_dir.display());
            }
            ensure_agent_state(self.agent, &self.home_dir)
        } else {
            self.ensure_ordinary_initialized()
        }
    }

    pub fn ensure_ordinary_initialized(&self) -> Result<()> {
        if self.is_host {
            bail!("profile 'host' is only valid for config/session commands, not profile creation");
        }
        ensure_ordinary_profile_initialized(&self.root_dir, &self.name)
    }

    pub fn validate_session_home(&self) -> Result<()> {
        if self.is_host {
            if !real_dir_exists(&self.home_dir, "host home")? {
                bail!("host home does not exist: {}", self.home_dir.display());
            }
            return Ok(());
        }
        real_dir_exists(&self.home_dir, "profile home")?;
        Ok(())
    }

    pub fn active_file(&self, file_name: &str) -> PathBuf {
        self.active_agent_dir.join(file_name)
    }

    pub fn provider_root_dir(&self) -> PathBuf {
        self.management_dir.clone()
    }

    pub fn provider_dir(&self, provider: &str) -> PathBuf {
        self.provider_root_dir().join(provider)
    }

    pub fn provider_file(&self, provider: &str, file_name: &str) -> PathBuf {
        self.provider_dir(provider).join(file_name)
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.management_dir.join(".backup")
    }

    pub fn state_path(&self) -> PathBuf {
        self.management_dir.join(".state.json")
    }

    pub fn ensure_management_dir(&self) -> Result<()> {
        if self.is_host {
            ensure_real_dir(&self.management_dir, "config management directory")
        } else {
            self.ensure_ordinary_initialized()
        }
    }
}

pub fn dispatch(command: &ProfileCommand) -> Result<i32> {
    let root = config_root()?;
    match command {
        ProfileCommand::List => {
            for profile in list_profiles(&root)? {
                if !crate::print_line(&profile)? {
                    break;
                }
            }
        }
        ProfileCommand::Create { profile } => create_ordinary_profile(&root, profile)?,
        ProfileCommand::Delete { profile, yes } => delete_ordinary_profile(&root, profile, *yes)?,
    }
    Ok(0)
}

pub fn list_profiles(root: &Path) -> Result<Vec<String>> {
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
            continue;
        };
        if name == ".config" || name == HOST_PROFILE || validate_name("profile", name).is_err() {
            continue;
        }
        let path = entry.path();
        let meta =
            fs::symlink_metadata(&path).with_context(|| format!("inspect {}", path.display()))?;
        if meta.file_type().is_dir() {
            profiles.push(name.to_string());
        }
    }
    profiles.sort();
    Ok(profiles)
}

pub fn create_ordinary_profile(root: &Path, profile: &str) -> Result<()> {
    validate_ordinary_profile_name(profile)?;
    ensure_ordinary_profile_initialized(root, profile)
}

pub fn delete_ordinary_profile(root: &Path, profile: &str, yes: bool) -> Result<()> {
    validate_ordinary_profile_name(profile)?;

    let home_dir = root.join(profile);
    let management_dir = root.join(".config").join(profile);
    let home_exists = real_dir_exists(&home_dir, "profile home")?;
    let management_exists = real_dir_exists(&management_dir, "profile management directory")?;
    if !home_exists && !management_exists {
        bail!("profile '{profile}' does not exist");
    }

    if !yes && !confirm_delete(profile)? {
        bail!("aborted");
    }

    if home_exists {
        fs::remove_dir_all(&home_dir)
            .with_context(|| format!("delete profile home {}", home_dir.display()))?;
    }
    if management_exists {
        fs::remove_dir_all(&management_dir).with_context(|| {
            format!(
                "delete profile management directory {}",
                management_dir.display()
            )
        })?;
    }
    Ok(())
}

pub fn validate_ordinary_profile_name(profile: &str) -> Result<()> {
    validate_name("profile", profile)?;
    if profile == HOST_PROFILE {
        bail!("profile 'host' is only valid for config/session commands");
    }
    Ok(())
}

pub fn ensure_ordinary_profile_initialized(root: &Path, profile: &str) -> Result<()> {
    validate_ordinary_profile_name(profile)?;
    let home_dir = root.join(profile);
    ensure_real_dir(&home_dir, "profile home")?;
    ensure_agent_state(AgentKind::Codex, &home_dir)?;
    ensure_agent_state(AgentKind::Claude, &home_dir)?;
    install_profile_gitconfig(&home_dir)?;
    for agent in [AgentKind::Codex, AgentKind::Claude] {
        ensure_real_dir(
            &root.join(".config").join(profile).join(agent.tag()),
            "config management directory",
        )?;
    }
    Ok(())
}

pub fn config_root() -> Result<PathBuf> {
    let root = if let Some(root) = crate::env_override("AIBOX_CONFIG_ROOT")? {
        PathBuf::from(root)
    } else {
        host_home()?.join(".aibox")
    };
    absolutize(root)
}

fn host_home() -> Result<PathBuf> {
    crate::env_override("HOME")?
        .map(PathBuf::from)
        .context("$HOME is not set")
}

fn absolutize(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()
            .context("get current dir for config root")?
            .join(path))
    }
}

pub fn validate_name(kind: &str, value: &str) -> Result<()> {
    if is_safe_name(value) {
        Ok(())
    } else {
        bail!("invalid {kind} name '{value}': use only letters, numbers, '_' and '-'")
    }
}

pub fn is_safe_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

/// Whether `path` exists as an actual directory entry rather than a symlink to
/// one.
pub(crate) fn real_dir_exists(path: &Path, kind: &str) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_dir() => Ok(true),
        Ok(_) => bail!("{kind} is not a real directory: {}", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).with_context(|| format!("inspect {kind} {}", path.display())),
    }
}

/// Create `path` when absent, then require its final directory entry to be a
/// real directory.
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
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_file() => return Ok(()),
        Ok(_) => bail!("{kind} is not a regular file: {}", path.display()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).with_context(|| format!("inspect {}", path.display())),
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
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .with_context(|| format!("chmod {:o} {}", mode, path.display()))?;
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
pub fn set_600(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod 600 {}", path.display()))
}

#[cfg(not(unix))]
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

        let _root = EnvGuard::set("AIBOX_CONFIG_ROOT", "relative-root");
        assert_eq!(config_root().unwrap(), cwd.join("relative-root"));
    }

    #[test]
    fn config_root_requires_home_without_override() {
        let _env_lock = crate::test_env_lock();
        let _root = EnvGuard::remove("AIBOX_CONFIG_ROOT");
        let _home = EnvGuard::remove("HOME");

        let err = config_root().unwrap_err().to_string();

        assert!(err.contains("$HOME is not set"), "{err}");
    }

    #[test]
    fn ordinary_profile_is_shared_agent_home() {
        let root = Path::new("/aibox");
        let codex = Profile::resolve(AgentKind::Codex, root, "default").unwrap();
        let claude = Profile::resolve(AgentKind::Claude, root, "default").unwrap();

        assert_eq!(codex.home_dir, Path::new("/aibox/default"));
        assert_eq!(claude.home_dir, Path::new("/aibox/default"));
        assert_eq!(codex.active_agent_dir, Path::new("/aibox/default/.codex"));
        assert_eq!(claude.active_agent_dir, Path::new("/aibox/default/.claude"));
        assert_eq!(
            codex.provider_dir("openai"),
            Path::new("/aibox/.config/default/codex/openai")
        );
        assert_eq!(
            codex.backups_dir(),
            Path::new("/aibox/.config/default/codex/.backup")
        );
        assert_eq!(
            codex.state_path(),
            Path::new("/aibox/.config/default/codex/.state.json")
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
            Path::new("/aibox/.config/host/codex/openai")
        );
        assert!(p.ensure_runnable_profile().is_err());
    }

    #[test]
    fn create_ordinary_profile_creates_full_baseline() {
        let root = tempfile::tempdir().unwrap();

        create_ordinary_profile(root.path(), "work").unwrap();

        let home = root.path().join("work");
        assert!(home.join(".codex").is_dir());
        assert!(home.join(".claude").is_dir());
        assert!(fs::read_to_string(home.join(".claude/statusline.sh"))
            .unwrap()
            .contains("context_window"));
        assert_eq!(
            fs::read_to_string(home.join(".gitconfig")).unwrap(),
            "[url \"https://github.com/\"]\n    insteadOf = git@github.com:\n    insteadOf = ssh://git@github.com/\n"
        );
        assert!(root.path().join(".config/work/codex").is_dir());
        assert!(root.path().join(".config/work/claude").is_dir());
    }

    #[test]
    fn create_ordinary_profile_is_idempotent_and_preserves_regular_files() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("work");
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
        assert!(root.path().join(".config/work/codex").is_dir());
        assert!(root.path().join(".config/work/claude").is_dir());
    }

    #[test]
    fn create_ordinary_profile_rejects_wrong_type_seed_files() {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("work");
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
    fn list_profiles_returns_sorted_real_profile_homes_only() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("zeta")).unwrap();
        fs::create_dir(root.path().join("alpha")).unwrap();
        fs::create_dir(root.path().join(".config")).unwrap();
        fs::create_dir(root.path().join(HOST_PROFILE)).unwrap();
        fs::create_dir(root.path().join("bad.name")).unwrap();
        fs::write(root.path().join("plain-file"), "").unwrap();

        assert_eq!(
            list_profiles(root.path()).unwrap(),
            vec!["alpha".to_string(), "zeta".to_string()]
        );
    }

    #[test]
    fn delete_ordinary_profile_removes_home_and_management_dirs() {
        let root = tempfile::tempdir().unwrap();
        create_ordinary_profile(root.path(), "default").unwrap();

        delete_ordinary_profile(root.path(), "default", true).unwrap();

        assert!(!root.path().join("default").exists());
        assert!(!root.path().join(".config/default").exists());
    }

    #[test]
    fn delete_ordinary_profile_handles_config_only_leftover() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(".config/work/codex")).unwrap();

        delete_ordinary_profile(root.path(), "work", true).unwrap();

        assert!(!root.path().join(".config/work").exists());
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
        symlink(outside.path(), &p.home_dir).unwrap();

        let err = p.ensure_runnable_profile().unwrap_err().to_string();
        assert!(err.contains("profile home is not a real directory"));
    }

    #[cfg(unix)]
    #[test]
    fn create_ordinary_profile_rejects_symlinked_statusline() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let claude = root.path().join("work/.claude");
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
}
