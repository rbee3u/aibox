//! aibox — run coding agents inside a Docker container that is the sandbox
//! boundary, with host-side provider configuration management.
//!
//! The binary pre-splits pass-through arguments at `--`, parses the left side
//! into [`cli::Cli`], and calls [`run_os`]. Provider and session operations stay
//! on the host; only an agent run starts Docker. Runs consume previously
//! applied active agent files and never mount provider snapshots.
//!
//! Most users should use the `aibox` binary. The library exposes the same
//! orchestration components so command assembly and host-side operations can be
//! tested or embedded without invoking the binary entry point.

#![warn(missing_docs)]

pub mod agent;
pub mod cli;
mod completion;
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
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

use agent::AgentKind;
use anyhow::{Context, Result};
use cli::{BuildArgs, Cli, Command, RunArgs, SessionArgs};
use docker::BuildCache;
use profile::Profile;
use std::ffi::OsString;

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

/// Handle an environment-activated shell completion request before normal
/// argument splitting and parsing.
///
/// This returns immediately for ordinary invocations and exits the process
/// after writing completion output for requests made by a generated shell
/// registration script.
pub fn handle_completion() {
    completion::handle_env();
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
        .any(|character| character.is_ascii_control() || character.is_ascii_whitespace())
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

pub(crate) fn print_text(text: &str) -> Result<bool> {
    write_text(&mut std::io::stdout().lock(), text)
}

fn write_line(out: &mut impl std::io::Write, line: &str) -> Result<bool> {
    if !write_text(out, line)? {
        return Ok(false);
    }
    match out.write_all(b"\n") {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(e) => Err(e).context("write to stdout"),
    }
}

fn write_text(out: &mut impl std::io::Write, text: &str) -> Result<bool> {
    match out.write_all(text.as_bytes()) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(false),
        Err(e) => Err(e).context("write to stdout"),
    }
}

/// Execute one parsed aibox command with UTF-8 agent pass-through arguments.
///
/// Use [`run_os`] when arguments collected from the operating system must be
/// forwarded without requiring UTF-8.
pub fn run(cli: Cli, passthrough: Vec<String>) -> Result<i32> {
    run_os(cli, passthrough.into_iter().map(OsString::from).collect())
}

/// Execute one parsed aibox command, preserving opaque operating-system
/// strings after the pass-through boundary.
///
/// `passthrough` must contain only the arguments after the first `--`; they are
/// forwarded unchanged for an agent run and rejected for subcommands. The
/// returned value is the process exit code to expose to the caller.
pub fn run_os(cli: Cli, passthrough: Vec<OsString>) -> Result<i32> {
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
            reject_passthrough("build takes no pass-through args", &passthrough)?;
            if root_agent.is_some() {
                anyhow::bail!("build does not accept --agent");
            }
            reject_command_run_options("build", &run_args)?;
            run_build(&args)
        }
        Some(Command::Completion(args)) => {
            reject_passthrough("completion takes no pass-through args", &passthrough)?;
            if root_agent.is_some() {
                anyhow::bail!("completion does not accept --agent");
            }
            reject_command_run_options("completion", &run_args)?;
            completion::dispatch(&args)
        }
        Some(Command::Profile(args)) => {
            reject_passthrough("profile takes no pass-through args", &passthrough)?;
            if root_agent.is_some() {
                anyhow::bail!("profile is shared across agents and does not accept --agent");
            }
            reject_command_run_options("profile", &run_args)?;
            profile::dispatch(&args.command)
        }
        Some(Command::Config(args)) => {
            if root_agent.is_some() {
                anyhow::bail!("config does not accept root --agent");
            }
            reject_run_only_options(&run_args)?;
            let agent = args.agent.unwrap_or(AgentKind::Codex);
            run_config_command(agent, args.profile_name(), &args.command, &passthrough)
        }
        Some(Command::Session(args)) => {
            if root_agent.is_some() {
                anyhow::bail!("session does not accept root --agent");
            }
            reject_run_only_options(&run_args)?;
            let agent = args.agent.unwrap_or(AgentKind::Codex);
            run_session_command(agent, args.profile_name(), &args, &passthrough)
        }
    }
}

fn run_config_command(
    agent: AgentKind,
    profile_name: &str,
    command: &cli::ConfigCommand,
    passthrough: &[OsString],
) -> Result<i32> {
    reject_passthrough("config/session take no pass-through args", passthrough)?;
    let root = profile::config_root()?;
    let prof = Profile::resolve(agent, &root, profile_name)?;
    config::dispatch(agent, &prof, command)
}

fn run_session_command(
    agent: AgentKind,
    profile_name: &str,
    args: &SessionArgs,
    passthrough: &[OsString],
) -> Result<i32> {
    reject_passthrough("config/session take no pass-through args", passthrough)?;
    let root = profile::config_root()?;
    let prof = Profile::resolve(agent, &root, profile_name)?;
    prof.validate_session_home()?;
    match args.command.as_ref() {
        None | Some(cli::SessionCommand::List) => {
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

fn reject_passthrough(restriction: &str, passthrough: &[OsString]) -> Result<()> {
    if !passthrough.is_empty() {
        anyhow::bail!("`-- <args>` applies only to a run; {restriction}");
    }
    Ok(())
}

fn reject_run_only_options(run: &RunArgs) -> Result<()> {
    let used = used_run_only_options(run);
    if !used.is_empty() {
        anyhow::bail!(
            "config/session do not accept run-only options: {}",
            used.join(", ")
        );
    }
    Ok(())
}

fn reject_command_run_options(command: &str, run: &RunArgs) -> Result<()> {
    let used = used_command_run_options(run);
    if !used.is_empty() {
        anyhow::bail!("{command} does not accept run options: {}", used.join(", "));
    }
    Ok(())
}

fn used_command_run_options(run: &RunArgs) -> Vec<&'static str> {
    let mut used = Vec::new();
    if run.profile.is_some() {
        used.push("--profile");
    }
    used.extend(used_run_only_options(run));
    used
}

fn used_run_only_options(run: &RunArgs) -> Vec<&'static str> {
    let mut used = Vec::new();
    if run.work.is_some() {
        used.push("--work");
    }
    if !run.mount.is_empty() {
        used.push("--mount");
    }
    used
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

fn run_agent(agent: AgentKind, run: &RunArgs, passthrough: &[OsString]) -> Result<i32> {
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
    runspec::validate_aibox_mount_sources(&work_dir, &mounts, &root)?;

    if !docker::image_exists(&image)? {
        anyhow::bail!("{image} is not present locally; build it first with `aibox build`");
    }

    let _profile_lock = prof.prepare_for_run()?;
    prof.validate_locked_run_paths()?;
    let home_dir = std::fs::canonicalize(&prof.home_dir)
        .with_context(|| format!("resolve profile home {}", prof.home_dir.display()))?;
    runspec::reject_colon_in_bind_source("profile home", &home_dir)?;

    let invocation = agent.build_invocation(passthrough);

    let run_args = runspec::assemble_run_args(
        agent,
        &work_dir,
        &home_dir,
        &mounts,
        &invocation.extra_run_args,
    );

    docker::run(&run_args, &image, &invocation.agent_cmd, || {})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::EnvGuard;

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
    if [ "$AIBOX_FAKE_DOCKER_IMAGE_MODE" = "missing" ]; then
        exit 1
    fi
    printf 'sha256:fake-image\n'
    exit 0
fi

if [ "$1" = "image" ] && [ "$2" = "ls" ]; then
    if [ "$AIBOX_FAKE_DOCKER_IMAGE_MODE" = "missing" ]; then
        exit 0
    fi
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
    fn invalid_image_override_is_rejected_before_docker_lookup() {
        for (image, expected) in [
            ("", "AIBOX_IMAGE is set but empty"),
            ("bad image", "whitespace/control"),
            ("--bad", "must not start"),
        ] {
            let fx = RunFixture::new();
            let _image = EnvGuard::set("AIBOX_IMAGE", image);

            let err = fx.run(&["aibox"], Vec::new()).unwrap_err().to_string();

            assert!(err.contains(expected), "{image:?}: {err}");
            assert_eq!(
                fx.log(),
                "",
                "{image:?}: an invalid image override should fail before docker is consulted"
            );
            assert!(
                !fx.root.path().join("default").exists(),
                "{image:?}: a bad environment override must not initialize a profile"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_image_override_is_rejected_before_docker_lookup() {
        use std::os::unix::ffi::OsStringExt;

        let fx = RunFixture::new();
        let image = OsString::from_vec(vec![b'a', b'i', b'b', b'o', b'x', 0xff]);
        let _image = EnvGuard::set("AIBOX_IMAGE", image);

        let err = fx.run(&["aibox"], Vec::new()).unwrap_err().to_string();

        assert!(err.contains("AIBOX_IMAGE is not valid UTF-8"), "{err}");
        assert_eq!(
            fx.log(),
            "",
            "an unrepresentable image name must fail before docker is consulted"
        );
        assert!(!fx.root.path().join("default").exists());
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
            fx.root.path().join("default/home").display()
        )));
        assert!(log.contains("<aibox:latest> <codex>"), "{log}");
        assert!(
            !log.contains("<--dangerously-bypass-approvals-and-sandbox>"),
            "{log}"
        );
        assert!(fx.root.path().join("default/home/.codex").is_dir());
        assert!(fx
            .root
            .path()
            .join("default/home/.claude/statusline.sh")
            .is_file());
        assert!(fx.root.path().join("default/home/.gitconfig").is_file());
        assert!(fx.root.path().join("default/config/codex").is_dir());
        assert!(fx.root.path().join("default/config/claude").is_dir());
        assert!(!fx.root.path().join("default/tracing").exists());
        assert!(!log.contains("<--env-file>"), "{log}");
        assert!(!log.contains("<-c>"), "{log}");
    }

    #[cfg(unix)]
    #[test]
    fn run_resolves_a_symlinked_aibox_root_before_mounting_profile_home() {
        use std::os::unix::fs::symlink;

        let fx = RunFixture::new();
        let parent_link = fx.root.path().join("parent-link");
        let real_parent = fx.root.path().join("real-parent");
        let real_root = real_parent.join("aibox-root");
        std::fs::create_dir(&real_parent).unwrap();
        std::fs::create_dir(&real_root).unwrap();
        symlink(&real_parent, &parent_link).unwrap();
        let configured_root = parent_link.join("aibox-root");
        let _root = EnvGuard::set("AIBOX_ROOT", configured_root.as_os_str());

        let code = fx.run(&["aibox"], Vec::new()).unwrap();

        assert_eq!(code, 0);
        let log = fx.log();
        assert!(
            log.contains(&format!(
                "<{}:/home/aibox>",
                real_root.join("default/home").display()
            )),
            "Docker must receive a resolved bind source: {log}"
        );
        assert!(
            !log.contains(&configured_root.display().to_string()),
            "the symlinked bind source must not reach Docker: {log}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_uses_a_valid_image_override_for_lookup_and_launch() {
        let fx = RunFixture::new();
        let _image = EnvGuard::set("AIBOX_IMAGE", "registry.example/aibox:test");

        let code = fx.run(&["aibox"], Vec::new()).unwrap();

        assert_eq!(code, 0);
        let log = fx.log();
        assert!(
            log.contains("<image> <inspect> <--format> <{{.Id}}> <registry.example/aibox:test>"),
            "{log}"
        );
        assert!(
            log.contains("<registry.example/aibox:test> <codex>"),
            "the validated override must also be the launched image: {log}"
        );
        assert!(!log.contains("<aibox:latest> <codex>"), "{log}");
    }

    #[cfg(unix)]
    #[test]
    fn run_preserves_applied_config_without_remounting_or_reapplying_provider_data() {
        let fx = RunFixture::new();
        let profile = Profile::resolve(AgentKind::Codex, fx.root.path(), "default").unwrap();
        config::create_provider(&profile, "openai").unwrap();
        std::fs::write(
            profile.provider_file("openai", "config.toml"),
            "model = \"provider\"\n",
        )
        .unwrap();
        std::fs::write(
            profile.provider_file("openai", "auth.json"),
            r#"{"token":"provider"}"#,
        )
        .unwrap();
        config::apply_provider(&profile, "openai").unwrap();

        let active_config = "model = \"locally-adjusted\"\n";
        let active_auth = r#"{"token":"locally-adjusted"}"#;
        std::fs::write(profile.active_file("config.toml"), active_config).unwrap();
        std::fs::write(profile.active_file("auth.json"), active_auth).unwrap();
        let backups_before = std::fs::read_dir(profile.backups_dir()).unwrap().count();

        let code = fx.run(&["aibox"], Vec::new()).unwrap();

        assert_eq!(code, 0);
        assert_eq!(
            std::fs::read_to_string(profile.active_file("config.toml")).unwrap(),
            active_config,
            "a run must consume the persisted active config without reapplying the last provider"
        );
        assert_eq!(
            std::fs::read_to_string(profile.active_file("auth.json")).unwrap(),
            active_auth,
            "a run must not replace persisted auth from provider metadata"
        );
        assert_eq!(
            std::fs::read_dir(profile.backups_dir()).unwrap().count(),
            backups_before,
            "a run must not perform an implicit config apply or backup"
        );
        let log = fx.log();
        assert!(
            !log.contains(&profile.provider_root_dir().display().to_string()),
            "provider metadata must stay host-only and never enter docker arguments: {log}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn codex_exec_subcommand_can_be_passed_through() {
        let fx = RunFixture::new();

        let code = fx
            .run(
                &["aibox"],
                vec![
                    "exec".to_string(),
                    "fix tests".to_string(),
                    "--json".to_string(),
                ],
            )
            .unwrap();

        assert_eq!(code, 0);
        let log = fx.log();
        assert!(
            log.contains("<aibox:latest> <codex> <exec> <fix tests> <--json>"),
            "{log}"
        );
        assert!(
            !log.contains("<--dangerously-bypass-approvals-and-sandbox>"),
            "{log}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_os_preserves_non_utf8_agent_arguments_through_docker_spawn() {
        use std::os::unix::ffi::OsStringExt;

        let fx = RunFixture::new();
        let opaque = OsString::from_vec(vec![b'f', 0x80, b'o']);
        let cli = Cli::try_parse_from(["aibox"]).unwrap();

        let code = run_os(cli, vec![opaque.clone()]).unwrap();

        assert_eq!(code, 0);
        let log = std::fs::read(&fx.docker_log).unwrap();
        assert!(
            log.windows(opaque.as_encoded_bytes().len())
                .any(|window| window == opaque.as_encoded_bytes()),
            "the opaque pass-through argument must reach the docker child unchanged"
        );
    }

    #[cfg(unix)]
    #[test]
    fn claude_run_seeds_statusline_but_not_settings() {
        let fx = RunFixture::new();
        fx.run(&["aibox", "--agent", "claude"], Vec::new()).unwrap();

        let log = fx.log();
        assert!(log.contains(&format!(
            "<{}:/home/aibox>",
            fx.root.path().join("default/home").display()
        )));
        assert!(log.contains("<aibox:latest> <claude>"), "{log}");
        assert!(!log.contains("<--dangerously-skip-permissions>"), "{log}");
        assert!(fx
            .root
            .path()
            .join("default/home/.claude/statusline.sh")
            .exists());
        assert!(fx.root.path().join("default/home/.codex").is_dir());
        assert!(fx.root.path().join("default/home/.gitconfig").is_file());
        assert!(fx.root.path().join("default/config/codex").is_dir());
        assert!(fx.root.path().join("default/config/claude").is_dir());
        assert!(!fx
            .root
            .path()
            .join("default/home/.claude/settings.json")
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn host_profile_is_rejected_for_run_but_allowed_for_session() {
        let fx = RunFixture::new();
        assert!(Cli::try_parse_from(["aibox", "-p", "host"]).is_err());

        let code = fx
            .run(&["aibox", "session", "-p", "host"], Vec::new())
            .unwrap();
        assert_eq!(code, 0);
    }

    #[cfg(unix)]
    #[test]
    fn config_and_session_reject_passthrough() {
        let fx = RunFixture::new();
        for argv in [
            &["aibox", "config", "list"][..],
            &["aibox", "session", "list"][..],
        ] {
            let err = fx
                .run(argv, vec!["ignored".to_string()])
                .unwrap_err()
                .to_string();
            assert!(err.contains("applies only to a run"), "{argv:?}: {err}");
        }
        assert_eq!(
            fx.log(),
            "",
            "rejected management commands must not consult Docker"
        );
    }

    #[cfg(unix)]
    #[test]
    fn config_command_agent_selects_the_provider_management_tree() {
        let fx = RunFixture::new();

        let code = fx
            .run(
                &[
                    "aibox",
                    "config",
                    "--agent",
                    "claude",
                    "create",
                    "anthropic",
                ],
                Vec::new(),
            )
            .unwrap();

        assert_eq!(code, 0);
        assert!(fx
            .root
            .path()
            .join("default/config/claude/anthropic/settings.json")
            .is_file());
        assert!(
            !fx.root
                .path()
                .join("default/config/codex/anthropic")
                .exists(),
            "a command-level --agent claude must not create a Codex provider"
        );
    }

    #[cfg(unix)]
    #[test]
    fn profile_commands_create_and_delete_without_starting_docker() {
        let fx = RunFixture::new();

        let code = fx
            .run(&["aibox", "profile", "create", "work"], Vec::new())
            .unwrap();

        assert_eq!(code, 0);
        assert!(fx.root.path().join("work/home/.codex").is_dir());
        assert!(fx.root.path().join("work/home/.claude").is_dir());

        let code = fx
            .run(&["aibox", "profile", "delete", "work", "--yes"], Vec::new())
            .unwrap();

        assert_eq!(code, 0);
        assert!(!fx.root.path().join("work").exists());
        assert_eq!(
            fx.log(),
            "",
            "host-side profile management must never invoke Docker"
        );
    }

    #[cfg(unix)]
    #[test]
    fn config_apply_and_delete_route_to_the_selected_profile_without_docker() {
        let fx = RunFixture::new();

        fx.run(
            &["aibox", "config", "--profile", "work", "create", "openai"],
            Vec::new(),
        )
        .unwrap();
        let selected = Profile::resolve(AgentKind::Codex, fx.root.path(), "work").unwrap();
        std::fs::write(
            selected.provider_file("openai", "config.toml"),
            "model = \"selected-profile\"\n",
        )
        .unwrap();
        std::fs::write(
            selected.provider_file("openai", "auth.json"),
            r#"{"token":"selected-profile"}"#,
        )
        .unwrap();

        let code = fx
            .run(
                &["aibox", "config", "apply", "openai", "--profile", "work"],
                Vec::new(),
            )
            .unwrap();

        assert_eq!(code, 0);
        assert_eq!(
            std::fs::read_to_string(selected.active_file("config.toml")).unwrap(),
            "model = \"selected-profile\"\n"
        );
        assert!(
            !fx.root
                .path()
                .join("default/home/.codex/config.toml")
                .exists(),
            "a scoped config command must not fall back to the default profile"
        );

        let code = fx
            .run(
                &[
                    "aibox",
                    "config",
                    "--profile=work",
                    "delete",
                    "openai",
                    "--yes",
                ],
                Vec::new(),
            )
            .unwrap();

        assert_eq!(code, 0);
        assert!(!selected.provider_dir("openai").exists());
        assert!(
            selected.active_file("config.toml").exists(),
            "deleting provider metadata must not roll back persisted active config"
        );
        assert_eq!(
            fx.log(),
            "",
            "host-side config management must never invoke Docker"
        );
    }

    #[cfg(unix)]
    #[test]
    fn session_delete_routes_to_the_selected_profile_without_docker() {
        let fx = RunFixture::new();
        profile::create_ordinary_profile(fx.root.path(), "work").unwrap();
        let id = "11111111-2222-3333-4444-555555555555";
        let transcript = crate::testutil::write_jsonl(
            fx.root.path(),
            &format!("work/home/.codex/sessions/2026/07/30/rollout-test-{id}.jsonl"),
            &[r#"{"timestamp":"2026-07-30T10:00:00Z","type":"session_meta"}"#],
        );

        let code = fx
            .run(
                &[
                    "aibox",
                    "session",
                    "delete",
                    id,
                    "--yes",
                    "--profile",
                    "work",
                ],
                Vec::new(),
            )
            .unwrap();

        assert_eq!(code, 0);
        assert!(
            !transcript.exists(),
            "the selected profile's transcript should be deleted"
        );
        assert_eq!(
            fx.log(),
            "",
            "host-side session management must never invoke Docker"
        );
    }

    #[cfg(unix)]
    #[test]
    fn agent_flag_conflicts_are_rejected_across_command_levels() {
        let fx = RunFixture::new();

        for argv in [
            &[
                "aibox", "--agent", "claude", "config", "--agent", "codex", "list",
            ][..],
            &[
                "aibox", "config", "--agent", "claude", "list", "--agent", "codex",
            ][..],
            &["aibox", "--agent", "claude", "session", "--agent", "codex"][..],
            &[
                "aibox", "session", "--agent", "claude", "list", "--agent", "codex",
            ][..],
        ] {
            assert!(
                Cli::try_parse_from(argv).is_err(),
                "{argv:?} should reject conflicting agent selectors"
            );
        }
        assert_eq!(
            fx.log(),
            "",
            "agent-selector errors should be resolved before docker is consulted"
        );
    }

    #[cfg(unix)]
    #[test]
    fn build_profile_and_completion_reject_passthrough_and_run_options() {
        let fx = RunFixture::new();

        let err = fx
            .run(&["aibox", "build"], vec!["ignored".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("build takes no pass-through args"), "{err}");

        assert!(Cli::try_parse_from(["aibox", "-p", "work", "build"]).is_err());

        let err = fx
            .run(&["aibox", "profile", "list"], vec!["ignored".to_string()])
            .unwrap_err()
            .to_string();
        assert!(err.contains("profile takes no pass-through args"), "{err}");

        assert!(Cli::try_parse_from(["aibox", "-m", "src:/src", "profile", "list"]).is_err());

        let err = fx
            .run(&["aibox", "completion", "zsh"], vec!["ignored".to_string()])
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("completion takes no pass-through args"),
            "{err}"
        );

        assert!(Cli::try_parse_from(["aibox", "--work", ".", "completion", "zsh"]).is_err());
        assert_eq!(
            fx.log(),
            "",
            "command-surface errors should be resolved before docker is consulted"
        );
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
    fn missing_image_does_not_initialize_profile_or_run_container() {
        let fx = RunFixture::new();
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "missing");

        let err = fx.run(&["aibox"], Vec::new()).unwrap_err().to_string();

        assert!(err.contains("not present locally"), "{err}");
        assert!(
            !fx.root.path().join("default").exists(),
            "a missing image must fail before profile initialization"
        );
        let log = fx.log();
        assert!(!log.contains("ARGS: <run>"), "{log}");
    }

    #[cfg(unix)]
    #[test]
    fn run_rejects_work_dir_that_would_expose_aibox_internal_tree() {
        let fx = RunFixture::new();
        let work = fx.root.path().to_str().unwrap();
        let err = fx
            .run(&["aibox", "-w", work], Vec::new())
            .unwrap_err()
            .to_string();

        assert!(err.contains("aibox internal data"), "{err}");
        assert!(!fx.root.path().join("default").exists());
    }

    #[cfg(unix)]
    #[test]
    fn run_rejects_mount_that_would_expose_profile_config() {
        let fx = RunFixture::new();
        let management = fx.root.path().join("default/config");
        std::fs::create_dir_all(management.join("codex")).unwrap();
        let mount = format!("{}:/secrets:ro", management.display());

        let err = fx
            .run(&["aibox", "-m", &mount], Vec::new())
            .unwrap_err()
            .to_string();

        assert!(err.contains("aibox internal data"), "{err}");
        assert!(!fx.root.path().join("default/home").exists());
        assert_eq!(
            fx.log(),
            "",
            "management mount validation should fail before docker is consulted"
        );
    }

    #[cfg(unix)]
    #[test]
    fn root_agent_flag_cannot_cross_command_boundaries() {
        assert!(Cli::try_parse_from(["aibox", "--agent", "claude", "profile", "list"]).is_err());

        assert!(Cli::try_parse_from(["aibox", "--agent", "claude", "build"]).is_err());

        assert!(Cli::try_parse_from([
            "aibox", "--agent", "claude", "config", "list", "--agent", "codex",
        ])
        .is_err());

        assert!(Cli::try_parse_from([
            "aibox", "--agent", "claude", "session", "delete", "abc", "--agent", "codex",
        ])
        .is_err());
    }

    #[test]
    fn output_writes_treat_broken_pipes_as_clean_stops_but_report_other_errors() {
        struct AlwaysBroken;
        impl std::io::Write for AlwaysBroken {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        struct BrokenOnNewline {
            writes: usize,
        }
        impl std::io::Write for BrokenOnNewline {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.writes += 1;
                if self.writes == 1 {
                    Ok(buf.len())
                } else {
                    Err(std::io::ErrorKind::BrokenPipe.into())
                }
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        struct PermissionDenied;
        impl std::io::Write for PermissionDenied {
            fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
                Err(std::io::ErrorKind::PermissionDenied.into())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        assert!(!write_line(&mut AlwaysBroken, "x").unwrap());
        assert!(!write_text(&mut AlwaysBroken, "x").unwrap());
        assert!(
            !write_line(&mut BrokenOnNewline { writes: 0 }, "x").unwrap(),
            "a reader may hang up after the line body but before its delimiter"
        );
        let err = write_text(&mut PermissionDenied, "x")
            .unwrap_err()
            .to_string();
        assert!(err.contains("write to stdout"), "{err}");
    }
}
