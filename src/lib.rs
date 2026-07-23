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

        #[cfg(unix)]
        fn prepend_path(dir: &std::path::Path) -> Self {
            let old = std::env::var_os("PATH");
            let mut paths = vec![dir.to_path_buf()];
            if let Some(old_path) = &old {
                paths.extend(std::env::split_paths(old_path));
            }
            let joined = std::env::join_paths(paths).unwrap();
            std::env::set_var("PATH", joined);
            EnvGuard { name: "PATH", old }
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

    #[cfg(unix)]
    fn write_missing_image_docker(dir: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join("docker");
        std::fs::write(
            &path,
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
        )
        .unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

    #[cfg(unix)]
    fn write_existing_image_docker(dir: &std::path::Path) {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join("docker");
        std::fs::write(
            &path,
            r#"#!/bin/sh
if [ "$1" = "image" ] && [ "$2" = "inspect" ]; then
    printf 'sha256:fake-image\n'
    exit 0
fi
exit 99
"#,
        )
        .unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
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

    #[test]
    fn image_refs_reject_docker_option_shaped_overrides() {
        for bad in ["--privileged", "-bad", "bad tag", "bad\nname", "bad\tname"] {
            let err = image_for(AgentKind::Codex, Some(bad))
                .unwrap_err()
                .to_string();
            assert!(err.contains("Docker image reference"), "{err}");
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
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let relay = config_root.join("default").join("envs").join("relay");
        std::fs::create_dir_all(relay.parent().unwrap()).unwrap();
        std::fs::write(
            &relay,
            "CODEX_BASE_URL=https://relay.example/v1\nCODEX_API_KEY=sk-test\nCODEX_MODEL=gpt-test\n",
        )
        .unwrap();

        let docker_dir = tempfile::tempdir().unwrap();
        write_missing_image_docker(docker_dir.path());
        let _path = EnvGuard::prepend_path(docker_dir.path());
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "codex", "-e", "relay"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(err.contains("build it first"), "{err}");
        assert!(
            !config_root.join("default").join("home").exists(),
            "missing image must not create profile home"
        );
    }

    #[cfg(unix)]
    #[test]
    fn malformed_env_is_rejected_before_home_creation() {
        let _env_lock = test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let config_root = root.path().join("aibox-config");
        let relay = config_root.join("default").join("envs").join("relay");
        std::fs::create_dir_all(relay.parent().unwrap()).unwrap();
        std::fs::write(&relay, "CODEX_API_KEY = sk-invalid\n").unwrap();

        let docker_dir = tempfile::tempdir().unwrap();
        write_existing_image_docker(docker_dir.path());
        let _path = EnvGuard::prepend_path(docker_dir.path());
        let _config = EnvGuard::set("AIBOX_CONFIG_ROOT", config_root.to_str().unwrap());

        let cli = Cli::try_parse_from(["aibox", "codex", "-e", "relay"]).unwrap();
        let err = run(cli, Vec::new()).unwrap_err().to_string();

        assert!(err.contains("not a valid KEY=VALUE line"), "{err}");
        assert!(
            !config_root.join("default").join("home").exists(),
            "invalid env syntax must not create profile home state"
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
