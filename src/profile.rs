//! Profile and config-management path layout.

use crate::agent::AgentKind;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub const HOST_PROFILE: &str = "host";

#[derive(Debug, Clone)]
pub struct Profile {
    pub agent: AgentKind,
    pub name: String,
    pub home_dir: PathBuf,
    pub active_agent_dir: PathBuf,
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
        ensure_real_dir(&self.home_dir, "profile home")
    }

    pub fn ensure_active_agent_dir(&self) -> Result<()> {
        if self.is_host {
            if !real_dir_exists(&self.home_dir, "host home")? {
                bail!("host home does not exist: {}", self.home_dir.display());
            }
        } else {
            ensure_real_dir(&self.home_dir, "profile home")?;
        }
        ensure_real_dir(&self.active_agent_dir, "agent config directory")
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
        ensure_real_dir(&self.management_dir, "config management directory")
    }
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
}
