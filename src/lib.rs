//! aibox — run coding agents inside a Docker container that is the sandbox
//! boundary, with host-side provider configuration management.

pub mod agent;
pub mod cli;
pub mod config;
pub mod creds;
pub mod docker;
pub mod merge;
pub mod platform;
pub mod profile;
pub mod runspec;
pub mod session;
mod session_claude;
mod session_codex;
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
use profile::Profile;
use runspec::RunOpts;

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

pub(crate) fn print_line(line: &str) -> Result<bool> {
    write_line(&mut std::io::stdout().lock(), line)
}

fn write_line(out: &mut impl std::io::Write, line: &str) -> Result<bool> {
    match writeln!(out, "{line}") {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(e) => Err(e).context("write to stdout"),
    }
}

pub fn run(cli: Cli, passthrough: Vec<String>) -> Result<i32> {
    match cli.command {
        Command::Build(args) => {
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
        if !passthrough.is_empty() {
            anyhow::bail!(
                "`-- <args>` applies only to a run; config/session take no pass-through args"
            );
        }
        reject_run_only_options(&args.run)?;
        let root = profile::config_root()?;
        let prof = Profile::resolve(agent, &root, &args.run.profile)?;
        return match action {
            Action::Config { command } => config::dispatch(agent, &prof, command),
            Action::Session { action, ids, yes } => {
                prof.validate_session_home()?;
                session::dispatch(agent, &prof.home_dir, action, ids, *yes)
            }
        };
    }

    run_agent(agent, &args.run, passthrough)
}

fn reject_run_only_options(run: &RunArgs) -> Result<()> {
    let mut used = Vec::new();
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
            "config/session do not accept run-only options: {}",
            used.join(", ")
        );
    }
    Ok(())
}

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

fn run_agent(agent: AgentKind, run: &RunArgs, passthrough: &[String]) -> Result<i32> {
    if run.exec && !agent.supports_exec() {
        anyhow::bail!("--exec is codex-only");
    }

    let image_override = env_override("AIBOX_IMAGE")?;
    let image = image_for(agent, image_override.as_deref())?;
    if image_override.is_some() {
        eprintln!(">> image overridden by $AIBOX_IMAGE: {image}");
    }

    let root = profile::config_root()?;
    let prof = Profile::resolve(agent, &root, &run.profile)?;
    if prof.is_host() {
        anyhow::bail!("profile 'host' is only valid for config/session commands, not Docker runs");
    }
    profile::real_dir_exists(&prof.home_dir, "profile home")?;

    let work_dir = runspec::resolve_work_dir(run.work.as_deref())?;
    let mounts = runspec::resolve_mounts(&run.mount)?;
    runspec::validate_extra_mount_targets(agent, &mounts)?;
    runspec::reject_colon_in_bind_source("profile home", &prof.home_dir)?;

    if !docker::image_exists(&image)? {
        anyhow::bail!(
            "{image} is not present locally; build it first with `aibox build {}`",
            agent.tag()
        );
    }

    profile::ensure_real_dir(&prof.home_dir, "profile home")?;
    runspec::seed_home(agent, &prof.home_dir)?;

    let opts = RunOpts {
        safe: run.safe,
        exec: run.exec,
        passthrough,
    };
    let invocation = agent.build_invocation(&opts)?;

    let run_args = runspec::assemble_run_args(
        agent,
        &work_dir,
        &prof.home_dir,
        &mounts,
        &invocation.extra_run_args,
    );

    docker::run(&run_args, &image, &invocation.agent_cmd, || {})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::EnvGuard;
    use clap::Parser;

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
    while [ "$#" -gt 0 ]; do
        if [ "$1" = "--cidfile" ]; then
            cid="$2"
            shift 2
        else
            shift
        fi
    done
    printf 'fake-container\n' > "$cid"
    exit 0
fi

exit 99
"#,
        );
    }

    #[cfg(unix)]
    struct RunFixture {
        _env_lock: std::sync::MutexGuard<'static, ()>,
        _run_lock: std::sync::MutexGuard<'static, ()>,
        root: tempfile::TempDir,
        _docker_dir: tempfile::TempDir,
        docker_log: std::path::PathBuf,
        _guards: Vec<EnvGuard>,
    }

    #[cfg(unix)]
    impl RunFixture {
        fn new() -> Self {
            let env_lock = test_env_lock();
            let run_lock = crate::creds::run_registry_test_lock();
            let root = tempfile::tempdir().unwrap();
            let host_home = root.path().join("host-home");
            std::fs::create_dir(&host_home).unwrap();
            let docker_dir = tempfile::tempdir().unwrap();
            let docker_log = docker_dir.path().join("docker.log");
            write_successful_run_docker(docker_dir.path());
            let guards = vec![
                EnvGuard::prepend_path(docker_dir.path()),
                EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", docker_log.as_os_str()),
                EnvGuard::set("AIBOX_CONFIG_ROOT", root.path().as_os_str()),
                EnvGuard::set("HOME", host_home.as_os_str()),
            ];
            Self {
                _env_lock: env_lock,
                _run_lock: run_lock,
                root,
                _docker_dir: docker_dir,
                docker_log,
                _guards: guards,
            }
        }

        fn run(&self, argv: &[&str], passthrough: Vec<String>) -> Result<i32> {
            let cli = Cli::try_parse_from(argv.iter().copied()).unwrap();
            run(cli, passthrough)
        }

        fn log(&self) -> String {
            std::fs::read_to_string(&self.docker_log).unwrap_or_default()
        }
    }

    #[test]
    fn image_ref_rejects_other_default_agent_image() {
        let err = validate_image_ref(AgentKind::Codex, "aibox-claude:latest")
            .unwrap_err()
            .to_string();
        assert!(err.contains("default claude image"));
    }

    #[cfg(unix)]
    #[test]
    fn run_uses_shared_profile_home_without_provider_injection() {
        let fx = RunFixture::new();
        let code = fx.run(&["aibox", "codex"], Vec::new()).unwrap();
        assert_eq!(code, 0);

        let log = fx.log();
        assert!(log.contains(&format!(
            "<{}:/home/codex>",
            fx.root.path().join("default").display()
        )));
        assert!(fx.root.path().join("default/.codex").is_dir());
        assert!(!log.contains("<--env-file>"), "{log}");
        assert!(!log.contains("<-c>"), "{log}");
    }

    #[cfg(unix)]
    #[test]
    fn claude_run_seeds_statusline_but_not_settings() {
        let fx = RunFixture::new();
        fx.run(&["aibox", "claude"], Vec::new()).unwrap();

        assert!(fx
            .root
            .path()
            .join("default/.claude/statusline.sh")
            .exists());
        assert!(!fx
            .root
            .path()
            .join("default/.claude/settings.json")
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn host_profile_is_rejected_for_run_but_allowed_for_session() {
        let fx = RunFixture::new();
        let err = fx
            .run(&["aibox", "codex", "-p", "host"], Vec::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("profile 'host' is only valid"));

        let code = fx
            .run(&["aibox", "codex", "-p", "host", "session"], Vec::new())
            .unwrap();
        assert_eq!(code, 0);
    }

    #[cfg(unix)]
    #[test]
    fn config_and_session_reject_passthrough_and_run_only_options() {
        let fx = RunFixture::new();
        let err = fx
            .run(
                &["aibox", "codex", "config", "list"],
                vec!["ignored".to_string()],
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("applies only to a run"));

        let err = fx
            .run(&["aibox", "codex", "--safe", "config", "list"], Vec::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("config/session do not accept run-only options"));
    }

    #[cfg(unix)]
    #[test]
    fn invalid_run_mount_does_not_create_profile_home() {
        let fx = RunFixture::new();
        let err = fx
            .run(&["aibox", "codex", "-m", "/no/such/dir:/cache"], Vec::new())
            .unwrap_err()
            .to_string();

        assert!(err.contains("mount host path does not exist"), "{err}");
        assert!(!fx.root.path().join("default").exists());
    }

    #[test]
    fn write_line_treats_broken_pipe_as_clean_stop() {
        struct Broken;
        impl std::io::Write for Broken {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert!(!write_line(&mut Broken, "x").unwrap());
    }
}
