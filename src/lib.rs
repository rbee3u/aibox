//! aibox — run coding agents (Claude Code, OpenAI Codex) inside a Docker
//! container that **is** the sandbox boundary.
//!
//! This library holds all the logic; the `aibox` binary (`main.rs`) is a thin
//! shell that parses argv and calls [`run`]. Splitting it this way keeps the
//! merge, `refresh`, session parsing, and arg handling as plain functions with
//! `#[test]`s.
//!
//! The two agents diverge only through [`agent::AgentKind`]; everything else is
//! shared.

pub mod agent;
pub mod cli;
pub mod creds;
pub mod docker;
pub mod envfile;
pub mod platform;
pub mod profile;
pub mod refresh;
pub mod runspec;
pub mod session;
mod session_claude;
mod session_codex;
pub mod template;

use agent::AgentKind;
use anyhow::{Context, Result};
use cli::{Action, BuildArgs, BuildTarget, Cli, Command, RunArgs};
use docker::BuildCache;
use envfile::MergedEnv;
use profile::Profile;
use runspec::RunOpts;

/// Read an optional environment override that must be non-empty when present.
/// Empty values are almost always accidental for path/tag knobs, and treating
/// them as real values can move state into surprising places.
pub(crate) fn env_override(name: &str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) if value.is_empty() => anyhow::bail!("{name} is set but empty"),
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("{name} is not valid UTF-8")
        }
    }
}

/// Resolve the image tag: `$AIBOX_IMAGE` wins, else the agent default.
fn image_for(agent: AgentKind, image_override: Option<&str>) -> String {
    image_override
        .unwrap_or_else(|| agent.image_default())
        .to_string()
}

/// Write one line to stdout. `Ok(true)` on success; `Ok(false)` when the reader
/// hung up (`session list | head` and friends) — the Rust runtime ignores
/// SIGPIPE, so a plain `println!` would panic on the broken pipe instead. The
/// caller should stop writing and exit cleanly. Other write errors are real.
/// Shared by the bulk-stdout paths: `session list`/`get` and `refresh --dry-run`.
pub(crate) fn print_line(line: &str) -> Result<bool> {
    use std::io::Write;
    match writeln!(std::io::stdout().lock(), "{line}") {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(e) => Err(e).context("write to stdout"),
    }
}

/// Top-level dispatch. `passthrough` is the argv tail after `--` (agent args).
///
/// `build` owns image construction. `refresh` / `session` short-circuit a run
/// and never touch Docker. A plain run flows through `run_agent`.
pub fn run(cli: Cli, passthrough: Vec<String>) -> Result<i32> {
    match cli.command {
        Command::Build(args) => {
            // Same rationale as refresh/session: `--` args are for an agent
            // run, and silently dropping them would hide a misuse.
            if !passthrough.is_empty() {
                anyhow::bail!(
                    "`-- <args>` applies only to a run; build takes no pass-through args"
                );
            }
            run_build(&args)
        }
        Command::Claude(args) => run_agent_command(AgentKind::Claude, &args, &passthrough),
        Command::Codex(args) => run_agent_command(AgentKind::Codex, &args, &passthrough),
    }
}

fn run_agent_command(
    agent: AgentKind,
    args: &cli::AgentArgs,
    passthrough: &[String],
) -> Result<i32> {
    if let Some(action) = &args.action {
        // `--` args are for the agent; refresh/session never start one, and
        // silently dropping them would hide a misuse.
        if !passthrough.is_empty() {
            anyhow::bail!(
                "`-- <args>` applies only to a run; refresh/session take no pass-through args"
            );
        }
        reject_run_only_options(&args.run)?;
        let root = profile::config_root(agent)?;
        let prof = Profile::resolve(agent, &root, &args.run.profile)?;
        return match action {
            Action::Refresh { target, dry_run } => {
                refresh::run_refresh(&prof, target.as_deref(), *dry_run)
            }
            Action::Session { action, ids, yes } => {
                session::dispatch(agent, &prof.home_dir, action, ids, *yes)
            }
        };
    }

    run_agent(agent, &args.run, passthrough)
}

/// Management actions use only `--profile`; accepting the other flattened run
/// flags and then silently ignoring them makes a mistyped command appear to do
/// something it did not. Reject every such option before touching profile
/// state.
fn reject_run_only_options(run: &RunArgs) -> Result<()> {
    let mut used = Vec::new();
    if run.env.is_some() {
        used.push("--env");
    }
    if run.work.is_some() {
        used.push("--work");
    }
    if !run.mount.is_empty() {
        used.push("--mount");
    }
    if run.safe {
        used.push("--safe");
    }
    if run.exec {
        used.push("--exec");
    }
    if !used.is_empty() {
        anyhow::bail!(
            "refresh/session do not accept run-only options: {}",
            used.join(", ")
        );
    }
    Ok(())
}

/// Build the shared base image, then one or both embedded agent images. Cached
/// by default. `--force` pulls a fresh Debian image for the base build, then
/// rebuilds the agent image(s) without pulling `aibox-base` from a registry.
fn run_build(args: &BuildArgs) -> Result<i32> {
    let image_override = env_override("AIBOX_IMAGE")?;
    if args.target.is_none() && image_override.is_some() {
        anyhow::bail!(
            "AIBOX_IMAGE is ambiguous with `aibox build`; choose `aibox build claude` or `aibox build codex`"
        );
    }

    let agents = match args.target {
        None => vec![AgentKind::Claude, AgentKind::Codex],
        Some(BuildTarget::Claude) => vec![AgentKind::Claude],
        Some(BuildTarget::Codex) => vec![AgentKind::Codex],
    };

    let base_cache = if args.force {
        BuildCache::NoCachePull
    } else {
        BuildCache::Cached
    };
    if args.force {
        eprintln!(
            ">> building {} (no cache, pulling fresh Debian base) ...",
            docker::BASE_IMAGE
        );
    } else {
        eprintln!(">> building {} (cache enabled) ...", docker::BASE_IMAGE);
    }
    docker::build_image(docker::BASE_DOCKERFILE, docker::BASE_IMAGE, base_cache)
        .context("build base image")?;

    let agent_cache = if args.force {
        BuildCache::NoCache
    } else {
        BuildCache::Cached
    };
    for agent in agents {
        let image = image_for(agent, image_override.as_deref());
        if args.force {
            eprintln!(">> building {image} (no cache) ...");
        } else {
            eprintln!(">> building {image} (cache enabled) ...");
        }
        docker::build_image(agent.dockerfile(), &image, agent_cache)
            .with_context(|| format!("build {}", agent.tag()))?;
    }

    Ok(0)
}

/// A normal (non-refresh, non-session) run: resolve the profile and relay, require
/// a pre-built image, merge config, stage credentials, assemble `docker run`,
/// and run the agent as a child (so credential cleanup fires afterwards).
fn run_agent(agent: AgentKind, run: &RunArgs, passthrough: &[String]) -> Result<i32> {
    let image_override = env_override("AIBOX_IMAGE")?;
    let image = image_for(agent, image_override.as_deref());
    // The override applies to *both* agents, so a leftover export runs claude
    // in the codex image (and vice versa) with only a confusing entrypoint
    // error to show for it. Say which image is in play before anything fails.
    if image_override.is_some() {
        eprintln!(">> image overridden by $AIBOX_IMAGE: {image}");
    }

    // Reject --exec before any work; see `AgentKind::supports_exec`.
    if run.exec && !agent.supports_exec() {
        anyhow::bail!("--exec is codex-only");
    }

    // --- resolve profile paths ------------------------------------------
    let root = profile::config_root(agent)?;
    let prof = Profile::resolve(agent, &root, &run.profile)?;

    // --- a relay is required --------------------------------------------
    // No default endpoint: every run picks one with -e.
    let Some(env_name) = run.env.as_deref() else {
        eprintln!("!! no relay selected — pick one with -e <name>:");
        let names = prof.relay_names();
        if names.is_empty() {
            eprintln!(
                "     (none yet — run  aibox {} -e <name>  to scaffold one)",
                agent.tag()
            );
        } else {
            for n in names {
                eprintln!("     {n}");
            }
        }
        return Ok(1);
    };

    // Home must exist before the run so the mount doesn't shadow the image with a
    // root-owned empty dir. Also seeds agent-specific first-run files.
    prof.ensure_home()?;
    runspec::seed_home(agent, &prof.home_dir)?;

    // First use of a named relay scaffolds a stub and stops so credentials can be
    // filled in (Ok(None)); an explicit missing path errors. Exit 1 like the
    // no-relay case: the agent never ran, and scripts must not read the stop
    // as a successful run.
    let Some(relay) = prof.resolve_relay_for_run(env_name)? else {
        return Ok(1);
    };

    // Runs never build implicitly. Build explicitly so cache policy is obvious.
    if !docker::image_exists(&image)? {
        anyhow::bail!(
            "{image} is not present locally; build it first with `aibox build {}`",
            agent.tag()
        );
    }

    // Nudge (don't touch) if base or the relay predates the current template.
    prof.nudge_if_stale(relay.path());

    // --- merge base + relay ---------------------------------------------
    let sources = prof.merge_sources(relay.path())?;
    let merged = MergedEnv::merge(&sources);

    // --- assemble and run -----------------------------------------------
    // Absolutized + validated: Docker would read a bare relative name as a
    // *named volume* and silently mount an empty one at /work. Extra `-m`
    // mounts get the same treatment for their host side.
    let work_dir = runspec::resolve_work_dir(run.work.as_deref())?;
    let mounts = runspec::resolve_mounts(&run.mount)?;
    // The profile home is bind-mounted at the container home; its path (from
    // $HOME / $AIBOX_CONFIG_ROOT / the profile name) is a bind source too, so it
    // must survive docker's `-v` colon splitting like `/work` and `-m` do.
    runspec::reject_colon_in_bind_source("profile home", &prof.home_dir)?;

    let opts = RunOpts {
        env: &merged,
        safe: run.safe,
        exec: run.exec,
        passthrough,
        home_dir: &prof.home_dir,
    };
    // `build_invocation` owns credential staging and endpoint wiring: Claude
    // stages the merged env as `--env-file`; Codex stages its key, guarded mount
    // targets, and `-c` overrides.
    let invocation = agent.build_invocation(&opts)?;

    let run_args = runspec::assemble_run_args(
        agent,
        &work_dir,
        &prof.home_dir,
        &mounts,
        &invocation.extra_run_args,
    );

    let code = docker::run(&run_args, &image, &invocation.agent_cmd)?;

    // Docker has returned; drop the whole invocation so its staged files and
    // guarded mount targets are unlinked together (their `Drop` impls do the
    // cleanup). Explicit rather than end-of-scope only to mark the ordering:
    // nothing ephemeral outlives the run.
    drop(invocation);
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use std::ffi::OsString;

    // These guards bail before Docker work, so they run without requiring a
    // built image.

    struct EnvGuard {
        name: &'static str,
        old: Option<OsString>,
    }

    impl EnvGuard {
        fn set(name: &'static str, value: &str) -> Self {
            let old = std::env::var_os(name);
            std::env::set_var(name, value);
            EnvGuard { name, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    #[test]
    fn env_override_rejects_empty_values() {
        let _guard = EnvGuard::set("AIBOX_TEST_EMPTY_OVERRIDE", "");

        let err = env_override("AIBOX_TEST_EMPTY_OVERRIDE")
            .unwrap_err()
            .to_string();

        assert!(err.contains("AIBOX_TEST_EMPTY_OVERRIDE is set but empty"));
    }

    #[test]
    fn refresh_session_and_build_reject_passthrough_args() {
        for argv in [
            ["aibox", "claude", "refresh"].as_slice(),
            ["aibox", "codex", "session"].as_slice(),
            ["aibox", "build"].as_slice(),
        ] {
            let cli = Cli::try_parse_from(argv.iter().copied()).unwrap();
            let err = run(cli, vec!["--model".into(), "opus".into()]).unwrap_err();
            assert!(
                err.to_string().contains("no pass-through args"),
                "unexpected error for {argv:?}: {err}"
            );
        }
    }

    #[test]
    fn refresh_and_session_reject_ignored_run_only_options() {
        for argv in [
            ["aibox", "codex", "-e", "relay", "session"].as_slice(),
            ["aibox", "claude", "--safe", "refresh"].as_slice(),
            ["aibox", "codex", "--exec", "session"].as_slice(),
        ] {
            let cli = Cli::try_parse_from(argv.iter().copied()).unwrap();
            let err = run(cli, Vec::new()).unwrap_err().to_string();
            assert!(
                err.contains("run-only options"),
                "unexpected error for {argv:?}: {err}"
            );
        }
    }

    #[test]
    fn claude_exec_is_rejected() {
        let cli = Cli::try_parse_from(["aibox", "claude", "--exec"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err();
        assert!(err.to_string().contains("--exec is codex-only"));
    }

    #[test]
    fn unsafe_profile_name_is_rejected_before_run_or_session_paths() {
        let cli = Cli::try_parse_from(["aibox", "codex", "-p", "..", "session"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err();
        assert!(err
            .to_string()
            .contains("profile name must be a single path segment"));

        let cli = Cli::try_parse_from(["aibox", "claude", "-p", "", "-e", "r"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err();
        assert!(err
            .to_string()
            .contains("profile name must be a single path segment"));
    }
}
