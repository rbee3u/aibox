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
#[cfg(test)]
mod testutil;

#[cfg(test)]
pub(crate) fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

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
fn image_for(agent: AgentKind, image_override: Option<&str>) -> Result<String> {
    let image = image_override.unwrap_or_else(|| agent.image_default());
    validate_image_ref(agent, image)?;
    Ok(image.to_string())
}

fn validate_image_ref(agent: AgentKind, image: &str) -> Result<()> {
    if image.is_empty() {
        anyhow::bail!("Docker image reference is empty");
    }
    if image.starts_with('-') {
        anyhow::bail!("Docker image reference must not start with '-': {image:?}");
    }
    if image
        .chars()
        .any(|c| c.is_ascii_control() || c.is_ascii_whitespace())
    {
        anyhow::bail!(
            "Docker image reference must not contain whitespace/control characters: {image:?}"
        );
    }
    if image_ref_is_default(image, docker::BASE_IMAGE) {
        anyhow::bail!("Docker image reference must not use aibox's internal base image: {image:?}");
    }
    let other_agent = match agent {
        AgentKind::Claude => AgentKind::Codex,
        AgentKind::Codex => AgentKind::Claude,
    };
    if image_ref_is_default(image, other_agent.image_default()) {
        anyhow::bail!(
            "Docker image reference {image:?} is the default {} image, not {}",
            other_agent.tag(),
            agent.tag()
        );
    }
    Ok(())
}

/// Docker normalizes familiar Docker Hub names (`busybox`, `library/busybox`,
/// and `docker.io/library/busybox`) to the same repository, supplies `latest`
/// when no tag is present, and permits a repository to be selected by digest.
/// Keep the safety checks in `validate_image_ref` aligned with those rules so
/// an equivalent spelling cannot bypass the agent/base-image guard.
fn image_ref_is_default(image: &str, default: &str) -> bool {
    let Some((image_repo, image_tag, image_has_digest)) = image_ref_parts(image) else {
        return false;
    };
    let Some((default_repo, default_tag, _)) = image_ref_parts(default) else {
        return image == default;
    };

    image_repo == default_repo
        && (image_has_digest || image_tag.unwrap_or("latest") == default_tag.unwrap_or("latest"))
}

fn image_ref_parts(image: &str) -> Option<(String, Option<&str>, bool)> {
    let (name_and_tag, has_digest) = match image.split_once('@') {
        Some((name, _)) => (name, true),
        None => (image, false),
    };
    if name_and_tag.is_empty() {
        return None;
    }

    // A colon denotes a tag only when it occurs after the final slash; an
    // earlier colon belongs to a registry port (`registry:5000/repo`).
    let last_slash = name_and_tag.rfind('/');
    let (repository, tag) = match name_and_tag.rfind(':') {
        Some(colon) if last_slash.is_none_or(|slash| colon > slash) => {
            (&name_and_tag[..colon], Some(&name_and_tag[colon + 1..]))
        }
        _ => (name_and_tag, None),
    };
    if repository.is_empty() {
        return None;
    }

    Some((normalize_docker_repository(repository), tag, has_digest))
}

fn normalize_docker_repository(repository: &str) -> String {
    let (domain, remainder) = match repository.split_once('/') {
        None => return format!("docker.io/library/{repository}"),
        Some(("docker.io" | "index.docker.io", remainder)) => ("docker.io", remainder),
        Some((first, _)) if first == "localhost" || first.contains('.') || first.contains(':') => {
            return repository.to_string();
        }
        Some(_) => ("docker.io", repository),
    };

    if remainder.contains('/') {
        format!("{domain}/{remainder}")
    } else {
        format!("{domain}/library/{remainder}")
    }
}

/// Write one line to stdout. `Ok(true)` on success; `Ok(false)` when the reader
/// hung up (`session list | head` and friends) — the Rust runtime ignores
/// SIGPIPE, so a plain `println!` would panic on the broken pipe instead. The
/// caller should stop writing and exit cleanly. Other write errors are real.
/// Shared by the bulk-stdout paths: `session list`/`get` and `refresh --dry-run`.
pub(crate) fn print_line(line: &str) -> Result<bool> {
    write_line(&mut std::io::stdout().lock(), line)
}

/// The classification behind [`print_line`], over any writer so the
/// broken-pipe-vs-real-error decision is testable without replacing fd 1.
fn write_line(out: &mut impl std::io::Write, line: &str) -> Result<bool> {
    match writeln!(out, "{line}") {
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
        prof.validate_existing_layout_boundary()?;
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
    let targets = build_targets(args, image_override.as_deref())?;

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
    for (agent, image) in targets {
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

fn build_targets(
    args: &BuildArgs,
    image_override: Option<&str>,
) -> Result<Vec<(AgentKind, String)>> {
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

    agents
        .into_iter()
        .map(|agent| {
            let image = image_for(agent, image_override)?;
            Ok((agent, image))
        })
        .collect()
}

/// A normal (non-refresh, non-session) run: resolve the profile and relay, require
/// a pre-built image, merge config, stage credentials, assemble `docker run`,
/// and run the agent as a child (so credential cleanup fires afterwards).
fn run_agent(agent: AgentKind, run: &RunArgs, passthrough: &[String]) -> Result<i32> {
    let image_override = env_override("AIBOX_IMAGE")?;
    let image = image_for(agent, image_override.as_deref())?;
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
    // Validate managed directories before resolving a named relay. Relay
    // resolution may scaffold `base` and `envs/<name>`, so deferring this check
    // would leave partial state when the mounted home or relay directory is a
    // symlink and `ensure_home` rejects it later.
    prof.validate_existing_layout_boundary()?;

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

    // Validate every Docker bind source/target before creating profile state.
    // Otherwise a bad `-w`, `-m`, or colon-containing profile root can leave
    // half-scaffolded homes/config files before Docker would reject the run.
    let work_dir = runspec::resolve_work_dir(run.work.as_deref())?;
    let mounts = runspec::resolve_mounts(&run.mount)?;
    runspec::validate_extra_mount_targets(agent, &mounts)?;
    // The profile home is bind-mounted at the container home; its path (from
    // $HOME / $AIBOX_CONFIG_ROOT / the profile name) is a bind source too, so it
    // must survive docker's `-v` colon splitting like `/work` and `-m` do.
    runspec::reject_colon_in_bind_source("profile home", &prof.home_dir)?;

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

    // --- merge base + relay ---------------------------------------------
    // Read and validate config before creating the mounted home. A malformed
    // explicit relay or base file is not usable and must not leave profile-home
    // state behind merely because the image happened to exist.
    let sources = prof.merge_sources(relay.path())?;
    let merged = MergedEnv::merge(&sources);

    // Home is needed only once the relay, image, and env-file syntax are usable.
    prof.ensure_home()?;
    runspec::seed_home(agent, &prof.home_dir)?;

    // Nudge (don't touch) if base or the relay predates the current template.
    prof.nudge_if_stale(relay.path());

    // --- assemble and run -----------------------------------------------
    let opts = RunOpts {
        env: &merged,
        safe: run.safe,
        exec: run.exec,
        passthrough,
        home_dir: &prof.home_dir,
        profile_dir: &prof.dir,
    };
    // `build_invocation` owns credential staging and endpoint wiring: Claude
    // stages the merged env as `--env-file`; Codex stages its key, guarded mount
    // targets, and `-c` overrides.
    let mut invocation = agent.build_invocation(&opts)?;

    let run_args = runspec::assemble_run_args(
        agent,
        &work_dir,
        &prof.home_dir,
        &mounts,
        &invocation.extra_run_args,
    );

    let agent_cmd = invocation.agent_cmd.clone();
    let code = docker::run(&run_args, &image, &agent_cmd, || {
        invocation.release_spawn_locks();
    })?;

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
    use crate::testutil::EnvGuard;
    use clap::Parser;

    // These guards bail before Docker work, so they run without requiring a
    // built image.

    #[cfg(unix)]
    fn write_missing_image_docker(dir: &std::path::Path) {
        crate::testutil::write_stub_script(
            dir,
            "docker",
            r#"#!/bin/sh
if [ "$1" = "image" ] && [ "$2" = "inspect" ]; then
    printf 'Error response from daemon: No such image: %s\n' "${5:-}" >&2
    exit 1
fi
if [ "$1" = "image" ] && [ "$2" = "ls" ]; then
    exit 0
fi
exit 99
"#,
        );
    }

    #[cfg(unix)]
    fn write_existing_image_docker(dir: &std::path::Path) {
        crate::testutil::write_stub_script(
            dir,
            "docker",
            r#"#!/bin/sh
if [ "$1" = "image" ] && [ "$2" = "inspect" ]; then
    printf 'sha256:fake-image\n'
    exit 0
fi
exit 99
"#,
        );
    }

    #[cfg(unix)]
    fn write_docker_that_disappears_after_image_check(dir: &std::path::Path) {
        crate::testutil::write_stub_script(
            dir,
            "docker",
            r#"#!/bin/sh
if [ "$1" = "image" ] && [ "$2" = "inspect" ]; then
    /bin/rm -f "$AIBOX_FAKE_DOCKER_PATH_TO_DELETE"
    printf 'sha256:fake-image\n'
    exit 0
fi
exit 99
"#,
        );
    }

    #[cfg(unix)]
    fn write_successful_run_docker(dir: &std::path::Path) {
        crate::testutil::write_stub_script(
            dir,
            "docker",
            r#"#!/bin/sh
log="$AIBOX_FAKE_DOCKER_LOG"
printf 'ARGS:' >> "$log"
for arg in "$@"; do
    printf ' <%s>' "$arg" >> "$log"
done
printf '\n' >> "$log"

if [ "$1" = "image" ] && [ "$2" = "inspect" ]; then
    printf 'sha256:fake-image\n'
    exit 0
fi

if [ "$1" = "inspect" ]; then
    printf 'false\n'
    exit 0
fi

if [ "$1" = "run" ]; then
    shift
    cid=
    envfile=
    authjson=
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --cidfile)
                cid="$2"
                shift 2
                ;;
            --env-file)
                envfile="$2"
                shift 2
                ;;
            -v)
                case "$2" in
                    *:/home/codex/.codex/auth.json:ro)
                        authjson="${2%:/home/codex/.codex/auth.json:ro}"
                        ;;
                esac
                shift 2
                ;;
            *)
                shift
                ;;
        esac
    done
    if [ -n "$envfile" ] && [ -f "$envfile" ]; then
        sed 's/^/ENV:/' "$envfile" >> "$log"
    fi
    if [ -n "$authjson" ] && [ -f "$authjson" ]; then
        sed 's/^/AUTH:/' "$authjson" >> "$log"
    fi
    printf 'fake-container\n' > "$cid"
    exit "${AIBOX_FAKE_DOCKER_RUN_EXIT:-0}"
fi

exit 99
"#,
        );
    }

    #[cfg(unix)]
    fn write_build_logging_docker(dir: &std::path::Path) {
        crate::testutil::write_stub_script(
            dir,
            "docker",
            r#"#!/bin/sh
if [ "$1" != "build" ]; then
    exit 99
fi
log="$AIBOX_FAKE_DOCKER_LOG"
printf 'ARGS:' >> "$log"
for arg in "$@"; do
    printf ' <%s>' "$arg" >> "$log"
    last="$arg"
done
printf '\n' >> "$log"
if [ ! -d "$last" ]; then
    printf 'context is not a directory: %s\n' "$last" >&2
    exit 98
fi
printf 'STDIN:' >> "$log"
sed -n '1p' >> "$log"
printf '\nEND\n' >> "$log"
"#,
        );
    }

    #[cfg(unix)]
    fn write_docker_should_not_be_called(dir: &std::path::Path) {
        crate::testutil::write_stub_script(
            dir,
            "docker",
            r#"#!/bin/sh
if [ -n "$AIBOX_FAKE_DOCKER_LOG" ]; then
    printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
fi
exit 97
"#,
        );
    }

    /// The two minimal relay bodies the run tests stage. Named because the exact
    /// key set is what makes a run *reach Docker* (Codex rejects a relay missing
    /// any required key), so a test staging its own near-copy tends to drift
    /// into testing a different code path than it claims.
    const CLAUDE_RELAY_BODY: &str =
        "ANTHROPIC_BASE_URL=https://relay.example\nANTHROPIC_AUTH_TOKEN=sk-claude\n";
    const CODEX_RELAY_BODY: &str =
        "CODEX_BASE_URL=https://relay.example/v1\nCODEX_API_KEY=sk-test\nCODEX_MODEL=gpt-test\n";
    /// The same Codex relay in auth.json mode — the second of the two mutually
    /// exclusive auth wirings, so several tests need it verbatim.
    const CODEX_AUTH_JSON_RELAY_BODY: &str = "CODEX_BASE_URL=https://relay.example/v1\nCODEX_API_KEY=sk-test\nCODEX_MODEL=gpt-test\nCODEX_REQUIRES_OPENAI_AUTH=1\n";

    /// One end-to-end run fixture: a profile tree under a temp config root and a
    /// stubbed `docker` on `$PATH` that logs the argv it was handed. Every
    /// `run()`-level test needs the same eight-step setup (tempdir, config root,
    /// relay file, stub script, and the four env guards); assembling it once
    /// keeps a test from silently diverging in how it stages the profile, and
    /// keeps the guards alive for exactly the fixture's lifetime.
    #[cfg(unix)]
    struct RunFixture {
        root: tempfile::TempDir,
        docker_dir: tempfile::TempDir,
        config_root: std::path::PathBuf,
        docker_log: std::path::PathBuf,
        _env_lock: std::sync::MutexGuard<'static, ()>,
        _run_lock: std::sync::MutexGuard<'static, ()>,
        guards: Vec<EnvGuard>,
    }

    #[cfg(unix)]
    impl RunFixture {
        /// Stage a profile whose `envs/relay` holds `relay_body`, with
        /// `write_docker` providing the `docker` stub.
        fn new(relay_body: &str, write_docker: fn(&std::path::Path)) -> Self {
            let fx = Self::bare(write_docker);
            let relay = fx.profile().join("envs").join("relay");
            std::fs::create_dir_all(relay.parent().unwrap()).unwrap();
            std::fs::write(&relay, relay_body).unwrap();
            fx
        }

        /// The same fixture with no profile state at all — for the first-use and
        /// "must fail before creating anything" paths, which assert on what does
        /// *not* get created.
        fn bare(write_docker: fn(&std::path::Path)) -> Self {
            let env_lock = test_env_lock();
            let run_lock = crate::creds::run_registry_test_lock();
            let root = tempfile::tempdir().unwrap();
            let config_root = root.path().join("aibox-config");

            let docker_dir = tempfile::tempdir().unwrap();
            let docker_log = docker_dir.path().join("docker.log");
            write_docker(docker_dir.path());
            let guards = vec![
                EnvGuard::prepend_path(docker_dir.path()),
                EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", docker_log.as_os_str()),
                EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.as_os_str()),
            ];

            RunFixture {
                root,
                docker_dir,
                config_root,
                docker_log,
                _env_lock: env_lock,
                _run_lock: run_lock,
                guards,
            }
        }

        /// The same fixture with the successful-run `docker` stub, which is what
        /// every "a real run reaches Docker" test wants.
        fn successful(relay_body: &str) -> Self {
            Self::new(relay_body, write_successful_run_docker)
        }

        fn env(&mut self, name: &'static str, value: &str) -> &mut Self {
            self.guards.push(EnvGuard::set(name, value));
            self
        }

        /// Set an env var from an `OsStr` (a path, typically), for the guards
        /// whose value is not a plain `&str`.
        fn env_os(&mut self, name: &'static str, value: &std::ffi::OsStr) -> &mut Self {
            self.guards.push(EnvGuard::set(name, value));
            self
        }

        /// The directory holding the stubbed `docker`, for the one test that
        /// makes the stub delete itself mid-run.
        /// Owned rather than borrowed: callers pass this straight into
        /// `env_os`, which takes `&mut self`, so a borrow of the fixture could
        /// not still be alive at the call.
        fn docker_dir(&self) -> std::path::PathBuf {
            self.docker_dir.path().to_path_buf()
        }

        fn profile(&self) -> std::path::PathBuf {
            self.config_root.join("default")
        }

        /// A path beside the config root, for fixtures that need a file outside
        /// the profile tree (an explicit `-e <path>` relay, for instance).
        fn scratch(&self, name: &str) -> std::path::PathBuf {
            self.root.path().join(name)
        }

        fn base(&self, contents: &str) -> &Self {
            std::fs::write(self.profile().join("base"), contents).unwrap();
            self
        }

        fn run(&self, argv: &[&str], passthrough: Vec<String>) -> Result<i32> {
            let cli = Cli::try_parse_from(argv.iter().copied()).unwrap();
            run(cli, passthrough)
        }

        fn log(&self) -> String {
            std::fs::read_to_string(&self.docker_log).unwrap_or_default()
        }

        /// The logged `docker run` argv line (the fixture's stub records one per
        /// docker invocation, prefixed by subcommand).
        fn run_line(&self) -> String {
            self.log()
                .lines()
                .find(|line| line.starts_with("ARGS: <run>"))
                .expect("docker run was invoked")
                .to_string()
        }
    }

    fn token_after_arg(line: &str, arg: &str) -> Option<String> {
        let marker = format!("<{arg}> <");
        let start = line.find(&marker)? + marker.len();
        let end = line[start..].find('>')?;
        Some(line[start..start + end].to_string())
    }

    fn mounted_source_for(line: &str, target: &str) -> Option<String> {
        let suffix = format!(":{target}:ro");
        line.split(" <")
            .map(|part| part.strip_suffix('>').unwrap_or(part))
            .find_map(|arg| arg.strip_suffix(&suffix).map(str::to_string))
    }

    #[test]
    fn env_override_rejects_empty_values() {
        let _env_lock = test_env_lock();
        let _guard = EnvGuard::set("AIBOX_TEST_EMPTY_OVERRIDE", "");

        let err = env_override("AIBOX_TEST_EMPTY_OVERRIDE")
            .unwrap_err()
            .to_string();

        assert!(err.contains("AIBOX_TEST_EMPTY_OVERRIDE is set but empty"));
    }

    #[cfg(unix)]
    #[test]
    fn env_override_rejects_non_utf8_values() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let _env_lock = test_env_lock();
        let _guard = EnvGuard::set(
            "AIBOX_TEST_NON_UTF8_OVERRIDE",
            OsString::from_vec(b"invalid-\xff".to_vec()),
        );

        let err = env_override("AIBOX_TEST_NON_UTF8_OVERRIDE")
            .unwrap_err()
            .to_string();

        assert!(err.contains("AIBOX_TEST_NON_UTF8_OVERRIDE is not valid UTF-8"));
    }

    /// A writer that fails every write with a chosen error kind, so the
    /// broken-pipe classification can be exercised without closing fd 1 (which
    /// would take the test harness's own stdout with it).
    struct FailingWriter(std::io::ErrorKind);

    impl std::io::Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(self.0, "write refused"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// `session list | head` and `refresh --dry-run | head` close the pipe while
    /// we are still writing. Rust ignores SIGPIPE, so an unhandled broken pipe
    /// would panic mid-listing; the bulk-stdout callers rely on `Ok(false)`
    /// meaning "reader hung up, stop cleanly" and on any other error still
    /// surfacing as a failure.
    #[test]
    fn line_writer_reports_a_hung_up_reader_separately_from_a_real_failure() {
        let mut sink: Vec<u8> = Vec::new();
        assert!(write_line(&mut sink, "row").unwrap());
        assert_eq!(sink, b"row\n");

        assert!(
            !write_line(&mut FailingWriter(std::io::ErrorKind::BrokenPipe), "row").unwrap(),
            "a broken pipe must stop the listing, not panic or error"
        );

        let err = write_line(
            &mut FailingWriter(std::io::ErrorKind::PermissionDenied),
            "row",
        )
        .unwrap_err()
        .to_string();
        assert!(
            err.contains("write to stdout"),
            "a real write failure must not be silently swallowed as a hung-up reader: {err}"
        );
    }

    #[test]
    fn image_refs_reject_docker_option_shaped_overrides() {
        // An empty ref is included here rather than in its own test: `AIBOX_IMAGE=`
        // is normally caught by env_override, but an empty ref reaching validation
        // would make the *next* docker argv token look like the image.
        for bad in [
            "",
            "--privileged",
            "-bad",
            "bad tag",
            "bad\nname",
            "bad\tname",
        ] {
            let err = image_for(AgentKind::Codex, Some(bad))
                .unwrap_err()
                .to_string();
            assert!(err.contains("Docker image reference"), "{bad:?}: {err}");
        }
    }

    #[test]
    fn image_refs_reject_internal_or_wrong_agent_images() {
        for base in [
            docker::BASE_IMAGE,
            "aibox-base",
            "library/aibox-base",
            "docker.io/aibox-base:latest",
            "docker.io/library/aibox-base",
            "index.docker.io/library/aibox-base:latest",
            "aibox-base@sha256:deadbeef",
            "aibox-base:dev@sha256:deadbeef",
        ] {
            let err = image_for(AgentKind::Codex, Some(base))
                .unwrap_err()
                .to_string();
            assert!(
                err.contains("internal base image"),
                "base image should not be runnable: {err}"
            );
        }

        for (agent, other, label) in [
            (AgentKind::Codex, AgentKind::Claude, "claude"),
            (AgentKind::Claude, AgentKind::Codex, "codex"),
        ] {
            let default = other.image_default();
            let tagless = default.strip_suffix(":latest").unwrap();
            let canonical = format!("docker.io/library/{tagless}");
            let digest = format!("{tagless}@sha256:deadbeef");
            for image in [default, tagless, &canonical, &digest] {
                let err = image_for(agent, Some(image)).unwrap_err().to_string();
                assert!(
                    err.contains(&format!("default {label} image")),
                    "cross-agent default image should be rejected: {err}"
                );
            }
        }
    }

    #[test]
    fn image_refs_accept_defaults_and_normal_overrides() {
        assert_eq!(
            image_for(AgentKind::Claude, None).unwrap(),
            "aibox-claude:latest"
        );
        assert_eq!(
            image_for(AgentKind::Codex, Some("registry.example/aibox:dev")).unwrap(),
            "registry.example/aibox:dev"
        );
        for distinct in [
            "registry.example/aibox-base:latest",
            "user/aibox-base:latest",
            "localhost/aibox-base:latest",
            "aibox-base:dev",
        ] {
            assert_eq!(
                image_for(AgentKind::Codex, Some(distinct)).unwrap(),
                distinct
            );
        }
    }

    #[test]
    fn build_targets_validate_image_overrides_before_building_base() {
        let args = BuildArgs {
            target: Some(BuildTarget::Codex),
            force: false,
        };

        let err = build_targets(&args, Some("bad tag"))
            .unwrap_err()
            .to_string();

        assert!(err.contains("Docker image reference"), "{err}");
    }

    #[test]
    fn build_all_rejects_ambiguous_image_override() {
        let args = BuildArgs {
            target: None,
            force: false,
        };

        let err = build_targets(&args, Some("custom/agent:dev"))
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("AIBOX_IMAGE is ambiguous with `aibox build`"),
            "{err}"
        );
    }

    #[test]
    fn build_target_accepts_image_override_for_selected_agent() {
        let args = BuildArgs {
            target: Some(BuildTarget::Claude),
            force: false,
        };

        let targets = build_targets(&args, Some("custom/claude:dev")).unwrap();

        assert_eq!(
            targets,
            vec![(AgentKind::Claude, "custom/claude:dev".to_string())]
        );
    }

    #[cfg(unix)]
    #[test]
    fn build_codex_force_builds_base_then_only_codex_image() {
        let _env_lock = test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("docker.log");
        write_build_logging_docker(dir.path());
        let _path = EnvGuard::prepend_path(dir.path());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "build", "codex", "--force"]).unwrap();
        let code = run(cli, Vec::new()).unwrap();

        assert_eq!(code, 0);
        let log = std::fs::read_to_string(log).unwrap();
        let build_lines: Vec<&str> = log
            .lines()
            .filter(|line| line.starts_with("ARGS:"))
            .collect();
        assert_eq!(build_lines.len(), 2, "force target build log:\n{log}");
        assert!(
            build_lines[0].contains("<--no-cache>")
                && build_lines[0].contains("<--pull>")
                && build_lines[0].contains("<-t> <aibox-base:latest>"),
            "base image must be rebuilt first with a fresh upstream base:\n{log}"
        );
        assert!(
            build_lines[1].contains("<--no-cache>")
                && !build_lines[1].contains("<--pull>")
                && build_lines[1].contains("<-t> <aibox-codex:latest>"),
            "target agent image should rebuild without pulling the local base:\n{log}"
        );
        assert!(
            !log.contains("aibox-claude:latest"),
            "a targeted codex build must not build the Claude image:\n{log}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn build_default_builds_base_then_both_agent_images_with_cache() {
        let _env_lock = test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("docker.log");
        write_build_logging_docker(dir.path());
        let _path = EnvGuard::prepend_path(dir.path());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "build"]).unwrap();
        let code = run(cli, Vec::new()).unwrap();

        assert_eq!(code, 0);
        let log = std::fs::read_to_string(log).unwrap();
        let build_lines: Vec<&str> = log
            .lines()
            .filter(|line| line.starts_with("ARGS:"))
            .collect();
        assert_eq!(build_lines.len(), 3, "default build log:\n{log}");
        assert!(
            build_lines[0].contains("<-t> <aibox-base:latest>")
                && !build_lines[0].contains("<--no-cache>")
                && !build_lines[0].contains("<--pull>"),
            "default build must build the shared base first with cache enabled:\n{log}"
        );
        assert!(
            build_lines[1].contains("<-t> <aibox-claude:latest>")
                && build_lines[2].contains("<-t> <aibox-codex:latest>"),
            "default build must build both agent images after the base:\n{log}"
        );
        assert!(
            !log.contains("<--no-cache>") && !log.contains("<--pull>"),
            "cached default build should not force cache or pull flags:\n{log}"
        );
    }

    #[test]
    fn refresh_session_and_build_reject_passthrough_args() {
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

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
        assert!(
            !config_root.exists(),
            "rejecting pass-through misuse must happen before profile state is created"
        );
    }

    #[test]
    fn refresh_and_session_reject_ignored_run_only_options() {
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        // Every run-only option, so a newly flattened flag can't start being
        // silently ignored by refresh/session.
        for (argv, flag) in [
            (
                ["aibox", "codex", "-e", "relay", "session"].as_slice(),
                "--env",
            ),
            (
                ["aibox", "claude", "-w", "src", "refresh"].as_slice(),
                "--work",
            ),
            (
                ["aibox", "codex", "-m", "src:/src", "session"].as_slice(),
                "--mount",
            ),
            (
                ["aibox", "claude", "--safe", "refresh"].as_slice(),
                "--safe",
            ),
            (["aibox", "codex", "--exec", "session"].as_slice(), "--exec"),
        ] {
            let cli = Cli::try_parse_from(argv.iter().copied()).unwrap();
            let err = run(cli, Vec::new()).unwrap_err().to_string();
            assert!(
                err.contains("run-only options"),
                "unexpected error for {argv:?}: {err}"
            );
            assert!(
                err.contains(flag),
                "error should name the rejected {flag} option: {err}"
            );
        }
        assert!(
            !config_root.exists(),
            "rejecting ignored run-only options must happen before profile state is created"
        );
    }

    /// `reject_run_only_options` lists the run-only flags by hand, so a newly
    /// flattened `RunArgs` field would be accepted and then silently ignored by
    /// refresh/session — the exact failure that rejection exists to prevent. The
    /// test above exercises today's five flags; this pins the field set itself, so
    /// adding a sixth fails here until it is either rejected or deliberately
    /// listed as shared with the management actions.
    #[test]
    fn run_only_option_rejection_covers_every_run_arg() {
        // `--profile` is the one run flag refresh/session legitimately accept.
        const SHARED_WITH_MANAGEMENT_ACTIONS: &[&str] = &["profile"];
        const REJECTED: &[&str] = &["env", "work", "mount", "safe", "exec"];

        let cli = Cli::try_parse_from(["aibox", "claude"]).unwrap();
        let debug = format!("{:?}", cli.command.agent_args().unwrap().run);
        let fields: Vec<&str> = debug
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .filter(|token| {
                debug.contains(&format!("{token}: ")) && !token.is_empty() && token != &"RunArgs"
            })
            .collect();

        for field in &fields {
            assert!(
                SHARED_WITH_MANAGEMENT_ACTIONS.contains(field) || REJECTED.contains(field),
                "RunArgs field {field:?} is neither rejected by reject_run_only_options nor \
                 listed as shared with refresh/session; refresh/session would ignore it silently"
            );
        }
        for expected in REJECTED.iter().chain(SHARED_WITH_MANAGEMENT_ACTIONS) {
            assert!(
                fields.contains(expected),
                "{expected:?} is no longer a RunArgs field; update this test and \
                 reject_run_only_options together"
            );
        }
    }

    /// `--safe` is the one flag that changes the sandbox posture, and it is
    /// spelled differently per agent (Claude drops a flag; Codex swaps a bypass
    /// for approvals + a workspace-write sandbox). The per-agent spellings are
    /// unit-tested in `agent`; this pins that a real run reaches Docker with the
    /// *bypass* gone, for both agents — a regression that left the bypass in
    /// place under `--safe` would silently run unrestricted.
    #[cfg(unix)]
    #[test]
    fn safe_runs_reach_docker_without_the_permission_bypass() {
        for (agent, relay_body, bypass) in [
            (
                "claude",
                CLAUDE_RELAY_BODY,
                "--dangerously-skip-permissions",
            ),
            (
                "codex",
                CODEX_RELAY_BODY,
                "--dangerously-bypass-approvals-and-sandbox",
            ),
        ] {
            let fx = RunFixture::successful(relay_body);

            let code = fx
                .run(&["aibox", agent, "-e", "relay", "--safe"], Vec::new())
                .unwrap();

            assert_eq!(code, 0, "{agent}");
            let run_line = fx.run_line();
            assert!(
                !run_line.contains(bypass),
                "{agent}: --safe must not reach docker with {bypass}: {run_line}"
            );
        }
    }

    #[test]
    fn claude_exec_is_rejected() {
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "claude", "--exec"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err();

        assert!(err.to_string().contains("--exec is codex-only"));
        assert!(
            !config_root.exists(),
            "Claude --exec must fail before profile state is created"
        );
    }

    #[test]
    fn invalid_image_override_is_rejected_before_profile_state() {
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());
        let _image = EnvGuard::set("AIBOX_IMAGE", "bad image");

        let cli = Cli::try_parse_from(["aibox", "codex", "-e", "relay"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(err.contains("Docker image reference"), "{err}");
        assert!(
            !config_root.exists(),
            "invalid image overrides must fail before profile state is created"
        );
    }

    #[test]
    fn missing_relay_selection_returns_nonzero_without_profile_state() {
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "claude"]).unwrap();
        let code = run(cli, Vec::new()).unwrap();

        assert_eq!(code, 1);
        assert!(
            !config_root.join("default").exists(),
            "omitting -e should only print a relay hint, not create profile state"
        );
    }

    #[test]
    fn missing_relay_selection_lists_existing_relays_without_running() {
        // The other branch of the same hint: with relays already scaffolded, the
        // run still stops at exit 1, but the user gets the names to choose from
        // instead of the first-use scaffold suggestion.
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let envs = config_root.join("default").join("envs");
        std::fs::create_dir_all(&envs).unwrap();
        std::fs::write(envs.join("alpha"), "ANTHROPIC_BASE_URL=https://a\n").unwrap();
        std::fs::write(envs.join("zeta"), "ANTHROPIC_BASE_URL=https://z\n").unwrap();
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "claude"]).unwrap();
        let code = run(cli, Vec::new()).unwrap();

        assert_eq!(code, 1, "a run without -e never starts the agent");
        assert!(
            !config_root.join("default").join("home").exists(),
            "listing relays must not create the mounted home"
        );
    }

    #[test]
    fn image_ref_parts_rejects_refs_with_no_repository() {
        // Malformed refs must not parse into a comparable repository: if they
        // did, `image_ref_is_default` could equate one with a managed image.
        for malformed in ["", "@sha256:abc", ":latest", "@", ":"] {
            assert!(
                image_ref_parts(malformed).is_none(),
                "{malformed:?} should not parse as a repository"
            );
        }

        // A registry port is not a tag: the colon before the final slash stays
        // part of the host, so the repository keeps its registry prefix.
        let (repo, tag, digest) = image_ref_parts("registry.example:5000/team/img").unwrap();
        assert_eq!(repo, "registry.example:5000/team/img");
        assert_eq!(tag, None);
        assert!(!digest);
    }

    #[test]
    fn image_ref_is_default_rejects_unparseable_refs() {
        // An unparseable *image* is never treated as a managed default: the
        // emptiness check in validate_image_ref is what rejects it, and this
        // guard must not claim a match it cannot justify.
        assert!(!image_ref_is_default("", "aibox-base:latest"));
        assert!(!image_ref_is_default(":latest", "aibox-base:latest"));

        // An unparseable *default* falls back to an exact string comparison.
        // Nothing that parses can equal something that doesn't, so this is
        // always a non-match — the guard fails closed rather than matching
        // loosely on a default it could not interpret.
        assert!(!image_ref_is_default("aibox-base:latest", ""));
        assert!(!image_ref_is_default("aibox-base:latest", ":latest"));
    }

    #[cfg(unix)]
    #[test]
    fn first_use_named_relay_scaffolds_without_checking_docker_image() {
        let fx = RunFixture::bare(write_docker_should_not_be_called);

        let code = fx
            .run(&["aibox", "codex", "-e", "relay"], Vec::new())
            .unwrap();

        assert_eq!(
            code, 1,
            "first use scaffolds config and asks the user to edit it"
        );
        assert!(fx.profile().join("base").is_file());
        assert!(fx.profile().join("envs/relay").is_file());
        assert!(
            !fx.profile().join("home").exists(),
            "scaffold-only run must stop before mounted home setup"
        );
        assert!(
            !fx.docker_log.exists(),
            "scaffold-only run must not query or run Docker before credentials exist"
        );
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

    #[test]
    fn bind_validation_runs_before_profile_side_effects() {
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli =
            Cli::try_parse_from(["aibox", "claude", "-e", "r", "-w", "/no/such/workdir"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(err.contains("work dir is not a directory"), "{err}");
        assert!(
            !config_root.join("default").exists(),
            "invalid work dir must not create profile state"
        );
    }

    #[test]
    fn profile_home_bind_source_is_validated_before_scaffold() {
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("bad:root");
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "codex", "-e", "relay"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(err.contains("profile home path contains ':'"), "{err}");
        assert!(
            !config_root.exists(),
            "invalid profile home bind source must not create profile state"
        );
    }

    #[test]
    fn invalid_relay_name_is_rejected_before_profile_home_creation() {
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "codex", "-e", ""]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(
            err.contains("relay name must be a single path segment"),
            "{err}"
        );
        assert!(
            !config_root.join("default").exists(),
            "invalid relay name must not create profile state"
        );
    }

    #[test]
    fn missing_explicit_env_path_is_rejected_before_home_creation() {
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let missing_env = root.path().join("missing.env");
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli =
            Cli::try_parse_from(["aibox", "claude", "-e", missing_env.to_str().unwrap()]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(err.contains("env file not found"), "{err}");
        assert!(
            !config_root.join("default").join("home").exists(),
            "missing explicit env file must not create profile home"
        );
    }

    #[cfg(unix)]
    #[test]
    fn missing_image_is_rejected_before_home_creation() {
        let fx = RunFixture::new(CODEX_RELAY_BODY, write_missing_image_docker);

        let err = fx
            .run(&["aibox", "codex", "-e", "relay"], Vec::new())
            .unwrap_err()
            .to_string();

        assert!(err.contains("build it first"), "{err}");
        assert!(
            !fx.profile().join("home").exists(),
            "missing image must not create profile home"
        );
    }

    #[cfg(unix)]
    #[test]
    fn malformed_env_is_rejected_before_home_creation() {
        let fx = RunFixture::new("CODEX_API_KEY = sk-invalid\n", write_existing_image_docker);

        let err = fx
            .run(&["aibox", "codex", "-e", "relay"], Vec::new())
            .unwrap_err()
            .to_string();

        assert!(err.contains("not a valid KEY=VALUE line"), "{err}");
        assert!(
            !fx.profile().join("home").exists(),
            "invalid env syntax must not create profile home state"
        );
    }

    #[cfg(unix)]
    #[test]
    fn successful_claude_run_merges_env_and_cleans_staged_file() {
        let fx = RunFixture::successful(CLAUDE_RELAY_BODY);
        fx.base(
            "ANTHROPIC_BASE_URL=https://base.example\nANTHROPIC_DEFAULT_HAIKU_MODEL=base-haiku\n",
        );

        let code = fx
            .run(
                &["aibox", "claude", "-e", "relay"],
                vec!["--model".to_string(), "opus".to_string()],
            )
            .unwrap();

        assert_eq!(code, 0);
        assert!(
            fx.profile()
                .join("home")
                .join(".claude")
                .join("statusline.sh")
                .is_file(),
            "a successful Claude run seeds the mounted home before docker run"
        );

        let log = fx.log();
        let run_line = fx.run_line();
        let home_mount = format!("{}:/home/claude", fx.profile().join("home").display());
        let work_mount = format!("{}:/work", std::env::current_dir().unwrap().display());
        assert!(run_line.contains(&format!("<{home_mount}>")), "{run_line}");
        assert!(run_line.contains(&format!("<{work_mount}>")), "{run_line}");
        // The container *is* the sandbox boundary, and these flags are assembled
        // in `runspec` but only reach Docker if `run_agent` actually forwards the
        // assembled args. Both halves are unit-tested; this pins the composition,
        // which is what a real run depends on.
        assert!(
            run_line.contains("<--cap-drop> <ALL>")
                && run_line.contains("<--security-opt> <no-new-privileges>"),
            "a real run must deliver the container hardening flags to docker: {run_line}"
        );
        assert!(
            run_line.contains("<aibox-claude:latest> <--dangerously-skip-permissions>"),
            "Claude runs permissive by default inside the container: {run_line}"
        );
        assert!(
            run_line.ends_with("<--model> <opus>"),
            "pass-through args should remain at the end of the Claude command: {run_line}"
        );
        assert!(
            log.contains("ENV:ANTHROPIC_BASE_URL=https://relay.example\n"),
            "relay values must override base values in the staged Docker env-file:\n{log}"
        );
        assert!(
            log.contains("ENV:ANTHROPIC_DEFAULT_HAIKU_MODEL=base-haiku\n"),
            "base-only values must survive the merge:\n{log}"
        );
        assert!(
            log.contains("ENV:ANTHROPIC_AUTH_TOKEN=sk-claude\n"),
            "{log}"
        );
        let staged_env = token_after_arg(&run_line, "--env-file").expect("staged env-file");
        assert!(
            !std::path::Path::new(&staged_env).exists(),
            "staged merged env-file must be removed after docker run returns"
        );
    }

    #[cfg(unix)]
    #[test]
    fn successful_run_uses_image_override_for_selected_agent() {
        let mut fx = RunFixture::successful(CLAUDE_RELAY_BODY);
        fx.env("AIBOX_IMAGE", "registry.example/team/claude:dev");

        let code = fx
            .run(&["aibox", "claude", "-e", "relay", "--safe"], Vec::new())
            .unwrap();

        assert_eq!(code, 0);
        let run_line = fx.run_line();
        assert!(
            run_line.contains("<registry.example/team/claude:dev>"),
            "run must use the validated AIBOX_IMAGE override: {run_line}"
        );
        assert!(
            !run_line.contains("<aibox-claude:latest>"),
            "custom image override must replace the default image: {run_line}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn successful_run_uses_explicit_env_path_without_scaffolding_relay_files() {
        let fx = RunFixture::bare(write_successful_run_docker);
        let explicit_relay = fx.scratch("external.env");
        std::fs::write(
            &explicit_relay,
            "ANTHROPIC_BASE_URL=https://external.example\nANTHROPIC_AUTH_TOKEN=sk-external\n",
        )
        .unwrap();

        let code = fx
            .run(
                &["aibox", "claude", "-e", explicit_relay.to_str().unwrap()],
                Vec::new(),
            )
            .unwrap();

        assert_eq!(code, 0);
        assert!(
            fx.profile().join("home/.claude/statusline.sh").is_file(),
            "a real run still creates and seeds the mounted home"
        );
        assert!(
            !fx.profile().join("base").exists() && !fx.profile().join("envs").exists(),
            "explicit env-file paths must not scaffold named-relay profile files"
        );

        let log = fx.log();
        assert!(
            log.contains("ENV:ANTHROPIC_BASE_URL=https://external.example\n"),
            "run must read the explicit relay path, not a named envs/ relay:\n{log}"
        );
        assert!(log.contains("ENV:ANTHROPIC_AUTH_TOKEN=sk-external\n"));
    }

    #[cfg(unix)]
    #[test]
    fn successful_run_absolutizes_extra_mount_sources_before_docker() {
        let fx = RunFixture::successful(CLAUDE_RELAY_BODY);

        let code = fx
            .run(
                &[
                    "aibox",
                    "claude",
                    "-e",
                    "relay",
                    "-m",
                    "Cargo.toml:/repo/Cargo.toml:ro",
                    "--mount",
                    "src:/src",
                ],
                Vec::new(),
            )
            .unwrap();

        assert_eq!(code, 0);
        let run_line = fx.run_line();
        let cwd = std::env::current_dir().unwrap();
        assert!(
            run_line.contains(&format!(
                "<{}:/repo/Cargo.toml:ro>",
                cwd.join("Cargo.toml").display()
            )),
            "relative file mount sources must be absolutized before docker sees them: {run_line}"
        );
        assert!(
            run_line.contains(&format!("<{}:/src>", cwd.join("src").display())),
            "relative directory mount sources must be absolutized before docker sees them: {run_line}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn successful_codex_run_assembles_docker_args_and_cleans_staged_key() {
        let fx = RunFixture::successful(CODEX_RELAY_BODY);

        let code = fx
            .run(
                &["aibox", "codex", "-e", "relay", "--safe", "--exec"],
                vec!["explain the repo".to_string()],
            )
            .unwrap();

        assert_eq!(code, 0);
        assert!(
            fx.profile().join("home").join(".codex").is_dir(),
            "a successful Codex run seeds CODEX_HOME before docker run"
        );

        let run_line = fx.run_line();
        let home_mount = format!("{}:/home/codex", fx.profile().join("home").display());
        let work_mount = format!("{}:/work", std::env::current_dir().unwrap().display());
        assert!(run_line.contains(&format!("<{home_mount}>")), "{run_line}");
        assert!(run_line.contains(&format!("<{work_mount}>")), "{run_line}");
        assert!(
            run_line.contains("<aibox-codex:latest> <exec>"),
            "{run_line}"
        );
        assert!(
            run_line.contains("<approval_policy=\"on-request\">")
                && run_line.contains("<-s> <workspace-write>"),
            "--safe exec must use Codex-compatible approval/sandbox args: {run_line}"
        );
        assert!(
            run_line.ends_with("<explain the repo>"),
            "pass-through args should remain at the end of the agent command: {run_line}"
        );
        let staged_key = token_after_arg(&run_line, "--env-file").expect("staged key env-file");
        assert!(
            !std::path::Path::new(&staged_key).exists(),
            "staged key env-file must be removed after docker run returns"
        );
    }

    #[cfg(unix)]
    #[test]
    fn successful_codex_run_inherits_base_model_config() {
        let fx = RunFixture::successful(
            "CODEX_BASE_URL=https://relay.example/v1\nCODEX_API_KEY=sk-test\n",
        );
        fx.base("CODEX_MODEL=base-model\nCODEX_REASONING=high\n");

        let code = fx
            .run(&["aibox", "codex", "-e", "relay"], Vec::new())
            .unwrap();

        assert_eq!(code, 0);
        let log = fx.log();
        let run_line = fx.run_line();
        assert!(
            run_line.contains("<model=\"base-model\">"),
            "Codex must inherit CODEX_MODEL from profile/base when relay omits it: {run_line}"
        );
        assert!(
            run_line.contains("<model_reasoning_effort=\"high\">"),
            "Codex must inherit optional model config from profile/base: {run_line}"
        );
        assert!(
            log.contains("ENV:OPENAI_API_KEY=sk-test\n"),
            "relay-only API key still reaches the staged Codex env-file:\n{log}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn successful_codex_auth_json_run_cleans_staged_mount_and_placeholder() {
        let fx = RunFixture::successful(
            "CODEX_BASE_URL=https://relay.example/v1\nCODEX_API_KEY=sk-we\"ird\\key\nCODEX_MODEL=gpt-test\nCODEX_REQUIRES_OPENAI_AUTH=1\n",
        );

        let code = fx
            .run(&["aibox", "codex", "-e", "relay"], Vec::new())
            .unwrap();

        assert_eq!(code, 0);
        let log = fx.log();
        let run_line = fx.run_line();
        let run_line = run_line.as_str();
        assert!(
            !run_line.contains("<--env-file>"),
            "auth.json mode must not also send OPENAI_API_KEY through env_key mode: {run_line}"
        );
        assert!(
            run_line.contains("<model_providers.aibox.requires_openai_auth=true>"),
            "Codex must be told to read the mounted auth.json: {run_line}"
        );
        assert!(
            !run_line.contains("model_providers.aibox.env_key"),
            "Codex auth modes must stay mutually exclusive: {run_line}"
        );
        let auth_json = log
            .lines()
            .find_map(|line| line.strip_prefix("AUTH:"))
            .expect("fake docker records the staged auth.json body");
        let auth: serde_json::Value = serde_json::from_str(auth_json).unwrap();
        assert_eq!(
            auth["OPENAI_API_KEY"], r#"sk-we"ird\key"#,
            "auth.json mode must stage a valid JSON credential file with the relay key"
        );
        let staged_auth = mounted_source_for(run_line, "/home/codex/.codex/auth.json")
            .expect("staged auth.json bind mount");
        assert!(
            !std::path::Path::new(&staged_auth).exists(),
            "staged auth.json must be removed after docker run returns"
        );
        assert!(
            !fx.profile().join("home/.codex/auth.json").exists(),
            "pre-created auth.json placeholder must be removed after docker run returns"
        );
    }

    #[cfg(unix)]
    #[test]
    fn successful_codex_auth_json_run_preserves_existing_login_file() {
        let fx = RunFixture::successful(CODEX_AUTH_JSON_RELAY_BODY);
        let auth = fx.profile().join("home/.codex/auth.json");
        let real_login = "{\"refresh_token\":\"real-user-login\"}\n";
        std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
        std::fs::write(&auth, real_login).unwrap();

        let code = fx
            .run(&["aibox", "codex", "-e", "relay"], Vec::new())
            .unwrap();

        assert_eq!(code, 0);
        assert_eq!(
            std::fs::read_to_string(&auth).unwrap(),
            real_login,
            "auth.json mode must not delete or overwrite an existing codex login file"
        );
        let log = fx.log();
        let run_line = fx.run_line();
        assert!(
            run_line.contains(":/home/codex/.codex/auth.json:ro"),
            "the run still uses a staged read-only auth.json mount: {run_line}"
        );
        assert!(
            log.contains("AUTH:{\"OPENAI_API_KEY\":\"sk-test\"}\n"),
            "the staged run credential must come from the relay, not from the existing login file:\n{log}"
        );
    }

    /// AGENTS.md's flat rule: secrets are staged in `$TMPDIR`, never written
    /// into the mounted profile home — SIGKILL skips `Drop` *and* the signal
    /// watcher, so anything under `home/` would survive indefinitely with no
    /// cleanup path left. The per-file tests above each check their own path is
    /// gone; this checks the stronger property, that no file anywhere in the
    /// home ever contained the key, for both agents and both Codex auth modes.
    #[cfg(unix)]
    #[test]
    fn credentials_never_land_in_the_mounted_profile_home() {
        const SECRET: &str = "sk-must-never-touch-the-home";

        for (agent, relay_body) in [
            (
                "claude",
                format!("ANTHROPIC_BASE_URL=https://relay.example\nANTHROPIC_AUTH_TOKEN={SECRET}\n"),
            ),
            (
                "codex",
                format!(
                    "CODEX_BASE_URL=https://relay.example/v1\nCODEX_API_KEY={SECRET}\nCODEX_MODEL=gpt-test\n"
                ),
            ),
            (
                "codex",
                format!(
                    "CODEX_BASE_URL=https://relay.example/v1\nCODEX_API_KEY={SECRET}\nCODEX_MODEL=gpt-test\nCODEX_REQUIRES_OPENAI_AUTH=1\n"
                ),
            ),
        ] {
            let fx = RunFixture::successful(&relay_body);

            let code = fx.run(&["aibox", agent, "-e", "relay"], Vec::new()).unwrap();
            assert_eq!(code, 0, "{agent}: {relay_body}");

            // The key did reach the container (via env-file or auth.json mount),
            // so this is not vacuously true.
            let log = fx.log();
            assert!(
                log.contains(SECRET),
                "{agent}: the relay key must still reach the container:\n{log}"
            );

            for entry in walkdir::WalkDir::new(fx.profile().join("home")) {
                let entry = entry.unwrap();
                if !entry.file_type().is_file() {
                    continue;
                }
                let body = std::fs::read(entry.path()).unwrap();
                assert!(
                    !String::from_utf8_lossy(&body).contains(SECRET),
                    "{agent}: {} in the mounted home holds the relay key; SIGKILL would leave it there",
                    entry.path().display()
                );
            }
        }
    }

    /// An agent failure is still a handled exit: `docker run` returns its
    /// non-zero status as `Ok(code)`, after which the invocation must be dropped
    /// exactly like it is on success. Exercise all three credential deliveries
    /// end to end so a future early return cannot leave a staged env-file,
    /// staged auth.json, or fixed-path placeholder behind.
    #[cfg(unix)]
    #[test]
    fn nonzero_agent_exit_cleans_credentials_in_every_auth_mode() {
        const SECRET: &str = "sk-clean-after-agent-failure";

        for (agent, relay_body, uses_auth_json) in [
            (
                "claude",
                format!(
                    "ANTHROPIC_BASE_URL=https://relay.example\nANTHROPIC_AUTH_TOKEN={SECRET}\n"
                ),
                false,
            ),
            (
                "codex",
                format!(
                    "CODEX_BASE_URL=https://relay.example/v1\nCODEX_API_KEY={SECRET}\nCODEX_MODEL=gpt-test\n"
                ),
                false,
            ),
            (
                "codex",
                format!(
                    "CODEX_BASE_URL=https://relay.example/v1\nCODEX_API_KEY={SECRET}\nCODEX_MODEL=gpt-test\nCODEX_REQUIRES_OPENAI_AUTH=1\n"
                ),
                true,
            ),
        ] {
            let mut fx = RunFixture::successful(&relay_body);
            let staging_dir = tempfile::tempdir().unwrap();
            fx.env("AIBOX_FAKE_DOCKER_RUN_EXIT", "42");
            fx.env_os("TMPDIR", staging_dir.path().as_os_str());

            let code = fx.run(&["aibox", agent, "-e", "relay"], Vec::new()).unwrap();

            assert_eq!(code, 42, "{agent}: the agent's failure code must survive");
            assert!(
                fx.log().contains(SECRET),
                "{agent}: prove the credential existed and reached Docker before checking cleanup"
            );
            assert!(
                std::fs::read_dir(staging_dir.path())
                    .unwrap()
                    .next()
                    .is_none(),
                "{agent}: handled non-zero exit must remove every staged temp file"
            );
            if uses_auth_json {
                assert!(
                    !fx.profile().join("home/.codex/auth.json").exists(),
                    "auth.json mode must also remove its fixed-path placeholder"
                );
            }
        }
    }

    /// `build_codex` stages the selected credential mode before translating
    /// optional query parameters. A malformed late option therefore exercises
    /// cleanup *inside* invocation construction, before `docker run` exists.
    /// Both Codex auth modes must unwind without leaving temp credentials; the
    /// auth.json mode must also remove the guarded mount target.
    #[cfg(unix)]
    #[test]
    fn post_staging_invocation_error_cleans_both_codex_auth_modes() {
        for requires_auth_json in [false, true] {
            let auth_mode = if requires_auth_json {
                "CODEX_REQUIRES_OPENAI_AUTH=1\n"
            } else {
                ""
            };
            let relay_body =
                format!("{CODEX_RELAY_BODY}{auth_mode}CODEX_QUERY_PARAMS=missing-equals\n");
            let mut fx = RunFixture::successful(&relay_body);
            let staging_dir = tempfile::tempdir().unwrap();
            fx.env_os("TMPDIR", staging_dir.path().as_os_str());

            let err = fx
                .run(&["aibox", "codex", "-e", "relay"], Vec::new())
                .unwrap_err()
                .to_string();

            assert!(err.contains("must be k=v"), "{err}");
            assert!(
                !fx.log().lines().any(|line| line.starts_with("ARGS: <run>")),
                "invocation validation must fail before Docker starts"
            );
            assert!(
                std::fs::read_dir(staging_dir.path())
                    .unwrap()
                    .next()
                    .is_none(),
                "auth.json={requires_auth_json}: builder error must unlink staged credentials"
            );
            assert!(
                !fx.profile().join("home/.codex/auth.json").exists(),
                "auth.json={requires_auth_json}: builder error must not leave a mount placeholder"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn docker_spawn_failure_cleans_codex_staged_auth_and_placeholder() {
        // The stub deletes itself after the image check, so the `docker run`
        // spawn fails *after* credentials have been staged — the window where a
        // leaked auth.json would be invisible to the normal exit path.
        let mut fx = RunFixture::new(
            CODEX_AUTH_JSON_RELAY_BODY,
            write_docker_that_disappears_after_image_check,
        );
        let staging_dir = tempfile::tempdir().unwrap();
        let docker_dir = fx.docker_dir();
        let docker_path = docker_dir.join("docker");
        fx.env_os("PATH", docker_dir.as_os_str());
        fx.env_os("AIBOX_FAKE_DOCKER_PATH_TO_DELETE", docker_path.as_os_str());
        fx.env_os("TMPDIR", staging_dir.path().as_os_str());

        let err = fx
            .run(&["aibox", "codex", "-e", "relay"], Vec::new())
            .unwrap_err()
            .to_string();

        assert!(err.contains("spawn docker run"), "{err}");
        assert!(
            std::fs::read_dir(staging_dir.path())
                .unwrap()
                .next()
                .is_none(),
            "a spawn failure must still unlink the staged auth.json"
        );
        assert!(
            !fx.profile().join("home/.codex/auth.json").exists(),
            "a spawn failure must remove the pre-created auth.json placeholder"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_rejects_symlinked_profile_dir_before_scaffold() {
        use std::os::unix::fs::symlink;

        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let outside = root.path().join("outside-profile");
        std::fs::create_dir(&config_root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, config_root.join("default")).unwrap();
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "codex", "-e", "relay"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(
            err.contains("profile directory is not a real directory"),
            "{err}"
        );
        assert!(
            !outside.join("home").exists(),
            "run scaffolding must not create home through a symlinked profile"
        );
        assert!(
            !outside.join("envs").exists(),
            "run scaffolding must not create envs through a symlinked profile"
        );
        assert!(
            !outside.join("base").exists(),
            "run scaffolding must not create base through a symlinked profile"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_rejects_symlinked_home_before_named_relay_scaffold() {
        use std::os::unix::fs::symlink;

        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let outside = root.path().join("outside-home");
        std::fs::create_dir_all(config_root.join("default")).unwrap();
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, config_root.join("default/home")).unwrap();
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "codex", "-e", "relay"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(
            err.contains("profile home is not a real directory"),
            "{err}"
        );
        assert!(
            !config_root.join("default/envs").exists(),
            "named relay validation must not scaffold envs after a home boundary failure"
        );
        assert!(
            !config_root.join("default/base").exists(),
            "named relay validation must not scaffold base after a home boundary failure"
        );
        assert!(
            !outside.join("envs").exists(),
            "named relay validation must not write through the home symlink"
        );
    }

    #[cfg(unix)]
    #[test]
    fn refresh_rejects_symlinked_profile_dir_without_writing_target() {
        use std::os::unix::fs::symlink;

        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let outside = root.path().join("outside-profile");
        std::fs::create_dir(&config_root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("base"), "unchanged\n").unwrap();
        symlink(&outside, config_root.join("default")).unwrap();
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "codex", "refresh"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(
            err.contains("profile directory is not a real directory"),
            "{err}"
        );
        assert_eq!(
            std::fs::read_to_string(outside.join("base")).unwrap(),
            "unchanged\n",
            "refresh must not write through a symlinked profile"
        );
    }

    #[cfg(unix)]
    #[test]
    fn session_delete_rejects_symlinked_profile_dir_without_deleting_target() {
        use std::os::unix::fs::symlink;

        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let outside = root.path().join("outside-profile");
        let transcript = outside.join(
            "home/.codex/sessions/2026/07/14/rollout-x-aaaaaaaa-1111-2222-3333-444455556666.jsonl",
        );
        std::fs::create_dir(&config_root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(
            &transcript,
            r#"{"timestamp":"2026-07-14T02:16:00Z","type":"session_meta","payload":{}}"#,
        )
        .unwrap();
        symlink(&outside, config_root.join("default")).unwrap();
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "codex", "session", "delete", "-y"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(
            err.contains("profile directory is not a real directory"),
            "{err}"
        );
        assert!(
            transcript.exists(),
            "session delete must not remove files through a symlinked profile"
        );
    }

    #[test]
    fn extra_mount_targets_are_validated_before_scaffold() {
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli =
            Cli::try_parse_from(["aibox", "codex", "-e", "relay", "-m", "src:/work"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(
            err.contains("would override or shadow an aibox-managed mount"),
            "{err}"
        );
        assert!(
            !config_root.join("default").exists(),
            "invalid extra mount must not create profile state"
        );
    }

    #[test]
    fn invalid_mount_mode_is_rejected_before_scaffold() {
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli =
            Cli::try_parse_from(["aibox", "codex", "-e", "relay", "-m", "src:/cache:rw"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(err.contains("invalid mount mode"), "{err}");
        assert!(
            !config_root.join("default").exists(),
            "invalid mount mode must not create profile state"
        );
    }
}
