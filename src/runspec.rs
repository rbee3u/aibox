//! Assembling the `docker run` invocation shared by both agents.
//!
//! The shared `docker run` tail: the hardening flags, TTY probe, Linux uid/gid +
//! host-gateway, and the home/`/work`/extra mounts. What *differs* between the
//! agents — credentials, endpoint wiring, and the agent command line — is
//! produced by [`crate::agent::AgentKind::build_invocation`] and folded in here.

use crate::agent::AgentKind;
use crate::creds::StagedFile;
use crate::platform;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;

/// The Claude status-line script, embedded so a fresh profile home gets it
/// seeded on first run (the home mount shadows the image, so it must land on the
/// host). Runs inside the container against its `jq`; stays Bash on purpose.
const CLAUDE_STATUS_SH: &str = include_str!("../assets/claude-status.sh");

/// Default Claude settings.json wiring the status line, written only if absent.
const CLAUDE_SETTINGS: &str = r#"{
  "statusLine": {
    "type": "command",
    "command": "bash /home/claude/.claude/statusline.sh"
  }
}
"#;

/// Resolve the `-w` work dir (or the launch cwd when absent) to an absolute
/// path and require an existing directory. Docker reads a bare name (no `/`)
/// as a *named volume*, so passing a relative path through would silently
/// mount an empty volume at `/work` instead of the project.
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
    Ok(path.to_string_lossy().into_owned())
}

/// Resolve the host side of each `-m host:container[:ro]` mount to an absolute
/// path (against the launch cwd, like `-w`) and require it to exist. Same trap
/// as `/work`: Docker reads a bare relative name as a *named volume* and would
/// silently mount an empty one at the container path. The container side must be
/// present and absolute, matching Docker's bind-mount target rules.
pub fn resolve_mounts(mounts: &[String]) -> Result<Vec<String>> {
    mounts
        .iter()
        .map(|m| {
            let (host, rest) = m
                .split_once(':')
                .filter(|(host, rest)| !host.is_empty() && rest.starts_with('/'))
                .with_context(|| format!("invalid mount (need host:container[:ro]): {m}"))?;
            let p = Path::new(host);
            let host_path = if p.is_absolute() {
                p.to_path_buf()
            } else {
                let cwd = std::env::current_dir().context("get current dir for mounts")?;
                cwd.join(p)
            };
            if !host_path.exists() {
                bail!("mount host path does not exist: {}", host_path.display());
            }
            Ok(format!("{}:{rest}", host_path.display()))
        })
        .collect()
}

/// Seed agent-specific first-run files into a profile home. First use only:
/// existing files are left untouched so customizations survive.
///
/// - Claude: the status-line script + a `settings.json` wiring it.
/// - Codex: `.codex/` (CODEX_HOME), which Codex refuses to start without and
///   which the mount would otherwise shadow.
pub fn seed_home(agent: AgentKind, home_dir: &Path) -> Result<()> {
    match agent {
        AgentKind::Claude => {
            let claude_dir = home_dir.join(".claude");
            fs::create_dir_all(&claude_dir)
                .with_context(|| format!("create {}", claude_dir.display()))?;
            let status_dst = claude_dir.join("statusline.sh");
            if !status_dst.exists() {
                fs::write(&status_dst, CLAUDE_STATUS_SH)
                    .with_context(|| format!("seed {}", status_dst.display()))?;
            }
            let settings = claude_dir.join("settings.json");
            if !settings.exists() {
                fs::write(&settings, CLAUDE_SETTINGS)
                    .with_context(|| format!("seed {}", settings.display()))?;
            }
        }
        AgentKind::Codex => {
            // Codex refuses to start if CODEX_HOME (=/home/codex/.codex) is not a
            // directory; the mount shadows the image copy, so create it host-side.
            let codex_dir = home_dir.join(".codex");
            fs::create_dir_all(&codex_dir)
                .with_context(|| format!("create {}", codex_dir.display()))?;
        }
    }
    Ok(())
}

/// Inputs to a run that the agent-specific invocation builder needs, gathered
/// once by the orchestrator.
pub struct RunOpts<'a> {
    /// Merged `base` + relay config.
    pub env: &'a crate::envfile::MergedEnv,
    /// `--safe`: keep the agent's own prompts/sandbox instead of bypassing.
    pub safe: bool,
    /// Codex `--exec` headless mode (Claude is rejected before invocation build).
    pub exec: bool,
    /// Pass-through args after `--`, handed to the agent verbatim.
    pub passthrough: &'a [String],
    /// The profile home dir on the host (mounted at the container home).
    pub home_dir: &'a Path,
}

/// What an agent wants from a run, after translating its relay config. Combines
/// with the shared Docker flags in [`assemble_run_args`].
pub struct Invocation {
    /// Extra `docker run` args the agent needs: Claude's merged `--env-file`;
    /// Codex's key `--env-file`, or a read-only `auth.json` / instructions mount.
    pub extra_run_args: Vec<String>,
    /// The agent command line (after the image): e.g. `--dangerously-skip-permissions`
    /// plus pass-through, or Codex's `-c` overrides.
    pub agent_cmd: Vec<String>,
    /// Staged credential files to keep alive until `docker run` returns. Dropping
    /// these unlinks them; they're held by the caller for the run's duration.
    pub staged: Vec<StagedFile>,
    /// Guarded fixed-path files (Codex's pre-created `auth.json` mount target),
    /// removed on drop only if we created them. Held for the run's duration.
    pub guarded: Vec<crate::creds::GuardedPath>,
}

/// Build the full `docker run` argument list (everything between `docker run`
/// and the image): `--rm`, `-it`/`-i`, hardening, Linux uid/gid + host-gateway,
/// the home / `/work` / extra mounts, then the agent's credential/auth args.
///
/// Agent-specific args come last because Codex's auth.json mode mounts a file
/// nested under the profile home. Keeping that nested mount after the parent
/// home mount avoids any runtime that applies bind mounts in argv order from
/// shadowing the credential with the parent directory mount.
pub fn assemble_run_args(
    agent: AgentKind,
    work_dir: &str,
    home_dir: &Path,
    extra_mounts: &[String],
    invocation_extra: &[String],
) -> Vec<String> {
    let mut a: Vec<String> = vec!["--rm".into()];

    // Interactive TTY only when we actually have one (so pipes still work).
    if platform::has_tty() {
        a.push("-it".into());
    } else {
        a.push("-i".into());
    }

    // Hardening: no privilege escalation, drop all Linux capabilities.
    a.extend(["--security-opt".into(), "no-new-privileges".into()]);
    a.extend(["--cap-drop".into(), "ALL".into()]);

    // Linux only: run as the host uid/gid so files created in /work stay yours,
    // and map host.docker.internal so a relay/proxy on the host is reachable.
    if platform::is_linux() {
        let (uid, gid) = platform::uid_gid();
        a.push("--user".into());
        a.push(format!("{uid}:{gid}"));
        a.push("--add-host".into());
        a.push("host.docker.internal:host-gateway".into());
    }

    // Mounts: isolated home + the project at /work + any extras.
    a.push("-v".into());
    a.push(format!("{}:{}", home_dir.display(), agent.container_home()));
    a.push("-v".into());
    a.push(format!("{work_dir}:/work"));
    a.extend(["-w".into(), "/work".into()]);
    for m in extra_mounts {
        a.push("-v".into());
        a.push(m.clone());
    }

    // Agent-specific credential/auth args (env-files, auth.json mount).
    a.extend(invocation_extra.iter().cloned());

    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_work_dir_absolute_dir_passes() {
        let dir = tempfile::tempdir().unwrap();
        let got = resolve_work_dir(Some(dir.path().to_str().unwrap())).unwrap();
        assert_eq!(got, dir.path().to_string_lossy());
    }

    #[test]
    fn resolve_work_dir_relative_resolves_against_cwd() {
        // `src` exists relative to the crate root, where cargo runs tests.
        let got = resolve_work_dir(Some("src")).unwrap();
        let p = Path::new(&got);
        assert!(p.is_absolute());
        assert_eq!(p, std::env::current_dir().unwrap().join("src"));
    }

    #[test]
    fn resolve_work_dir_none_uses_cwd() {
        let got = resolve_work_dir(None).unwrap();
        assert_eq!(got, std::env::current_dir().unwrap().to_string_lossy());
    }

    #[test]
    fn resolve_work_dir_missing_or_file_errors() {
        assert!(resolve_work_dir(Some("/no/such/dir")).is_err());
        let f = tempfile::NamedTempFile::new().unwrap();
        assert!(resolve_work_dir(Some(f.path().to_str().unwrap())).is_err());
    }

    #[test]
    fn resolve_mounts_absolutizes_and_validates_host_side() {
        let dir = tempfile::tempdir().unwrap();
        let host = dir.path().display();

        // Absolute host path passes through unchanged, options intact.
        let got = resolve_mounts(&[format!("{host}:/cache:ro")]).unwrap();
        assert_eq!(got, vec![format!("{host}:/cache:ro")]);

        // Relative host path resolves against the launch cwd (like -w). `src`
        // exists relative to the crate root, where cargo runs tests.
        let got = resolve_mounts(&["src:/src".to_string()]).unwrap();
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(got, vec![format!("{}:/src", cwd.join("src").display())]);

        // A missing host path errors instead of becoming an empty named volume.
        let err = resolve_mounts(&["/no/such/dir:/data".to_string()]).unwrap_err();
        assert!(err.to_string().contains("does not exist"));
        let err = resolve_mounts(&["no-such-dir:/data".to_string()]).unwrap_err();
        assert!(err.to_string().contains("does not exist"));

        // No host part at all is a usage error, not a silent volume mount.
        assert!(resolve_mounts(&["/data".to_string()]).is_err());
        assert!(resolve_mounts(&[":/data".to_string()]).is_err());
        // The container target must be present and absolute.
        assert!(resolve_mounts(&["src:".to_string()]).is_err());
        assert!(resolve_mounts(&["src:relative".to_string()]).is_err());
    }

    fn contains_pair(args: &[String], a: &str, b: &str) -> bool {
        args.windows(2).any(|w| w[0] == a && w[1] == b)
    }

    fn pair_pos(args: &[String], a: &str, b: &str) -> Option<usize> {
        args.windows(2).position(|w| w[0] == a && w[1] == b)
    }

    #[test]
    fn assemble_run_args_hardening_and_mount_order() {
        let extra = vec!["--env-file".to_string(), "/tmp/aibox-env.x".to_string()];
        let mounts = vec!["/host/cache:/cache:ro".to_string()];

        let args = assemble_run_args(
            AgentKind::Claude,
            "/abs/work",
            Path::new("/abs/home"),
            &mounts,
            &extra,
        );

        assert_eq!(args[0], "--rm");
        let tty = if platform::has_tty() { "-it" } else { "-i" };
        assert_eq!(args[1], tty);

        // The container is the sandbox boundary; the hardening flags must
        // survive any reshuffling of this assembly.
        assert!(contains_pair(&args, "--security-opt", "no-new-privileges"));
        assert!(contains_pair(&args, "--cap-drop", "ALL"));

        assert!(contains_pair(&args, "-v", "/abs/home:/home/claude"));
        assert!(contains_pair(&args, "-v", "/abs/work:/work"));
        assert!(contains_pair(&args, "-w", "/work"));
        assert!(contains_pair(&args, "-v", "/host/cache:/cache:ro"));
        assert_eq!(
            &args[args.len() - 2..],
            ["--env-file", "/tmp/aibox-env.x"],
            "invocation extras come last so nested credential mounts cannot be shadowed"
        );

        // Linux-only flags mirror the host platform probes.
        assert_eq!(
            args.iter().any(|a| a == "--user"),
            platform::is_linux(),
            "--user is Linux-only"
        );
        assert_eq!(
            contains_pair(&args, "--add-host", "host.docker.internal:host-gateway"),
            platform::is_linux(),
            "--add-host is Linux-only"
        );
    }

    #[test]
    fn assemble_run_args_mounts_home_at_agent_container_home() {
        let args = assemble_run_args(
            AgentKind::Codex,
            "/abs/work",
            Path::new("/abs/home"),
            &[],
            &[],
        );
        assert!(contains_pair(&args, "-v", "/abs/home:/home/codex"));
    }

    #[test]
    fn assemble_run_args_places_nested_invocation_mount_after_home_mount() {
        let auth_mount = "/tmp/auth.json:/home/codex/.codex/auth.json:ro";
        let args = assemble_run_args(
            AgentKind::Codex,
            "/abs/work",
            Path::new("/abs/home"),
            &[],
            &["-v".to_string(), auth_mount.to_string()],
        );

        let home = pair_pos(&args, "-v", "/abs/home:/home/codex").expect("home mount");
        let auth = pair_pos(&args, "-v", auth_mount).expect("auth mount");
        assert!(
            home < auth,
            "nested auth.json mount must be listed after the parent home mount"
        );
    }

    #[test]
    fn seed_home_claude_seeds_once_and_preserves_customizations() {
        let home = tempfile::tempdir().unwrap();

        seed_home(AgentKind::Claude, home.path()).unwrap();
        let status = home.path().join(".claude").join("statusline.sh");
        let settings = home.path().join(".claude").join("settings.json");
        assert_eq!(fs::read_to_string(&status).unwrap(), CLAUDE_STATUS_SH);
        assert_eq!(fs::read_to_string(&settings).unwrap(), CLAUDE_SETTINGS);

        // A second run must not clobber user customizations.
        fs::write(&status, "my custom status").unwrap();
        fs::write(&settings, "{\"mine\":true}\n").unwrap();
        seed_home(AgentKind::Claude, home.path()).unwrap();
        assert_eq!(fs::read_to_string(&status).unwrap(), "my custom status");
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{\"mine\":true}\n");
    }

    #[test]
    fn seed_home_codex_creates_codex_home_dir() {
        let home = tempfile::tempdir().unwrap();
        seed_home(AgentKind::Codex, home.path()).unwrap();
        assert!(home.path().join(".codex").is_dir());
    }
}
