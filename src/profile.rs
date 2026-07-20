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
use std::io::Write;
use std::path::{Component, Path, PathBuf};

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
    let root = if let Some(root) = crate::env_override("AIBOX_CONFIG_ROOT")? {
        PathBuf::from(root)
    } else {
        let home = crate::env_override("HOME")?.context("$HOME is not set")?;
        agent.config_root_default(&home)
    };
    absolutize(root)
}

/// Docker bind sources must be absolute: a relative source is interpreted as a
/// named volume (or rejected as an invalid volume name). Keep a relative custom
/// config root useful by resolving it against the launch directory, just like
/// `-w` and the host side of `-m`.
fn absolutize(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()
            .context("get current dir for config root")?
            .join(path))
    }
}

/// Validate user-facing names that become one path segment under the config
/// root. Explicit relay paths are handled separately; profile names and named
/// relays must not be able to escape their container directory.
fn validate_path_name(kind: &str, name: &str) -> Result<()> {
    let mut components = Path::new(name).components();
    let safe = matches!(components.next(), Some(Component::Normal(part)) if part.to_str() == Some(name))
        && components.next().is_none()
        && !name.contains('/')
        && !name.contains('\\');
    if safe {
        return Ok(());
    }
    bail!("{kind} name must be a single path segment, not {name:?}");
}

fn validate_relay_name(name: &str) -> Result<()> {
    validate_path_name("relay", name)?;
    if name.starts_with('.') {
        bail!(
            "relay name must not start with '.', use an explicit path for hidden files: {name:?}"
        );
    }
    Ok(())
}

impl Profile {
    /// Resolve the paths for `agent`/`profile` under `root`. Pure — creates
    /// nothing.
    pub fn resolve(agent: AgentKind, root: &Path, profile: &str) -> Result<Self> {
        validate_path_name("profile", profile)?;
        let dir = root.join(profile);
        Ok(Profile {
            agent,
            home_dir: dir.join("home"),
            envs_dir: dir.join("envs"),
            base_file: dir.join("base"),
            dir,
        })
    }

    /// Resolve an `-e <name|path>` argument to a [`RelayRef`], without touching
    /// disk. A value containing `/` or ending in `.env` is a path; otherwise a
    /// name under `envs/`.
    pub fn relay_ref(&self, env: &str) -> Result<RelayRef> {
        if env.contains('/') || env.ends_with(".env") {
            Ok(RelayRef::Path(PathBuf::from(env)))
        } else {
            validate_relay_name(env)?;
            Ok(RelayRef::Named {
                name: env.to_string(),
                path: self.envs_dir.join(env),
            })
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
        let relay = self.relay_ref(env)?;
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
    /// if the dir is absent. Hidden files (`.DS_Store` and friends) are skipped —
    /// they're never relays.
    pub fn relay_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Ok(rd) = fs::read_dir(&self.envs_dir) {
            for entry in rd.flatten() {
                if entry.path().is_file() {
                    if let Some(n) = entry.file_name().to_str() {
                        if !n.starts_with('.') {
                            names.push(n.to_string());
                        }
                    }
                }
            }
        }
        names.sort();
        names
    }

    /// Nudge (without touching the file) when `base` or the relay predates the
    /// current template, so stale docs can be refreshed with `refresh`.
    pub fn nudge_if_stale(&self, relay_path: &Path) {
        let targets = [
            (self.base_file.as_path(), "base".to_string()),
            (relay_path, self.refresh_arg_for(relay_path)),
        ];
        for (f, arg) in targets {
            let Ok(contents) = fs::read_to_string(f) else {
                continue;
            };
            let fv = template::file_template_version(&contents);
            if fv < TEMPLATE_VERSION {
                eprintln!(
                    ">> {} is template v{fv} (current v{TEMPLATE_VERSION}) — refresh docs with: aibox {} refresh {arg}",
                    f.display(),
                    self.agent.tag()
                );
            }
        }
    }

    /// The `refresh` argument that resolves back to `path`: the bare file name
    /// when that name round-trips through [`Self::relay_ref`] to the same path
    /// (a named relay under `envs/`), else the full path — so the hinted command
    /// always targets the file it was printed for. `base` is excluded from the
    /// round-trip: `refresh base` is reserved for the profile's base file, so a
    /// relay that happens to be named `base` must be hinted by path.
    fn refresh_arg_for(&self, path: &Path) -> String {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name != "base"
                && validate_relay_name(name).is_ok()
                && self.envs_dir.join(name) == path
            {
                return name.to_string();
            }
        }
        path.display().to_string()
    }

    /// The merge sources for a run: `base` (if present) then the relay. Returned
    /// as contents so the merge is a pure operation on strings. Keys are
    /// validated here, where each source still has its file name — a bad line
    /// reported later (or by docker) couldn't point back at the file to fix.
    pub fn merge_sources(&self, relay_path: &Path) -> Result<Vec<String>> {
        let mut out = Vec::new();
        if self.base_file.is_file() {
            out.push(read_env_source(&self.base_file)?);
        }
        out.push(read_env_source(relay_path)?);
        Ok(out)
    }
}

/// Read one env-file source and validate its keys against the file it came
/// from (see [`crate::envfile::check_keys`]).
fn read_env_source(path: &Path) -> Result<String> {
    let contents = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    crate::envfile::check_keys(path, &contents)?;
    Ok(contents)
}

/// Write `contents` to `path` with 0600 permissions. The complete replacement
/// is prepared beside the target and then atomically persisted, so a short
/// write, disk error, or process interruption cannot leave a credential file
/// half-truncated. Existing symlinks are resolved first so refreshing a
/// deliberately shared config updates its target rather than replacing the
/// link itself.
pub fn write_600(path: &Path, contents: &str) -> Result<()> {
    let target = match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => fs::canonicalize(path)
            .with_context(|| format!("resolve config symlink {}", path.display()))?,
        Ok(meta) if !meta.is_file() => {
            bail!("config path is not a file: {}", path.display());
        }
        Ok(_) => path.to_path_buf(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => path.to_path_buf(),
        Err(e) => return Err(e).with_context(|| format!("inspect {}", path.display())),
    };

    if target.exists() && !target.is_file() {
        bail!("config path is not a file: {}", target.display());
    }

    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut replacement = tempfile::Builder::new()
        .prefix(".aibox-write.")
        .tempfile_in(parent)
        .with_context(|| format!("create replacement beside {}", target.display()))?;
    set_600(replacement.path())?;
    replacement
        .write_all(contents.as_bytes())
        .with_context(|| format!("write replacement for {}", target.display()))?;
    replacement
        .as_file()
        .sync_all()
        .with_context(|| format!("sync replacement for {}", target.display()))?;
    replacement
        .persist(&target)
        .map_err(|e| e.error)
        .with_context(|| format!("replace {}", target.display()))?;
    Ok(())
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
        let p = Profile::resolve(AgentKind::Claude, Path::new("/root"), "default").unwrap();
        assert_eq!(p.dir, Path::new("/root/default"));
        assert_eq!(p.home_dir, Path::new("/root/default/home"));
        assert_eq!(p.envs_dir, Path::new("/root/default/envs"));
        assert_eq!(p.base_file, Path::new("/root/default/base"));
    }

    #[test]
    fn relative_config_roots_are_absolutized_for_docker_mounts() {
        let got = absolutize(PathBuf::from("relative-root")).unwrap();
        assert_eq!(got, std::env::current_dir().unwrap().join("relative-root"));
        assert!(absolutize(PathBuf::from("/absolute-root"))
            .unwrap()
            .is_absolute());
    }

    #[test]
    fn profile_name_must_be_one_safe_path_segment() {
        assert!(Profile::resolve(AgentKind::Claude, Path::new("/root"), "default").is_ok());
        assert!(Profile::resolve(AgentKind::Claude, Path::new("/root"), ".hidden").is_ok());

        for bad in ["", ".", "..", "a/b", "a\\b"] {
            let err = Profile::resolve(AgentKind::Claude, Path::new("/root"), bad)
                .map(|_| ())
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("profile name must be a single path segment"),
                "bad profile {bad:?} should be rejected clearly: {err}"
            );
        }
    }

    #[test]
    fn relay_ref_name_vs_path() {
        let p = Profile::resolve(AgentKind::Codex, Path::new("/root"), "default").unwrap();
        assert_eq!(
            p.relay_ref("openrouter").unwrap(),
            RelayRef::Named {
                name: "openrouter".into(),
                path: PathBuf::from("/root/default/envs/openrouter")
            }
        );
        assert_eq!(
            p.relay_ref("/abs/path").unwrap(),
            RelayRef::Path(PathBuf::from("/abs/path"))
        );
        assert_eq!(
            p.relay_ref("my.env").unwrap(),
            RelayRef::Path(PathBuf::from("my.env"))
        );
    }

    #[test]
    fn named_relay_must_be_one_safe_path_segment() {
        let p = Profile::resolve(AgentKind::Codex, Path::new("/root"), "default").unwrap();

        for bad in ["", ".", "..", "a\\b"] {
            let err = p.relay_ref(bad).unwrap_err().to_string();
            assert!(
                err.contains("relay name must be a single path segment"),
                "bad relay {bad:?} should be rejected clearly: {err}"
            );
        }
        let err = p.relay_ref(".hidden").unwrap_err().to_string();
        assert!(
            err.contains("relay name must not start with '.'"),
            "hidden relay names should be rejected clearly: {err}"
        );

        assert_eq!(
            p.relay_ref("nested/relay").unwrap(),
            RelayRef::Path(PathBuf::from("nested/relay")),
            "values containing / remain explicit paths, not named relays"
        );
        assert_eq!(
            p.relay_ref("../relay.env").unwrap(),
            RelayRef::Path(PathBuf::from("../relay.env")),
            "explicit path relays keep their existing behavior"
        );
        assert_eq!(
            p.relay_ref("./.hidden").unwrap(),
            RelayRef::Path(PathBuf::from("./.hidden")),
            "hidden files are still reachable as explicit paths"
        );
    }

    #[test]
    fn scaffold_creates_base_and_relay_then_signals_stop() {
        let root = tmp();
        let p = Profile::resolve(AgentKind::Claude, root.path(), "default").unwrap();
        let got = p.resolve_relay_for_run("openrouter").unwrap();
        assert!(got.is_none(), "first use should scaffold and stop");
        assert!(p.base_file.is_file());
        assert!(p.envs_dir.join("openrouter").is_file());
    }

    #[test]
    fn existing_relay_resolves() {
        let root = tmp();
        let p = Profile::resolve(AgentKind::Claude, root.path(), "default").unwrap();
        p.resolve_relay_for_run("r").unwrap(); // scaffold
        let got = p.resolve_relay_for_run("r").unwrap();
        assert!(got.is_some(), "second use should resolve the existing file");
    }

    #[test]
    fn missing_explicit_path_errors() {
        let root = tmp();
        let p = Profile::resolve(AgentKind::Claude, root.path(), "default").unwrap();
        assert!(p.resolve_relay_for_run("/no/such/file.env").is_err());
    }

    #[test]
    fn refresh_arg_round_trips_named_relay_and_falls_back_to_path() {
        let p = Profile::resolve(AgentKind::Claude, Path::new("/root"), "default").unwrap();
        // A named relay under envs/ hints its bare name.
        assert_eq!(
            p.refresh_arg_for(Path::new("/root/default/envs/openrouter")),
            "openrouter"
        );
        // A path-style relay hints the full path (its bare name wouldn't
        // resolve back to the same file).
        assert_eq!(p.refresh_arg_for(Path::new("/tmp/x.env")), "/tmp/x.env");
        // A relay literally named `base` also hints the full path: the bare
        // name would make `refresh base` hit the profile's base file instead.
        assert_eq!(
            p.refresh_arg_for(Path::new("/root/default/envs/base")),
            "/root/default/envs/base"
        );
    }

    #[test]
    fn relay_names_skips_hidden_files() {
        let root = tmp();
        let p = Profile::resolve(AgentKind::Claude, root.path(), "default").unwrap();
        p.resolve_relay_for_run("r").unwrap(); // scaffold
        fs::write(p.envs_dir.join(".DS_Store"), b"junk").unwrap();
        assert_eq!(p.relay_names(), vec!["r".to_string()]);
    }

    #[test]
    fn merge_sources_rejects_malformed_key_naming_the_file() {
        let root = tmp();
        let p = Profile::resolve(AgentKind::Codex, root.path(), "default").unwrap();
        p.resolve_relay_for_run("r").unwrap(); // scaffold base + relay
        let relay = p.envs_dir.join("r");
        fs::write(&relay, "CODEX_API_KEY = sk-x\n").unwrap();

        let err = p.merge_sources(&relay).unwrap_err().to_string();

        assert!(err.contains(&relay.display().to_string()), "{err}");
        assert!(err.contains("CODEX_API_KEY = sk-x"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn write_600_creates_and_restricts_existing_files() {
        use std::os::unix::fs::PermissionsExt;

        let root = tmp();
        let fresh = root.path().join("fresh");
        write_600(&fresh, "secret\n").unwrap();
        assert_eq!(fs::read_to_string(&fresh).unwrap(), "secret\n");
        let mode = fs::metadata(&fresh).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);

        let existing = root.path().join("existing");
        fs::write(&existing, "old\n").unwrap();
        fs::set_permissions(&existing, fs::Permissions::from_mode(0o644)).unwrap();
        write_600(&existing, "new-secret\n").unwrap();
        assert_eq!(fs::read_to_string(&existing).unwrap(), "new-secret\n");
        let mode = fs::metadata(&existing).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn write_600_rejects_directories_without_changing_their_mode() {
        use std::os::unix::fs::PermissionsExt;

        let root = tmp();
        let dir = root.path().join("not-a-config-file");
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();

        let err = write_600(&dir, "secret\n").unwrap_err().to_string();

        assert!(err.contains("config path is not a file"));
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }

    #[cfg(unix)]
    #[test]
    fn write_600_refreshes_a_symlink_target_without_replacing_the_link() {
        use std::os::unix::fs::symlink;

        let root = tmp();
        let target = root.path().join("shared.env");
        let link = root.path().join("relay.env");
        fs::write(&target, "old\n").unwrap();
        symlink(&target, &link).unwrap();

        write_600(&link, "new\n").unwrap();

        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
    }

    #[cfg(unix)]
    #[test]
    fn scaffolded_files_are_0600() {
        use std::os::unix::fs::PermissionsExt;
        let root = tmp();
        let p = Profile::resolve(AgentKind::Codex, root.path(), "default").unwrap();
        p.resolve_relay_for_run("r").unwrap();
        let mode = fs::metadata(&p.base_file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
