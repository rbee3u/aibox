//! Per-profile path layout, directory creation, config root resolution, and
//! relay resolution / scaffolding.
//!
//! Everything is per-profile on the host, under `$AIBOX_CONFIG_ROOT` (default
//! `$HOME/.aibox/<agent>`):
//!
//! ```text
//!   <root>/<profile>/
//!     ├── base        # shared config inherited by every relay
//!     ├── envs/       # relay endpoints, pick one per run with -e <name>
//!     └── home/       # mounted as the agent's home
//! ```

use crate::agent::{AgentKind, TEMPLATE_VERSION};
use crate::template;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Resolved per-profile paths. Built from the agent, the config root, and the
/// profile name.
pub struct Profile {
    pub agent: AgentKind,
    pub dir: PathBuf,
    pub home_dir: PathBuf,
    pub envs_dir: PathBuf,
    pub base_file: PathBuf,
}

/// How a relay endpoint was resolved. A bare name lives under `envs/` and may be
/// scaffolded on first use; a path (contains `/` or ends `.env`) is taken as-is
/// and never scaffolded.
#[derive(Debug, PartialEq, Eq)]
pub enum RelayRef {
    /// A name under `<profile>/envs/`; may be scaffolded.
    Named { name: String, path: PathBuf },
    /// An explicit path; must already exist.
    Path(PathBuf),
}

impl RelayRef {
    pub fn path(&self) -> &Path {
        match self {
            RelayRef::Named { path, .. } => path,
            RelayRef::Path(p) => p,
        }
    }
}

/// The config root: `$AIBOX_CONFIG_ROOT` if set, else `$HOME/.aibox/<agent>`.
pub fn config_root(agent: AgentKind) -> Result<PathBuf> {
    if let Ok(root) = std::env::var("AIBOX_CONFIG_ROOT") {
        return Ok(PathBuf::from(root));
    }
    let home = std::env::var("HOME").context("$HOME is not set")?;
    Ok(agent.config_root_default(&home))
}

impl Profile {
    /// Resolve the paths for `agent`/`profile` under `root`. Pure — creates
    /// nothing.
    pub fn resolve(agent: AgentKind, root: &Path, profile: &str) -> Self {
        let dir = root.join(profile);
        Profile {
            agent,
            home_dir: dir.join("home"),
            envs_dir: dir.join("envs"),
            base_file: dir.join("base"),
            dir,
        }
    }

    /// Resolve an `-e <name|path>` argument to a [`RelayRef`], without touching
    /// disk. A value containing `/` or ending in `.env` is a path; otherwise a
    /// name under `envs/`.
    pub fn relay_ref(&self, env: &str) -> RelayRef {
        if env.contains('/') || env.ends_with(".env") {
            RelayRef::Path(PathBuf::from(env))
        } else {
            RelayRef::Named {
                name: env.to_string(),
                path: self.envs_dir.join(env),
            }
        }
    }

    /// Ensure the profile home exists (created before `docker run` so the mount
    /// doesn't shadow an image path with a root-owned empty dir).
    pub fn ensure_home(&self) -> Result<()> {
        fs::create_dir_all(&self.home_dir)
            .with_context(|| format!("create profile home {}", self.home_dir.display()))
    }

    /// Resolve the relay for a run, scaffolding a named relay (and `base`) on
    /// first use. Returns:
    /// - `Ok(Some(relay))` — the relay file exists and is ready to use;
    /// - `Ok(None)` — we just scaffolded a stub and the caller should stop so the
    ///   user can fill in credentials;
    /// - `Err` — an explicit path that doesn't exist.
    pub fn resolve_relay_for_run(&self, env: &str) -> Result<Option<RelayRef>> {
        let relay = self.relay_ref(env);
        if relay.path().is_file() {
            return Ok(Some(relay));
        }
        match &relay {
            RelayRef::Path(p) => {
                bail!("env file not found: {}", p.display());
            }
            RelayRef::Named { name, path } => {
                self.scaffold(name, path)?;
                Ok(None)
            }
        }
    }

    /// Write a `base` (once) plus a relay stub, then leave it to the caller to
    /// stop. Files are 0600.
    fn scaffold(&self, name: &str, relay_path: &Path) -> Result<()> {
        fs::create_dir_all(&self.envs_dir)
            .with_context(|| format!("create {}", self.envs_dir.display()))?;
        if !self.base_file.is_file() {
            write_600(
                &self.base_file,
                &template::base_template(self.agent, TEMPLATE_VERSION),
            )?;
        }
        write_600(
            relay_path,
            &template::relay_template(self.agent, name, TEMPLATE_VERSION),
        )?;
        eprintln!(
            ">> scaffolded {} and {}",
            self.base_file.display(),
            relay_path.display()
        );
        eprintln!(
            ">> edit the credentials, then re-run: aibox {} -e {}",
            self.agent.tag(),
            name
        );
        Ok(())
    }

    /// List relay names under `envs/` (for the "no relay selected" hint). Empty
    /// if the dir is absent.
    pub fn relay_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Ok(rd) = fs::read_dir(&self.envs_dir) {
            for entry in rd.flatten() {
                if entry.path().is_file() {
                    if let Some(n) = entry.file_name().to_str() {
                        names.push(n.to_string());
                    }
                }
            }
        }
        names.sort();
        names
    }

    /// Nudge (without touching the file) when `base` or the relay predates the
    /// current template, so stale docs can be refreshed with `sync`.
    pub fn nudge_if_stale(&self, relay_path: &Path) {
        for f in [self.base_file.as_path(), relay_path] {
            let Ok(contents) = fs::read_to_string(f) else {
                continue;
            };
            let fv = template::file_template_version(&contents);
            if fv < TEMPLATE_VERSION {
                let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("");
                eprintln!(
                    ">> {} is template v{fv} (current v{TEMPLATE_VERSION}) — refresh docs with: aibox {} sync {name}",
                    f.display(),
                    self.agent.tag()
                );
            }
        }
    }

    /// The merge sources for a run: `base` (if present) then the relay. Returned
    /// as contents so the merge is a pure operation on strings.
    pub fn merge_sources(&self, relay_path: &Path) -> Result<Vec<String>> {
        let mut out = Vec::new();
        if self.base_file.is_file() {
            out.push(
                fs::read_to_string(&self.base_file)
                    .with_context(|| format!("read {}", self.base_file.display()))?,
            );
        }
        out.push(
            fs::read_to_string(relay_path)
                .with_context(|| format!("read {}", relay_path.display()))?,
        );
        Ok(out)
    }
}

/// Write `contents` to `path` with 0600 permissions (create or truncate). Used
/// for every scaffolded config file.
pub fn write_600(path: &Path, contents: &str) -> Result<()> {
    fs::write(path, contents).with_context(|| format!("write {}", path.display()))?;
    set_600(path)
}

/// chmod 0600 on Unix; a no-op elsewhere.
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

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn path_layout() {
        let p = Profile::resolve(AgentKind::Claude, Path::new("/root"), "default");
        assert_eq!(p.dir, Path::new("/root/default"));
        assert_eq!(p.home_dir, Path::new("/root/default/home"));
        assert_eq!(p.envs_dir, Path::new("/root/default/envs"));
        assert_eq!(p.base_file, Path::new("/root/default/base"));
    }

    #[test]
    fn relay_ref_name_vs_path() {
        let p = Profile::resolve(AgentKind::Codex, Path::new("/root"), "default");
        assert_eq!(
            p.relay_ref("openrouter"),
            RelayRef::Named {
                name: "openrouter".into(),
                path: PathBuf::from("/root/default/envs/openrouter")
            }
        );
        assert_eq!(
            p.relay_ref("/abs/path"),
            RelayRef::Path(PathBuf::from("/abs/path"))
        );
        assert_eq!(
            p.relay_ref("my.env"),
            RelayRef::Path(PathBuf::from("my.env"))
        );
    }

    #[test]
    fn scaffold_creates_base_and_relay_then_signals_stop() {
        let root = tmp();
        let p = Profile::resolve(AgentKind::Claude, root.path(), "default");
        let got = p.resolve_relay_for_run("openrouter").unwrap();
        assert!(got.is_none(), "first use should scaffold and stop");
        assert!(p.base_file.is_file());
        assert!(p.envs_dir.join("openrouter").is_file());
    }

    #[test]
    fn existing_relay_resolves() {
        let root = tmp();
        let p = Profile::resolve(AgentKind::Claude, root.path(), "default");
        p.resolve_relay_for_run("r").unwrap(); // scaffold
        let got = p.resolve_relay_for_run("r").unwrap();
        assert!(got.is_some(), "second use should resolve the existing file");
    }

    #[test]
    fn missing_explicit_path_errors() {
        let root = tmp();
        let p = Profile::resolve(AgentKind::Claude, root.path(), "default");
        assert!(p.resolve_relay_for_run("/no/such/file.env").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn scaffolded_files_are_0600() {
        use std::os::unix::fs::PermissionsExt;
        let root = tmp();
        let p = Profile::resolve(AgentKind::Codex, root.path(), "default");
        p.resolve_relay_for_run("r").unwrap();
        let mode = fs::metadata(&p.base_file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
