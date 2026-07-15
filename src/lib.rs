//! aibox — run coding agents (Claude Code, OpenAI Codex) inside a Docker
//! container that IS the sandbox boundary.
//!
//! This library holds all the logic; the `aibox` binary (`main.rs`) is a thin
//! shell that parses argv and calls [`run`]. Splitting it this way keeps the
//! merge, `sync`, session parsing, and arg handling as plain functions with
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
pub mod runspec;
pub mod session;
pub mod sync;
pub mod template;

use agent::AgentKind;
use anyhow::{Context, Result};
use cli::{Action, Cli, RunArgs};
use envfile::MergedEnv;
use profile::Profile;
use runspec::RunOpts;

/// Resolve the image tag: `$AIBOX_IMAGE` wins, else the agent default.
fn image_for(agent: AgentKind) -> String {
    std::env::var("AIBOX_IMAGE").unwrap_or_else(|_| agent.image_default().to_string())
}

/// Top-level dispatch. `passthrough` is the argv tail after `--` (agent args).
///
/// `sync` / `session` short-circuit a run and never touch docker. A plain run
/// flows through [`run_agent`].
pub fn run(cli: Cli, passthrough: Vec<String>) -> Result<i32> {
    let agent = cli.agent.kind();
    let args = cli.agent.args();

    if let Some(action) = &args.action {
        let root = profile::config_root(agent)?;
        let prof = Profile::resolve(agent, &root, &args.run.profile);
        return match action {
            Action::Sync { target, dry_run } => sync::run_sync(&prof, target.as_deref(), *dry_run),
            Action::Session { action, id } => {
                let act = action.as_deref().unwrap_or("list");
                session::dispatch(agent, &prof.home_dir, act, id.as_deref())
            }
        };
    }

    run_agent(agent, &args.run, &passthrough)
}

/// A normal (non-sync, non-session) run: build if asked, resolve the profile and
/// relay, merge config, stage credentials, assemble `docker run`, and exec the
/// agent as a child (so credential cleanup fires afterwards).
fn run_agent(agent: AgentKind, run: &RunArgs, passthrough: &[String]) -> Result<i32> {
    let image = image_for(agent);

    // Reject --exec for agents without a headless subcommand (Claude) before any
    // work — notably before --build, so `aibox claude --build --exec` fails fast
    // instead of building an image first. The flag is shared in the CLI struct;
    // whether it's supported is an AgentKind question (see `supports_exec`).
    if run.exec && !agent.supports_exec() {
        anyhow::bail!("--exec is codex-only");
    }

    // --- explicit build --------------------------------------------------
    // `--build` always rebuilds fresh (--no-cache --pull); a missing image is
    // auto-built (cached, fast) just before the run below.
    if run.build {
        eprintln!(">> building {image} (no cache, pulling fresh base) ...");
        docker::build_image(agent, &image, true).context("--build")?;
    }

    // --- resolve profile paths ------------------------------------------
    let root = profile::config_root(agent)?;
    let prof = Profile::resolve(agent, &root, &run.profile);

    // --- a relay is required --------------------------------------------
    // No default endpoint: every run picks one with -e. A bare --build built
    // above, so it just reports and stops.
    let Some(env_name) = run.env.as_deref() else {
        if run.build {
            eprintln!(
                ">> image built. run it with a relay: aibox {} -e <name>",
                agent.tag()
            );
            return Ok(0);
        }
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
    // filled in (Ok(None)); an explicit missing path errors.
    let Some(relay) = prof.resolve_relay_for_run(env_name)? else {
        return Ok(0);
    };

    // Nudge (don't touch) if base or the relay predates the current template.
    prof.nudge_if_stale(relay.path());

    // --- build if the image is missing ----------------------------------
    // Cached on purpose (speed); upgrading is --build's job.
    if !run.build && !docker::image_exists(&image) {
        eprintln!(">> {image} not present — building from cache (fast; --build to refresh) ...");
        docker::build_image(agent, &image, false).context("auto-build")?;
    }

    // --- merge base + relay ---------------------------------------------
    let sources = prof.merge_sources(relay.path())?;
    let merged = MergedEnv::merge(&sources);

    // --- assemble and run -----------------------------------------------
    let work_dir = match &run.work {
        Some(w) => w.clone(),
        None => std::env::current_dir()
            .context("get current dir for /work")?
            .to_string_lossy()
            .into_owned(),
    };

    let opts = RunOpts {
        env: &merged,
        safe: run.safe,
        exec: run.exec,
        passthrough,
        home_dir: &prof.home_dir,
    };
    // `build_invocation` owns all credential staging and endpoint wiring: Claude
    // stages the merged env as `--env-file`, Codex stages its key and `-c`
    // overrides. Everything ephemeral lands in `invocation.staged`.
    let invocation = agent.build_invocation(&opts)?;

    let run_args = runspec::assemble_run_args(
        agent,
        &work_dir,
        &prof.home_dir,
        &run.mount,
        &invocation.extra_run_args,
    );

    let code = docker::run(&run_args, &image, &invocation.agent_cmd)?;

    // docker has returned; drop the whole invocation so its staged files and
    // guarded mount targets are unlinked together (their `Drop` impls do the
    // cleanup). Explicit rather than end-of-scope only to mark the ordering:
    // nothing ephemeral outlives the run.
    drop(invocation);
    Ok(code)
}
