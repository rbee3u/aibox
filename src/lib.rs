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
use cli::{BuildArgs, Cli, Command, RunArgs, SessionArgs};
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

fn image_for(image_override: Option<&str>) -> Result<String> {
    let image = image_override.unwrap_or(docker::IMAGE);
    validate_image_ref(image)?;
    Ok(image.to_string())
}

fn validate_image_ref(image: &str) -> Result<()> {
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
    Ok(())
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
    let Cli {
        agent: root_agent,
        run: run_args,
        command,
    } = cli;

    match command {
        None => run_agent(
            root_agent.unwrap_or(AgentKind::Codex),
            &run_args,
            &passthrough,
        ),
        Some(Command::Build(args)) => {
            if !passthrough.is_empty() {
                anyhow::bail!(
                    "`-- <args>` applies only to a run; build takes no pass-through args"
                );
            }
            if root_agent.is_some() {
                anyhow::bail!("build does not accept --agent");
            }
            reject_command_run_options("build", &run_args)?;
            run_build(&args)
        }
        Some(Command::Profile(args)) => {
            if !passthrough.is_empty() {
                anyhow::bail!(
                    "`-- <args>` applies only to a run; profile takes no pass-through args"
                );
            }
            if root_agent.is_some() {
                anyhow::bail!("profile is shared across agents and does not accept --agent");
            }
            reject_command_run_options("profile", &run_args)?;
            profile::dispatch(&args.command)
        }
        Some(Command::Config(args)) => {
            let agent =
                resolve_agent3(root_agent, args.agent, config_command_agent(&args.command))?
                    .unwrap_or(AgentKind::Codex);
            run_config_command(agent, &run_args, &args.command, &passthrough)
        }
        Some(Command::Session(args)) => {
            let agent = resolve_agent3(
                root_agent,
                args.agent,
                session_command_agent(args.command.as_ref()),
            )?
            .unwrap_or(AgentKind::Codex);
            run_session_command(agent, &run_args, &args, &passthrough)
        }
    }
}

fn resolve_agent(
    root_agent: Option<AgentKind>,
    command_agent: Option<AgentKind>,
) -> Result<Option<AgentKind>> {
    match (root_agent, command_agent) {
        (Some(root), Some(command)) if root != command => {
            anyhow::bail!("--agent must be provided only once")
        }
        (Some(agent), Some(_)) => Ok(Some(agent)),
        (Some(agent), None) | (None, Some(agent)) => Ok(Some(agent)),
        (None, None) => Ok(None),
    }
}

fn resolve_agent3(
    root_agent: Option<AgentKind>,
    command_agent: Option<AgentKind>,
    subcommand_agent: Option<AgentKind>,
) -> Result<Option<AgentKind>> {
    let agent = resolve_agent(root_agent, command_agent)?;
    resolve_agent(agent, subcommand_agent)
}

fn config_command_agent(command: &cli::ConfigCommand) -> Option<AgentKind> {
    match command {
        cli::ConfigCommand::List { agent }
        | cli::ConfigCommand::Get { agent, .. }
        | cli::ConfigCommand::Create { agent, .. }
        | cli::ConfigCommand::Apply { agent, .. }
        | cli::ConfigCommand::Edit { agent, .. }
        | cli::ConfigCommand::Delete { agent, .. } => *agent,
    }
}

fn session_command_agent(command: Option<&cli::SessionCommand>) -> Option<AgentKind> {
    match command {
        None => None,
        Some(cli::SessionCommand::List { agent })
        | Some(cli::SessionCommand::Get { agent, .. })
        | Some(cli::SessionCommand::Delete { agent, .. }) => *agent,
    }
}

fn run_config_command(
    agent: AgentKind,
    run: &RunArgs,
    command: &cli::ConfigCommand,
    passthrough: &[String],
) -> Result<i32> {
    if !passthrough.is_empty() {
        anyhow::bail!(
            "`-- <args>` applies only to a run; config/session take no pass-through args"
        );
    }
    reject_run_only_options(run)?;
    let root = profile::config_root()?;
    let prof = Profile::resolve(agent, &root, run.profile_name())?;
    config::dispatch(agent, &prof, command)
}

fn run_session_command(
    agent: AgentKind,
    run: &RunArgs,
    args: &SessionArgs,
    passthrough: &[String],
) -> Result<i32> {
    if !passthrough.is_empty() {
        anyhow::bail!(
            "`-- <args>` applies only to a run; config/session take no pass-through args"
        );
    }
    reject_run_only_options(run)?;
    let root = profile::config_root()?;
    let prof = Profile::resolve(agent, &root, run.profile_name())?;
    prof.validate_session_home()?;
    match args.command.as_ref() {
        None | Some(cli::SessionCommand::List { .. }) => {
            session::dispatch(agent, &prof.home_dir, "list", &[], false, false)
        }
        Some(cli::SessionCommand::Get { id, .. }) => session::dispatch(
            agent,
            &prof.home_dir,
            "get",
            std::slice::from_ref(id),
            false,
            false,
        ),
        Some(cli::SessionCommand::Delete { ids, all, yes, .. }) => {
            session::dispatch(agent, &prof.home_dir, "delete", ids, *all, *yes)
        }
    }
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

fn reject_command_run_options(command: &str, run: &RunArgs) -> Result<()> {
    let mut used = Vec::new();
    if run.profile.is_some() {
        used.push("--profile");
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
        anyhow::bail!("{command} does not accept run options: {}", used.join(", "));
    }
    Ok(())
}

fn run_build(args: &BuildArgs) -> Result<i32> {
    let image_override = env_override("AIBOX_IMAGE")?;
    let image = image_for(image_override.as_deref())?;
    let cache = if args.force {
        BuildCache::NoCachePull
    } else {
        BuildCache::Cached
    };
    if args.force {
        eprintln!(">> building {image} (no cache, pulling fresh Debian base) ...");
    } else {
        eprintln!(">> building {image} (cache enabled) ...");
    }
    docker::build_image(docker::DOCKERFILE, &image, cache).context("build aibox image")?;

    Ok(0)
}

fn run_agent(agent: AgentKind, run: &RunArgs, passthrough: &[String]) -> Result<i32> {
    if run.exec && !agent.supports_exec() {
        anyhow::bail!("--exec is codex-only");
    }

    let image_override = env_override("AIBOX_IMAGE")?;
    let image = image_for(image_override.as_deref())?;
    if image_override.is_some() {
        eprintln!(">> image overridden by $AIBOX_IMAGE: {image}");
    }

    let root = profile::config_root()?;
    let prof = Profile::resolve(agent, &root, run.profile_name())?;
    if prof.is_host() {
        anyhow::bail!("profile 'host' is only valid for config/session commands, not Docker runs");
    }
    profile::real_dir_exists(&prof.home_dir, "profile home")?;

    let work_dir = runspec::resolve_work_dir(run.work.as_deref())?;
    let mounts = runspec::resolve_mounts(&run.mount)?;
    runspec::validate_extra_mount_targets(agent, &mounts)?;
    runspec::reject_colon_in_bind_source("profile home", &prof.home_dir)?;

    if !docker::image_exists(&image)? {
        anyhow::bail!("{image} is not present locally; build it first with `aibox build`");
    }

    prof.ensure_ordinary_initialized()?;

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
    fn write_successful_build_docker(dir: &std::path::Path) {
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
done
printf '\nSTDIN:' >> "$log"
cat >> "$log"
printf '\nEND\n' >> "$log"
"#,
        );
    }

    #[cfg(unix)]
    struct RunFixture {
        // Fields drop in declaration order. Restore env before deleting stub
        // dirs, and release the env lock last so parallel tests can't observe a
        // half-restored PATH.
        _guards: Vec<EnvGuard>,
        _docker_dir: tempfile::TempDir,
        root: tempfile::TempDir,
        docker_log: std::path::PathBuf,
        _run_lock: std::sync::MutexGuard<'static, ()>,
        _env_lock: std::sync::MutexGuard<'static, ()>,
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
                EnvGuard::set("AIBOX_ROOT", root.path().as_os_str()),
                EnvGuard::set("HOME", host_home.as_os_str()),
            ];
            Self {
                _guards: guards,
                _docker_dir: docker_dir,
                root,
                docker_log,
                _run_lock: run_lock,
                _env_lock: env_lock,
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
    fn image_ref_validation_rejects_bad_refs() {
        validate_image_ref("aibox:latest").unwrap();
        assert!(validate_image_ref("")
            .unwrap_err()
            .to_string()
            .contains("empty"));
        assert!(validate_image_ref("--bad")
            .unwrap_err()
            .to_string()
            .contains("must not start"));
        assert!(validate_image_ref("bad image")
            .unwrap_err()
            .to_string()
            .contains("whitespace"));
    }

    #[cfg(unix)]
    #[test]
    fn build_uses_single_image_and_aibox_image_override() {
        let _env_lock = test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("docker-build.log");
        write_successful_build_docker(dir.path());
        let _path = EnvGuard::prepend_path(dir.path());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log.as_os_str());
        let _image = EnvGuard::set("AIBOX_IMAGE", "local/aibox:dev");

        let cli = Cli::try_parse_from(["aibox", "build", "--force"]).unwrap();
        let code = run(cli, Vec::new()).unwrap();

        assert_eq!(code, 0);
        let log = std::fs::read_to_string(log).unwrap();
        assert!(log.contains("<--no-cache> <--pull>"), "{log}");
        assert!(log.contains("<-t> <local/aibox:dev>"), "{log}");
        assert!(log.contains("STDIN:# aibox.Dockerfile"), "{log}");
        assert_eq!(log.matches("ARGS: <build>").count(), 1, "{log}");
    }

    #[cfg(unix)]
    #[test]
    fn default_run_uses_codex_shared_profile_home_without_provider_injection() {
        let fx = RunFixture::new();
        let code = fx.run(&["aibox"], Vec::new()).unwrap();
        assert_eq!(code, 0);

        let log = fx.log();
        assert!(log.contains(&format!(
            "<{}:/home/aibox>",
            fx.root.path().join("default").display()
        )));
        assert!(
            log.contains("<aibox:latest> <codex> <--dangerously-bypass-approvals-and-sandbox>"),
            "{log}"
        );
        assert!(fx.root.path().join("default/.codex").is_dir());
        assert!(fx
            .root
            .path()
            .join("default/.claude/statusline.sh")
            .is_file());
        assert!(fx.root.path().join("default/.gitconfig").is_file());
        assert!(fx.root.path().join(".config/default/codex").is_dir());
        assert!(fx.root.path().join(".config/default/claude").is_dir());
        assert!(!log.contains("<--env-file>"), "{log}");
        assert!(!log.contains("<-c>"), "{log}");
    }

    #[cfg(unix)]
    #[test]
    fn claude_run_seeds_statusline_but_not_settings() {
        let fx = RunFixture::new();
        fx.run(&["aibox", "--agent", "claude"], Vec::new()).unwrap();

        let log = fx.log();
        assert!(log.contains(&format!(
            "<{}:/home/aibox>",
            fx.root.path().join("default").display()
        )));
        assert!(
            log.contains("<aibox:latest> <claude> <--dangerously-skip-permissions>"),
            "{log}"
        );
        assert!(fx
            .root
            .path()
            .join("default/.claude/statusline.sh")
            .exists());
        assert!(fx.root.path().join("default/.codex").is_dir());
        assert!(fx.root.path().join("default/.gitconfig").is_file());
        assert!(fx.root.path().join(".config/default/codex").is_dir());
        assert!(fx.root.path().join(".config/default/claude").is_dir());
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
            .run(&["aibox", "-p", "host"], Vec::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("profile 'host' is only valid"));

        let code = fx
            .run(&["aibox", "-p", "host", "session"], Vec::new())
            .unwrap();
        assert_eq!(code, 0);
    }

    #[cfg(unix)]
    #[test]
    fn config_and_session_reject_passthrough_and_run_only_options() {
        let fx = RunFixture::new();
        let err = fx
            .run(&["aibox", "config", "list"], vec!["ignored".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("applies only to a run"));

        let err = fx
            .run(&["aibox", "--safe", "config", "list"], Vec::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("config/session do not accept run-only options"));
    }

    #[cfg(unix)]
    #[test]
    fn invalid_run_mount_does_not_create_profile_home() {
        let fx = RunFixture::new();
        let err = fx
            .run(&["aibox", "-m", "/no/such/dir:/cache"], Vec::new())
            .unwrap_err()
            .to_string();

        assert!(err.contains("mount host path does not exist"), "{err}");
        assert!(!fx.root.path().join("default").exists());
    }

    #[cfg(unix)]
    #[test]
    fn claude_exec_and_profile_agent_flag_are_rejected() {
        let fx = RunFixture::new();
        let err = fx
            .run(&["aibox", "--agent", "claude", "--exec"], Vec::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("--exec is codex-only"));

        let err = fx
            .run(
                &["aibox", "--agent", "claude", "profile", "list"],
                Vec::new(),
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("profile is shared across agents"));

        let err = fx
            .run(&["aibox", "--agent", "claude", "build"], Vec::new())
            .unwrap_err()
            .to_string();
        assert!(err.contains("build does not accept --agent"));

        let err = fx
            .run(
                &[
                    "aibox", "--agent", "claude", "config", "list", "--agent", "codex",
                ],
                Vec::new(),
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("--agent must be provided only once"));

        let err = fx
            .run(
                &[
                    "aibox", "--agent", "claude", "session", "delete", "abc", "--agent", "codex",
                ],
                Vec::new(),
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("--agent must be provided only once"));
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
