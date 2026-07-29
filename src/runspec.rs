//! Resolve bind mounts, enforce the host/container boundary, and assemble the
//! `docker run` arguments shared by both agents.

use crate::agent::AgentKind;
use crate::platform;
use anyhow::{bail, Context, Result};
use std::path::{Component, Path, PathBuf};

/// Reject a bind source containing `:` because Docker's `-v` short syntax
/// cannot represent it safely.
pub fn reject_colon_in_bind_source(kind: &str, path: &Path) -> Result<()> {
    let Some(path_str) = path.to_str() else {
        bail!(
            "{kind} path is not valid UTF-8 and cannot be represented safely for docker: {}",
            path.display()
        );
    };
    if path_str.contains(':') {
        bail!(
            "{kind} path contains ':', which docker -v cannot represent: {}",
            path.display()
        );
    }
    Ok(())
}

/// Resolve the work directory to an existing absolute UTF-8 path.
///
/// Relative input is anchored to the process working directory.
pub fn resolve_work_dir(work: Option<&str>) -> Result<String> {
    let cwd = std::env::current_dir().context("get current dir for /work")?;
    let path = match work {
        Some(w) => {
            let p = Path::new(w);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                cwd.join(p)
            }
        }
        None => cwd,
    };
    if !path.is_dir() {
        bail!("work dir is not a directory: {}", path.display());
    }
    reject_colon_in_bind_source("work dir", &path)?;
    Ok(path
        .to_str()
        .context("work dir path is not valid UTF-8")?
        .to_string())
}

/// Parse and resolve user bind mounts into Docker `-v` specifications.
///
/// Sources must exist; relative sources are anchored to the process working
/// directory. Targets must be absolute, and the only accepted mode is `ro`.
/// This function does not enforce managed-target or aibox-root boundaries;
/// callers must subsequently use [`validate_extra_mount_targets`] and
/// [`validate_aibox_mount_sources`].
pub fn resolve_mounts(mounts: &[String]) -> Result<Vec<String>> {
    mounts
        .iter()
        .map(|m| {
            let spec = parse_mount_spec(m)?;
            let p = Path::new(spec.host);
            let host_path = if p.is_absolute() {
                p.to_path_buf()
            } else {
                std::env::current_dir()
                    .context("get current dir for mounts")?
                    .join(p)
            };
            if !host_path.exists() {
                bail!("mount host path does not exist: {}", host_path.display());
            }
            reject_colon_in_bind_source("mount host", &host_path)?;
            let host_path = host_path
                .to_str()
                .context("mount host path is not valid UTF-8")?;
            Ok(format!(
                "{host_path}:{}{}",
                spec.target,
                spec.mode.map(|m| format!(":{m}")).unwrap_or_default()
            ))
        })
        .collect()
}

struct MountSpec<'a> {
    host: &'a str,
    target: &'a str,
    mode: Option<&'a str>,
}

fn parse_mount_spec(mount: &str) -> Result<MountSpec<'_>> {
    let mut parts = mount.split(':');
    let host = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let mode = parts.next();
    if parts.next().is_some() {
        bail!("invalid mount (need host:container[:ro]): {mount}");
    }
    if host.is_empty() || target.is_empty() || !target.starts_with('/') {
        bail!("invalid mount (need host:container[:ro]): {mount}");
    }
    match mode {
        None | Some("ro") => Ok(MountSpec { host, target, mode }),
        Some("") => bail!("invalid mount mode in {mount:?}: use :ro or omit the mode"),
        Some(other) => bail!("invalid mount mode {other:?} in {mount:?}: only :ro is supported"),
    }
}

/// Reject extra mounts that replace `/work`, the agent home, or an ancestor of
/// either managed target.
pub fn validate_extra_mount_targets(agent: AgentKind, mounts: &[String]) -> Result<()> {
    for mount in mounts {
        let target = bind_target(mount)?;
        let target = normalize_container_target(target)?;
        if shadows_managed_target(&target, "/work")
            || shadows_managed_target(&target, agent.container_home())
        {
            bail!(
                "extra mount target {target:?} would override or shadow an aibox-managed mount; choose a nested target instead: {mount}"
            );
        }
    }
    Ok(())
}

/// Keep host-only aibox data out of user-selected bind mounts.
///
/// Sources unrelated to `aibox_root` are allowed. Its ancestors are rejected
/// because mounting one would expose the root indirectly. Sources inside the
/// root must resolve beneath an ordinary profile's `home` subtree; profile
/// roots, management data, tracing data, and the host profile are rejected.
/// Sources are resolved before comparison, so a symlink cannot disguise an
/// aibox-internal target as an unrelated path.
pub fn validate_aibox_mount_sources(
    work_dir: &str,
    extra_mounts: &[String],
    aibox_root: &Path,
) -> Result<()> {
    let resolved_root = canonicalize_existing_prefix(aibox_root)
        .with_context(|| format!("resolve aibox root {}", aibox_root.display()))?;
    let work_source = canonicalize_existing_prefix(Path::new(work_dir))
        .with_context(|| format!("resolve work dir {work_dir}"))?;
    reject_aibox_internal_source(
        "work dir",
        Path::new(work_dir),
        &work_source,
        &resolved_root,
        aibox_root,
    )?;

    for mount in extra_mounts {
        let source = bind_source(mount)?;
        let source_path = Path::new(source);
        let resolved_source = canonicalize_existing_prefix(source_path)
            .with_context(|| format!("resolve mount host path {source}"))?;
        reject_aibox_internal_source(
            "mount host",
            source_path,
            &resolved_source,
            &resolved_root,
            aibox_root,
        )?;
    }
    Ok(())
}

fn reject_aibox_internal_source(
    kind: &str,
    display_path: &Path,
    source: &Path,
    resolved_root: &Path,
    aibox_root: &Path,
) -> Result<()> {
    if resolved_root.starts_with(source) {
        bail!(
            "{kind} would expose aibox internal data: {} overlaps {}",
            display_path.display(),
            aibox_root.display()
        );
    }
    let Ok(relative) = source.strip_prefix(resolved_root) else {
        return Ok(());
    };
    let mut components = relative.components();
    let Some(Component::Normal(profile_name)) = components.next() else {
        bail!(
            "{kind} would expose aibox internal data: {}",
            display_path.display()
        );
    };
    let Some(profile_name) = profile_name.to_str() else {
        bail!(
            "{kind} would expose aibox internal data: {}",
            display_path.display()
        );
    };
    let is_home = matches!(
        components.next(),
        Some(Component::Normal(name)) if name == crate::profile::PROFILE_HOME_DIR
    );
    if !is_home
        || crate::profile::validate_ordinary_profile_name(profile_name).is_err()
        || crate::profile::validate_profile_layout(aibox_root, profile_name).is_err()
    {
        bail!(
            "{kind} would expose aibox internal data: {}; only ordinary profile home trees may be mounted from {}",
            display_path.display(),
            aibox_root.display()
        );
    }
    Ok(())
}

fn bind_source(mount: &str) -> Result<&str> {
    let (source, _) = mount
        .split_once(':')
        .with_context(|| format!("invalid resolved mount: {mount}"))?;
    if source.is_empty() {
        bail!("invalid resolved mount source: {mount}");
    }
    Ok(source)
}

fn bind_target(mount: &str) -> Result<&str> {
    let (_, rest) = mount
        .split_once(':')
        .with_context(|| format!("invalid resolved mount: {mount}"))?;
    let target = rest
        .split_once(':')
        .map(|(target, _)| target)
        .unwrap_or(rest);
    if target.is_empty() {
        bail!("invalid resolved mount target: {mount}");
    }
    Ok(target)
}

fn normalize_container_target(target: &str) -> Result<String> {
    if !target.starts_with('/') {
        bail!("container mount target must be absolute: {target:?}");
    }

    let mut parts = Vec::new();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        Ok("/".to_string())
    } else {
        Ok(format!("/{}", parts.join("/")))
    }
}

fn shadows_managed_target(target: &str, managed: &str) -> bool {
    target == managed
        || target == "/"
        || managed
            .strip_prefix(target)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf> {
    let mut cursor = path;
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(cursor) {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(normalize_path_components(&resolved));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = cursor
                    .file_name()
                    .with_context(|| format!("path does not exist: {}", path.display()))?;
                missing.push(name.to_os_string());
                cursor = cursor
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .with_context(|| format!("path does not exist: {}", path.display()))?;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("resolve {}", path.display()));
            }
        }
    }
}

fn normalize_path_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(_) | Component::RootDir | Component::Prefix(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

/// Seed runtime state required by an agent because the profile mount shadows
/// the image's home directory.
pub fn seed_home(agent: AgentKind, home_dir: &Path) -> Result<()> {
    crate::profile::ensure_agent_state(agent, home_dir)
}

/// Agent-specific command options that remain after aibox parses its own CLI.
pub struct RunOpts<'a> {
    /// Whether to select Codex's headless `exec` subcommand.
    pub exec: bool,
    /// Arguments copied verbatim after the agent executable and run mode.
    pub passthrough: &'a [String],
}

/// Agent command and any agent-specific additions to `docker run`.
pub struct Invocation {
    /// Docker arguments required by this agent before the image name.
    pub extra_run_args: Vec<String>,
    /// Executable and arguments to run inside the container.
    pub agent_cmd: Vec<String>,
}

/// Assemble Docker arguments for the managed home, work tree, and extra mounts.
///
/// Callers must resolve `work_dir` and `extra_mounts` with
/// [`resolve_work_dir`] and [`resolve_mounts`], then apply
/// [`validate_extra_mount_targets`] and [`validate_aibox_mount_sources`].
/// `home_dir` must also pass [`reject_colon_in_bind_source`]. This function is
/// only a pure argument builder and repeats none of those checks.
pub fn assemble_run_args(
    agent: AgentKind,
    work_dir: &str,
    home_dir: &Path,
    extra_mounts: &[String],
    extra_run_args: &[String],
) -> Vec<String> {
    let mut args: Vec<String> = vec!["--rm".into()];

    if platform::has_tty() {
        args.push("-it".into());
    } else {
        args.push("-i".into());
    }

    args.extend(["--security-opt".into(), "no-new-privileges".into()]);
    args.extend(["--cap-drop".into(), "ALL".into()]);

    if platform::is_linux() {
        let (uid, gid) = platform::uid_gid();
        args.push("--user".into());
        args.push(format!("{uid}:{gid}"));
        args.push("--add-host".into());
        args.push("host.docker.internal:host-gateway".into());
    }

    args.push("-v".into());
    args.push(format!("{}:{}", home_dir.display(), agent.container_home()));
    args.push("-v".into());
    args.push(format!("{work_dir}:/work"));
    args.extend(["-w".into(), "/work".into()]);
    for mount in extra_mounts {
        args.push("-v".into());
        args.push(mount.clone());
    }
    args.extend(extra_run_args.iter().cloned());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::contains_pair;
    use std::fs;

    #[test]
    fn resolve_work_dir_none_uses_cwd() {
        let got = resolve_work_dir(None).unwrap();
        assert_eq!(got, std::env::current_dir().unwrap().to_string_lossy());
    }

    #[test]
    fn resolve_work_dir_absolutizes_relative_paths() {
        let cwd = std::env::current_dir().unwrap();

        let got = resolve_work_dir(Some("src")).unwrap();

        assert_eq!(got, cwd.join("src").to_string_lossy());
    }

    #[test]
    fn resolve_mounts_absolutizes_and_validates_host_side() {
        let cwd = std::env::current_dir().unwrap();
        let got =
            resolve_mounts(&["src:/src".to_string(), "src:/readonly:ro".to_string()]).unwrap();
        assert_eq!(
            got,
            vec![
                format!("{}:/src", cwd.join("src").display()),
                format!("{}:/readonly:ro", cwd.join("src").display())
            ]
        );

        assert!(resolve_mounts(&["/no/such/dir:/data".to_string()]).is_err());
        assert!(resolve_mounts(&["src:relative".to_string()]).is_err());
        assert!(resolve_mounts(&["src:/cache:rw".to_string()]).is_err());
    }

    #[test]
    fn resolve_mounts_rejects_malformed_short_syntax() {
        for (mount, expected) in [
            ("src", "invalid mount"),
            (":/cache", "invalid mount"),
            ("src:", "invalid mount"),
            ("src:relative", "invalid mount"),
            ("src:/cache:", "invalid mount mode"),
            ("src:/cache:rw", "only :ro is supported"),
            ("src:/cache:ro:extra", "invalid mount"),
        ] {
            let err = resolve_mounts(&[mount.to_string()])
                .unwrap_err()
                .to_string();
            assert!(
                err.contains(expected),
                "{mount:?} should fail with {expected:?}, got {err:?}"
            );
        }
    }

    #[test]
    fn extra_mounts_must_not_replace_managed_targets() {
        for target in [
            "/work",
            "/work/",
            "/work/.",
            "/tmp/../work",
            "/",
            "/home",
            "/home/aibox",
            "/home/aibox/",
            "/home/aibox/.",
            "/home/aibox/..",
            "/home/aibox/.cache/../..",
        ] {
            let err =
                validate_extra_mount_targets(AgentKind::Codex, &[format!("/host:{target}:ro")])
                    .unwrap_err()
                    .to_string();
            assert!(err.contains("would override or shadow"));
        }
        validate_extra_mount_targets(
            AgentKind::Codex,
            &[
                "/host:/workspace:ro".to_string(),
                "/host:/work-cache:ro".to_string(),
                "/host:/work/.cache:ro".to_string(),
                "/host:/home/aibox-cache:ro".to_string(),
                "/host:/home/aibox2:ro".to_string(),
                "/host:/home/aibox/.cache:ro".to_string(),
            ],
        )
        .unwrap();
    }

    #[test]
    fn aibox_mount_sources_only_allow_ordinary_profile_home_trees() {
        let root = tempfile::tempdir().unwrap();
        let profile_home = root.path().join("default/home");
        let home_child = profile_home.join("projects/demo");
        let profile_config = root.path().join("default/config");
        let tracing = root.path().join("default/tracing");
        let host = root.path().join("host/config");
        fs::create_dir_all(&home_child).unwrap();
        fs::create_dir_all(profile_config.join("codex")).unwrap();
        fs::create_dir(&tracing).unwrap();
        fs::create_dir_all(&host).unwrap();

        for rejected in [
            root.path(),
            &root.path().join("default"),
            &profile_config,
            &profile_config.join("codex"),
            &tracing,
            &root.path().join("host"),
            &host,
            root.path().parent().unwrap(),
        ] {
            let err = validate_aibox_mount_sources(rejected.to_str().unwrap(), &[], root.path())
                .unwrap_err()
                .to_string();
            assert!(err.contains("aibox internal data"), "{rejected:?}: {err}");
        }

        validate_aibox_mount_sources(profile_home.to_str().unwrap(), &[], root.path()).unwrap();
        validate_aibox_mount_sources(home_child.to_str().unwrap(), &[], root.path()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn aibox_mount_check_resolves_symlinked_sources() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let links = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("default/config/codex")).unwrap();
        let linked_config = links.path().join("config");
        symlink(root.path().join("default/config"), &linked_config).unwrap();

        let err = validate_aibox_mount_sources(linked_config.to_str().unwrap(), &[], root.path())
            .unwrap_err()
            .to_string();

        assert!(err.contains("aibox internal data"), "{err}");
    }

    #[test]
    fn aibox_mount_check_normalizes_dotdot_sources() {
        let root = tempfile::tempdir().unwrap();
        let profile_home = root.path().join("default/home");
        let management = root.path().join("default/config");
        fs::create_dir_all(&profile_home).unwrap();
        fs::create_dir(&management).unwrap();

        let work = profile_home.join("..");
        let err = validate_aibox_mount_sources(work.to_str().unwrap(), &[], root.path())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("aibox internal data"),
            "a dotdot work path that resolves to the profile root must fail: {err}"
        );

        let mount_source = profile_home.join("../config");
        let err = validate_aibox_mount_sources(
            profile_home.to_str().unwrap(),
            &[format!("{}:/secrets:ro", mount_source.display())],
            root.path(),
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("aibox internal data"),
            "a dotdot mount source that resolves into config must be rejected: {err}"
        );
    }

    #[test]
    fn aibox_mount_check_allows_unrelated_paths() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        validate_aibox_mount_sources(
            outside.path().to_str().unwrap(),
            &[format!("{}:/archive:ro", outside.path().display())],
            root.path(),
        )
        .unwrap();
    }

    #[test]
    fn assemble_run_args_mounts_shared_home_at_agent_home() {
        let args = assemble_run_args(
            AgentKind::Codex,
            "/abs/work",
            Path::new("/abs/profile"),
            &[],
            &[],
        );
        assert!(contains_pair(&args, "-v", "/abs/profile:/home/aibox"));
        assert!(contains_pair(&args, "-v", "/abs/work:/work"));
        assert!(!args.iter().any(|arg| arg == "--env-file"));
    }

    #[test]
    fn seed_home_creates_agent_state_and_claude_statusline() {
        let home = tempfile::tempdir().unwrap();
        seed_home(AgentKind::Codex, home.path()).unwrap();
        assert!(home.path().join(".codex").is_dir());
        assert!(!home.path().join(".codex/statusline.sh").exists());

        let home = tempfile::tempdir().unwrap();
        seed_home(AgentKind::Claude, home.path()).unwrap();
        let statusline = home.path().join(".claude/statusline.sh");
        assert!(statusline.is_file());
        assert!(fs::read_to_string(statusline)
            .unwrap()
            .contains("context_window"));
    }

    #[test]
    fn reject_bind_source_with_colon() {
        let parent = tempfile::tempdir().unwrap();
        let colon_dir = parent.path().join("a:b");
        fs::create_dir(&colon_dir).unwrap();
        let err = resolve_work_dir(Some(colon_dir.to_str().unwrap()))
            .unwrap_err()
            .to_string();
        assert!(err.contains("contains ':'"));
    }
}
