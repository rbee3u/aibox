//! Assembling the `docker run` invocation shared by both agents.
//!
//! The shared `docker run` tail: the hardening flags, TTY probe, Linux uid/gid +
//! host-gateway, the home/`/work`/ extra mounts, and the permission-bypass
//! toggle. What *differs* between the
//! agents — how the endpoint is wired and what the agent command line looks like
//! — is produced by [`crate::agent::AgentKind::build_invocation`] and folded in
//! here.

use crate::agent::AgentKind;
use crate::creds::StagedFile;
use crate::platform;
use anyhow::{Context, Result};
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

/// Seed agent-specific first-run files into a profile home. First use only:
/// existing files are left untouched so customizations survive.
///
/// - Claude: the status-line script + a `settings.json` wiring it.
/// - Codex: `.codex/` (CODEX_HOME), which codex refuses to start without and
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
    /// Codex `--exec` headless mode (ignored by Claude).
    pub exec: bool,
    /// Pass-through args after `--`, handed to the agent verbatim.
    pub passthrough: &'a [String],
    /// The profile home dir on the host (mounted at the container home).
    pub home_dir: &'a Path,
}

/// What an agent wants from a run, after translating its relay config. Combines
/// with the shared docker flags in [`assemble_run_args`].
pub struct Invocation {
    /// Extra `docker run` args the agent needs (e.g. Codex's `--env-file` for the
    /// key, or a read-only `auth.json` / instructions mount).
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

/// Build the full `docker run` argument list (everything between `docker run` and
/// the image), folding in the agent's `extra_run_args`: `--rm`, the
/// credential/auth args, `-it`/`-i`, hardening, Linux uid/gid + host-gateway, and
/// the home / `/work` / extra mounts.
pub fn assemble_run_args(
    agent: AgentKind,
    work_dir: &str,
    home_dir: &Path,
    extra_mounts: &[String],
    invocation_extra: &[String],
) -> Vec<String> {
    let mut a: Vec<String> = vec!["--rm".into()];

    // Agent-specific credential/auth args (Codex's --env-file or auth.json mount).
    a.extend(invocation_extra.iter().cloned());

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

    a
}
