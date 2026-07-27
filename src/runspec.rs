//! Assembling the `docker run` invocation shared by both agents.

use crate::agent::AgentKind;
use crate::platform;
use anyhow::{bail, Context, Result};
use std::path::Path;

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

/// Seed runtime state required by an agent because the profile mount shadows
/// the image's home directory.
pub fn seed_home(agent: AgentKind, home_dir: &Path) -> Result<()> {
    crate::profile::ensure_agent_state(agent, home_dir)
}

pub struct RunOpts<'a> {
    pub safe: bool,
    pub exec: bool,
    pub passthrough: &'a [String],
}

pub struct Invocation {
    pub extra_run_args: Vec<String>,
    pub agent_cmd: Vec<String>,
}

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
    fn resolve_mounts_absolutizes_and_validates_host_side() {
        let got = resolve_mounts(&["src:/src".to_string()]).unwrap();
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(got, vec![format!("{}:/src", cwd.join("src").display())]);

        assert!(resolve_mounts(&["/no/such/dir:/data".to_string()]).is_err());
        assert!(resolve_mounts(&["src:relative".to_string()]).is_err());
        assert!(resolve_mounts(&["src:/cache:rw".to_string()]).is_err());
    }

    #[test]
    fn extra_mounts_must_not_replace_managed_targets() {
        for target in ["/work", "/", "/home", "/home/codex", "/home/codex/.."] {
            let err =
                validate_extra_mount_targets(AgentKind::Codex, &[format!("/host:{target}:ro")])
                    .unwrap_err()
                    .to_string();
            assert!(err.contains("would override or shadow"));
        }
        validate_extra_mount_targets(
            AgentKind::Codex,
            &["/host:/home/codex/.cache:ro".to_string()],
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
        assert!(contains_pair(&args, "-v", "/abs/profile:/home/codex"));
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
