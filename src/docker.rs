//! Building and running the container.
//!
//! Two entry points: [`build_image`] (invoked by `aibox build`) and [`run`]
//! (spawn `docker run` for the agent). Both shell out to the `docker`
//! CLI via [`std::process::Command`].
//!
//! ## Why the Dockerfile comes from stdin
//!
//! The embedded Dockerfiles have no `COPY`; they fetch everything with
//! apt/curl/npm. So the build context is unused, and we feed the Dockerfile to
//! `docker build -f - <ctx>` on stdin with an empty context directory.

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Local image tag used when no image override is supplied.
pub const IMAGE: &str = "aibox:latest";

/// Shared development-runtime Dockerfile with both agent CLIs installed.
pub const DOCKERFILE: &str = include_str!("../assets/aibox.Dockerfile");

const CONTAINER_CREATE_WAIT: Duration = Duration::from_secs(1);
const CONTAINER_CREATE_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Cache policy for a Docker build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildCache {
    /// Keep Docker's cache enabled.
    Cached,
    /// Re-run every layer, but do not pull the `FROM` image.
    NoCache,
    /// Re-run every layer and pull a fresh `FROM` image.
    NoCachePull,
}

impl BuildCache {
    fn docker_args(self) -> &'static [&'static str] {
        match self {
            BuildCache::Cached => &[],
            BuildCache::NoCache => &["--no-cache"],
            BuildCache::NoCachePull => &["--no-cache", "--pull"],
        }
    }
}

/// Build `dockerfile` into `image` using `cache`.
///
/// The Dockerfile is piped in on stdin; the context is an empty temp dir since
/// no Dockerfile references it.
pub fn build_image(dockerfile: &str, image: &str, cache: BuildCache) -> Result<()> {
    let ctx = tempfile::tempdir().context("create empty build context")?;

    let mut cmd = Command::new("docker");
    cmd.arg("build");
    cmd.args(cache.docker_args());
    cmd.args(["-f", "-", "-t", image]);
    cmd.arg(ctx.path());
    cmd.stdin(Stdio::piped());

    let mut child = cmd
        .spawn()
        .context("spawn docker build (is docker installed?)")?;

    // Feed the Dockerfile, then drop stdin so docker sees EOF. If docker exited
    // early (bad flag, daemon down) the write fails with EPIPE — reap the child
    // first and report *its* status, which carries the real error; a broken-pipe
    // message would only mask it.
    let mut stdin = child.stdin.take().expect("stdin piped");
    let write_res = stdin.write_all(dockerfile.as_bytes());
    drop(stdin);

    let status = child.wait().context("wait for docker build")?;
    if !status.success() {
        bail!("docker build failed ({status})");
    }
    write_res.context("write Dockerfile to docker build stdin")?;
    Ok(())
}

/// True if an image reference exists locally.
pub fn image_exists(image: &str) -> Result<bool> {
    let inspect = Command::new("docker")
        .args(["image", "inspect", "--format", "{{.Id}}", image])
        .output()
        .context("inspect docker image (is docker installed?)")?;
    if inspect.status.success() {
        return Ok(true);
    }

    let list_ref = image_ref_for_exact_ls(image);
    let list = Command::new("docker")
        .args(["image", "ls", "--quiet", "--no-trunc", &list_ref])
        .output()
        .context("list docker image (is docker installed?)")?;
    if list.status.success() {
        return Ok(!String::from_utf8_lossy(&list.stdout).trim().is_empty());
    }

    let inspect_stderr = String::from_utf8_lossy(&inspect.stderr);
    let list_stderr = String::from_utf8_lossy(&list.stderr);
    bail!(
        "docker image inspect failed ({}): {}; docker image ls failed ({}): {}",
        inspect.status,
        inspect_stderr.trim(),
        list.status,
        list_stderr.trim()
    )
}

fn image_ref_for_exact_ls(image: &str) -> String {
    if image.contains('@') {
        return image.to_string();
    }
    let last = image.rsplit('/').next().unwrap_or(image);
    if last.contains(':') {
        image.to_string()
    } else {
        format!("{image}:latest")
    }
}

/// Run `docker run <args> <image> <cmd...>` as a child process and return its
/// exit code. A child (not `exec`) so the caller's credential cleanup still runs
/// after it returns. The child's pid and `--cidfile` are registered with `creds`
/// for the run's duration, so a SIGINT/SIGTERM aimed at the wrapper alone stops
/// the container instead of leaving it running unsupervised — killing just the
/// docker CLI is not enough when a TTY is attached (the CLI only proxies
/// signals without one; see `creds`).
pub fn run(
    run_args: &[String],
    image: &str,
    cmd: &[String],
    after_container_created: impl FnOnce(),
) -> Result<i32> {
    let mut after_container_created = Some(after_container_created);
    // Docker refuses to reuse an existing cidfile, so ask for a fresh path
    // inside a temp dir. The id it holds is not a secret; if a signal kills us
    // before the dir's cleanup, the leftover is harmless.
    let cid_dir = tempfile::tempdir().context("create cidfile dir")?;
    let cid_path = cid_dir.path().join("cid");

    // Register the cidfile *before* spawning: a signal landing between spawn
    // and registration could otherwise find neither a pid nor a container id,
    // leaving the container running unsupervised.
    crate::creds::set_cidfile(&cid_path)?;
    let spawned = Command::new("docker")
        .arg("run")
        .arg("--cidfile")
        .arg(&cid_path)
        .args(run_args)
        .arg(image)
        .args(cmd)
        .spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            crate::creds::clear_child();
            return Err(e).context("spawn docker run (is docker installed?)");
        }
    };

    crate::creds::set_child(child.id());
    let create = match wait_for_container_create(&mut child, &cid_path) {
        Ok(create) => create,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            let _ = crate::creds::finish_child();
            return Err(e);
        }
    };
    let waited: Result<ExitStatus> = match create {
        ContainerCreate::Created => {
            if let Some(callback) = after_container_created.take() {
                callback();
            }
            child.wait().map_err(anyhow::Error::from)
        }
        ContainerCreate::ChildExited(status) => Ok(status),
        ContainerCreate::TimedOut => {
            // If Docker is unusually slow to materialize the cidfile, keep any
            // pre-spawn mount-target locks until the daemon does record the
            // container id. If it never does, keep the conservative old behavior:
            // the locks stay held until the child exits.
            wait_with_delayed_container_create(&mut child, &cid_path, &mut after_container_created)
        }
    };
    let stopped_lingering_container = crate::creds::finish_child();
    let status = waited.context("wait for docker run")?;

    let code = exit_code(status);
    Ok(if stopped_lingering_container && code == 0 {
        1
    } else {
        code
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainerCreate {
    Created,
    ChildExited(ExitStatus),
    TimedOut,
}

fn wait_for_container_create(child: &mut Child, cid_path: &Path) -> Result<ContainerCreate> {
    let started = Instant::now();
    loop {
        if cidfile_has_id(cid_path) {
            return Ok(ContainerCreate::Created);
        }
        if let Some(status) = child
            .try_wait()
            .context("poll docker run before container create")?
        {
            return Ok(ContainerCreate::ChildExited(status));
        }
        if started.elapsed() >= CONTAINER_CREATE_WAIT {
            return Ok(ContainerCreate::TimedOut);
        }
        std::thread::sleep(CONTAINER_CREATE_POLL_INTERVAL);
    }
}

fn cidfile_has_id(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|cid| !cid.trim().is_empty())
}

fn wait_with_delayed_container_create<F: FnOnce()>(
    child: &mut Child,
    cid_path: &Path,
    after_container_created: &mut Option<F>,
) -> Result<ExitStatus> {
    loop {
        if cidfile_has_id(cid_path) {
            if let Some(callback) = after_container_created.take() {
                callback();
            }
            return child.wait().context("wait for docker run");
        }
        if let Some(status) = child
            .try_wait()
            .context("poll docker run after delayed container create")?
        {
            return Ok(status);
        }
        std::thread::sleep(CONTAINER_CREATE_POLL_INTERVAL);
    }
}

/// Map an exit status to a code: the child's own code when it exited, the
/// shell convention `128 + signal` when it was killed by a signal (so scripts
/// can tell "agent failed" from "interrupted"), else 1.
fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return 128 + sig;
        }
    }
    1
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::testutil::EnvGuard;
    use std::fs;
    use std::path::Path;

    fn write_fake_docker(dir: &Path) {
        crate::testutil::write_stub_script(
            dir,
            "docker",
            r#"#!/bin/sh
if [ "$1" = "image" ] && [ "$2" = "inspect" ]; then
    case "$AIBOX_FAKE_DOCKER_IMAGE_MODE" in
        exists)
            printf 'sha256:fake-image\n'
            exit 0
            ;;
        missing-localized)
            printf 'image not found: %s\n' "${5:-}" >&2
            exit 1
            ;;
        missing-empty)
            exit 1
            ;;
        list-exists-tagged|tagless-repository-match)
            exit 1
            ;;
        daemon-error)
            printf 'Cannot connect to the Docker daemon\n' >&2
            exit 1
            ;;
        *)
            exit 97
            ;;
    esac
fi
if [ "$1" = "image" ] && [ "$2" = "ls" ]; then
    case "$AIBOX_FAKE_DOCKER_IMAGE_MODE" in
        exists)
            printf 'sha256:fake-image\n'
            exit 0
            ;;
        missing-localized|missing-empty)
            exit 0
            ;;
        list-exists-tagged)
            if [ "${5:-}" = "repo/name:tag" ]; then
                printf 'sha256:fake-image\n'
            fi
            exit 0
            ;;
        tagless-repository-match)
            if [ "${5:-}" = "repo/name" ]; then
                printf 'sha256:fake-image\n'
            fi
            exit 0
            ;;
        daemon-error)
            printf 'Cannot connect to the Docker daemon\n' >&2
            exit 1
            ;;
        *)
            exit 96
            ;;
    esac
fi
if { [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering" ] || [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-stubborn" ] || [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-failure" ] || [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-kill-failure" ]; } && [ "$1" = "inspect" ]; then
    if [ -e "$AIBOX_FAKE_DOCKER_STOPPED" ]; then
        printf 'false\n'
    else
        printf 'true\n'
    fi
    exit 0
fi
if { [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering" ] || [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-stubborn" ] || [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-failure" ] || [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-kill-failure" ]; } && [ "$1" = "kill" ]; then
    printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
    if [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-stubborn" ] && [ "$2" = "--signal" ]; then
        exit 0
    fi
    if [ "$AIBOX_FAKE_DOCKER_MODE" = "lingering-kill-failure" ]; then
        exit 39
    fi
    : > "$AIBOX_FAKE_DOCKER_STOPPED"
    exit 0
fi
if [ "$1" != "run" ]; then
    exit 99
fi
if [ -n "$AIBOX_FAKE_DOCKER_RUN_LOG" ]; then
    printf 'run%s\n' "$(for a in "$@"; do printf ' <%s>' "$a"; done)" >> "$AIBOX_FAKE_DOCKER_RUN_LOG"
fi
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
case "$AIBOX_FAKE_DOCKER_MODE" in
    forwards-args)
        printf 'fake-container\n' > "$cid"
        exit 0
        ;;
    delayed-cid)
        sleep 0.2
        if [ -n "$AIBOX_CALLBACK_MARKER" ] && [ -e "$AIBOX_CALLBACK_MARKER" ]; then
            printf 'early\n' > "$AIBOX_EARLY_MARKER"
        fi
        printf 'fake-container\n' > "$cid"
        sleep 0.05
        exit 0
        ;;
    slow-cid)
        sleep 1.2
        if [ -n "$AIBOX_CALLBACK_MARKER" ] && [ -e "$AIBOX_CALLBACK_MARKER" ]; then
            printf 'early\n' > "$AIBOX_EARLY_MARKER"
        fi
        printf 'fake-container\n' > "$cid"
        sleep 0.05
        exit 0
        ;;
    no-cid)
        exit 23
        ;;
    slow-no-cid)
        sleep 1.2
        exit 24
        ;;
    lingering|lingering-stubborn|lingering-kill-failure)
        printf 'fake-container\n' > "$cid"
        exit 0
        ;;
    lingering-failure)
        printf 'fake-container\n' > "$cid"
        exit 47
        ;;
    *)
        exit 98
        ;;
esac
"#,
        );
    }

    fn write_fake_build_docker(dir: &Path) {
        crate::testutil::write_stub_script(
            dir,
            "docker",
            r#"#!/bin/sh
if [ "$1" != "build" ]; then
    exit 99
fi
if [ "$AIBOX_FAKE_DOCKER_BUILD_MODE" = "exit-early" ]; then
    exit 23
fi
log="$AIBOX_FAKE_DOCKER_BUILD_LOG"
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
cat >> "$log"
printf '\nEND\n' >> "$log"
"#,
        );
    }

    #[test]
    fn embedded_dockerfiles_do_not_require_build_context() {
        for line in DOCKERFILE.lines() {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let instruction = trimmed
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            assert_ne!(
                instruction, "COPY",
                "aibox Dockerfile must stay build-context-free: {line:?}"
            );
        }
    }

    #[test]
    fn build_image_uses_stdin_empty_context_and_cache_flags() {
        let _env_lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("docker-build.log");
        write_fake_build_docker(dir.path());
        let _path = EnvGuard::prepend_path(dir.path());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_BUILD_LOG", log.as_os_str());

        build_image("FROM scratch\n", "test/cached:latest", BuildCache::Cached).unwrap();
        build_image("RUN true\n", "test/nocache:latest", BuildCache::NoCache).unwrap();
        build_image("RUN false\n", "test/pull:latest", BuildCache::NoCachePull).unwrap();

        let log = fs::read_to_string(log).unwrap();
        assert!(
            log.contains("ARGS: <build> <-f> <-> <-t> <test/cached:latest> <"),
            "cached build should pass Dockerfile through stdin with only -f/-t: {log}"
        );
        assert!(
            log.contains("ARGS: <build> <--no-cache> <-f> <-> <-t> <test/nocache:latest> <"),
            "no-cache build should add only --no-cache: {log}"
        );
        assert!(
            log.contains("ARGS: <build> <--no-cache> <--pull> <-f> <-> <-t> <test/pull:latest> <"),
            "forced base build should add --no-cache and --pull: {log}"
        );
        assert!(log.contains("STDIN:FROM scratch\n"), "{log}");
        assert!(log.contains("STDIN:RUN true\n"), "{log}");
        assert!(log.contains("STDIN:RUN false\n"), "{log}");
    }

    #[test]
    fn build_image_reports_docker_status_when_child_exits_early() {
        let _env_lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("docker-build.log");
        write_fake_build_docker(dir.path());
        let _path = EnvGuard::prepend_path(dir.path());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_BUILD_LOG", log.as_os_str());
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_BUILD_MODE", "exit-early");

        let err = build_image("RUN true\n", "test/fails:latest", BuildCache::Cached)
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("docker build failed"),
            "docker status should be reported instead of masking it with stdin write errors: {err}"
        );
    }

    #[test]
    fn image_exists_uses_exact_image_inspect() {
        let _env_lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        write_fake_docker(dir.path());
        let _path = EnvGuard::prepend_path(dir.path());

        {
            let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "exists");
            assert!(image_exists("repo/name:tag").unwrap());
        }

        {
            let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "missing-localized");
            assert!(!image_exists("repo/name:tag").unwrap());
        }

        {
            let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "missing-empty");
            assert!(!image_exists("repo/name:tag").unwrap());
        }

        {
            let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "list-exists-tagged");
            assert!(image_exists("repo/name:tag").unwrap());
        }

        {
            let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "tagless-repository-match");
            assert!(
                !image_exists("repo/name").unwrap(),
                "tagless lookup must query repo/name:latest, not broad repo/name"
            );
        }

        {
            let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_IMAGE_MODE", "daemon-error");
            let err = image_exists("repo/name:tag").unwrap_err().to_string();
            assert!(err.contains("docker image inspect failed"), "{err}");
            assert!(err.contains("docker image ls failed"), "{err}");
            assert!(err.contains("Cannot connect"), "{err}");
        }
    }

    #[test]
    fn image_ref_for_exact_ls_adds_latest_only_when_tagless() {
        assert_eq!(image_ref_for_exact_ls("busybox"), "busybox:latest");
        assert_eq!(image_ref_for_exact_ls("repo/name"), "repo/name:latest");
        assert_eq!(
            image_ref_for_exact_ls("registry.example:5000/repo/name"),
            "registry.example:5000/repo/name:latest"
        );
        assert_eq!(image_ref_for_exact_ls("repo/name:dev"), "repo/name:dev");
        assert_eq!(
            image_ref_for_exact_ls("repo/name@sha256:abc"),
            "repo/name@sha256:abc"
        );
    }

    #[test]
    fn cidfile_has_id_requires_a_non_empty_container_id() {
        let dir = tempfile::tempdir().unwrap();
        let cid = dir.path().join("cid");

        assert!(!cidfile_has_id(&cid), "missing cidfile is not a create");
        fs::write(&cid, "").unwrap();
        assert!(!cidfile_has_id(&cid), "empty cidfile is not a create");
        fs::write(&cid, " \n\t").unwrap();
        assert!(
            !cidfile_has_id(&cid),
            "whitespace-only cidfile must not release spawn locks"
        );
        fs::write(&cid, "fake-container\n").unwrap();
        assert!(cidfile_has_id(&cid));
    }

    #[test]
    fn run_spawn_failure_does_not_call_container_created_callback() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = crate::creds::run_registry_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let callback_marker = dir.path().join("callback");
        let _path = EnvGuard::set("PATH", dir.path().as_os_str());

        let err = run(&[], "image:tag", &[], || {
            fs::write(&callback_marker, "called\n").unwrap();
        })
        .unwrap_err()
        .to_string();

        assert!(err.contains("spawn docker run"), "{err}");
        assert!(
            !callback_marker.exists(),
            "spawn failure means no container exists, so spawn locks must not be released"
        );
    }

    #[test]
    fn run_forwards_run_args_image_and_cmd_in_order() {
        // The sandbox boundary lives entirely in `run_args` (--cap-drop ALL,
        // --security-opt no-new-privileges, --user, every bind mount). `run`
        // must deliver them to `docker run` unchanged, with the image after the
        // run args and the command after the image. A dropped run_args or a
        // swapped image/cmd would silently strip the isolation the tool exists
        // to enforce, so assert the whole assembled line, not just --cidfile.
        let _env_lock = crate::test_env_lock();
        let _run_lock = crate::creds::run_registry_test_lock();
        let dir = tempfile::tempdir().unwrap();
        write_fake_docker(dir.path());
        let run_log = dir.path().join("run.log");
        let _path = EnvGuard::prepend_path(dir.path());
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "forwards-args");
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_RUN_LOG", run_log.as_os_str());

        let run_args = vec![
            "--cap-drop".to_string(),
            "ALL".to_string(),
            "--security-opt".to_string(),
            "no-new-privileges".to_string(),
        ];
        let cmd = vec!["exec".to_string(), "--flag".to_string()];
        let code = run(&run_args, IMAGE, &cmd, || {}).unwrap();

        assert_eq!(code, 0);
        let log = fs::read_to_string(&run_log).unwrap();
        // --cidfile is inserted first (before run_args) so its own tests keep
        // working; assert the rest follows it in order and the image sits
        // between the run args and the command.
        assert!(
            log.contains(
                "<--cap-drop> <ALL> <--security-opt> <no-new-privileges> \
                 <aibox:latest> <exec> <--flag>"
            ),
            "run must forward run_args, then image, then cmd, in order: {log}"
        );
    }

    #[test]
    fn run_callback_waits_until_cidfile_has_container_id() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = crate::creds::run_registry_test_lock();
        let dir = tempfile::tempdir().unwrap();
        write_fake_docker(dir.path());
        let callback_marker = dir.path().join("callback");
        let early_marker = dir.path().join("early");
        let _path = EnvGuard::prepend_path(dir.path());
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "delayed-cid");
        let _callback = EnvGuard::set("AIBOX_CALLBACK_MARKER", callback_marker.as_os_str());
        let _early = EnvGuard::set("AIBOX_EARLY_MARKER", early_marker.as_os_str());

        let code = run(&[], "image:tag", &[], || {
            fs::write(&callback_marker, "called\n").unwrap();
        })
        .unwrap();

        assert_eq!(code, 0);
        assert!(
            callback_marker.exists(),
            "callback runs after the cidfile is populated"
        );
        assert!(
            !early_marker.exists(),
            "callback must not run before Docker records a container id"
        );
    }

    #[test]
    fn run_callback_still_runs_when_cidfile_appears_after_initial_wait() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = crate::creds::run_registry_test_lock();
        let dir = tempfile::tempdir().unwrap();
        write_fake_docker(dir.path());
        let callback_marker = dir.path().join("callback");
        let early_marker = dir.path().join("early");
        let _path = EnvGuard::prepend_path(dir.path());
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "slow-cid");
        let _callback = EnvGuard::set("AIBOX_CALLBACK_MARKER", callback_marker.as_os_str());
        let _early = EnvGuard::set("AIBOX_EARLY_MARKER", early_marker.as_os_str());

        let code = run(&[], "image:tag", &[], || {
            fs::write(&callback_marker, "called\n").unwrap();
        })
        .unwrap();

        assert_eq!(code, 0);
        assert!(
            callback_marker.exists(),
            "callback runs once the delayed cidfile is populated"
        );
        assert!(
            !early_marker.exists(),
            "callback must not run before the delayed container id exists"
        );
    }

    #[test]
    fn run_does_not_call_callback_when_child_exits_before_cidfile() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = crate::creds::run_registry_test_lock();
        let dir = tempfile::tempdir().unwrap();
        write_fake_docker(dir.path());
        let callback_marker = dir.path().join("callback");
        let _path = EnvGuard::prepend_path(dir.path());
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "no-cid");
        let _callback = EnvGuard::set("AIBOX_CALLBACK_MARKER", callback_marker.as_os_str());

        let code = run(&[], "image:tag", &[], || {
            fs::write(&callback_marker, "called\n").unwrap();
        })
        .unwrap();

        assert_eq!(code, 23);
        assert!(
            !callback_marker.exists(),
            "no container id means mount-target locks stay held for drop cleanup"
        );
    }

    #[test]
    fn run_does_not_call_callback_when_delayed_child_exits_without_cidfile() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = crate::creds::run_registry_test_lock();
        let dir = tempfile::tempdir().unwrap();
        write_fake_docker(dir.path());
        let callback_marker = dir.path().join("callback");
        let _path = EnvGuard::prepend_path(dir.path());
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "slow-no-cid");
        let _callback = EnvGuard::set("AIBOX_CALLBACK_MARKER", callback_marker.as_os_str());

        let code = run(&[], "image:tag", &[], || {
            fs::write(&callback_marker, "called\n").unwrap();
        })
        .unwrap();

        assert_eq!(code, 24);
        assert!(
            !callback_marker.exists(),
            "callback must remain locked out even after the initial create wait timed out"
        );
    }

    #[test]
    fn run_immediately_kills_a_lingering_container_and_returns_nonzero() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = crate::creds::run_registry_test_lock();
        let dir = tempfile::tempdir().unwrap();
        write_fake_docker(dir.path());
        let stopped_marker = dir.path().join("stopped");
        let docker_log = dir.path().join("docker.log");
        let _path = EnvGuard::prepend_path(dir.path());
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "lingering-stubborn");
        let _stopped = EnvGuard::set("AIBOX_FAKE_DOCKER_STOPPED", stopped_marker.as_os_str());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", docker_log.as_os_str());

        let started = Instant::now();
        let code = run(&[], "image:tag", &[], || {}).unwrap();

        assert_eq!(
            code, 1,
            "a client-side zero exit is not a successful agent run when its container lingered"
        );
        assert!(stopped_marker.exists());
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "orphan cleanup must not wait through the ten-second graceful signal path"
        );
        let log = fs::read_to_string(docker_log).unwrap();
        assert!(log.contains("kill fake-container"), "{log}");
        assert!(!log.contains("--signal"), "{log}");
    }

    #[test]
    fn run_kills_a_lingering_container_even_when_client_failed() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = crate::creds::run_registry_test_lock();
        let dir = tempfile::tempdir().unwrap();
        write_fake_docker(dir.path());
        let stopped_marker = dir.path().join("stopped");
        let docker_log = dir.path().join("docker.log");
        let _path = EnvGuard::prepend_path(dir.path());
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "lingering-failure");
        let _stopped = EnvGuard::set("AIBOX_FAKE_DOCKER_STOPPED", stopped_marker.as_os_str());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", docker_log.as_os_str());

        let code = run(&[], "image:tag", &[], || {}).unwrap();

        assert_eq!(
            code, 47,
            "the Docker client's failure code must be preserved after orphan cleanup"
        );
        assert!(
            stopped_marker.exists(),
            "an orphan must be killed even when docker run itself exits non-zero"
        );
        let log = fs::read_to_string(docker_log).unwrap();
        assert!(log.contains("kill fake-container"), "{log}");
        assert!(!log.contains("--signal"), "{log}");
    }

    #[test]
    fn run_reports_failure_when_lingering_container_kill_fails() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = crate::creds::run_registry_test_lock();
        let dir = tempfile::tempdir().unwrap();
        write_fake_docker(dir.path());
        let stopped_marker = dir.path().join("stopped");
        let docker_log = dir.path().join("docker.log");
        let _path = EnvGuard::prepend_path(dir.path());
        let _mode = EnvGuard::set("AIBOX_FAKE_DOCKER_MODE", "lingering-kill-failure");
        let _stopped = EnvGuard::set("AIBOX_FAKE_DOCKER_STOPPED", stopped_marker.as_os_str());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", docker_log.as_os_str());

        let code = run(&[], "image:tag", &[], || {}).unwrap();

        assert_eq!(
            code, 1,
            "a zero-exit Docker client must not make an uncleaned orphan look successful"
        );
        assert!(
            !stopped_marker.exists(),
            "the fixture must prove the daemon-side kill failed"
        );
        let log = fs::read_to_string(docker_log).unwrap();
        assert!(log.contains("kill fake-container"), "{log}");
        assert!(!log.contains("--signal"), "{log}");
    }

    #[test]
    fn exit_code_maps_child_exit_and_signal_statuses() {
        let exited = Command::new("sh").args(["-c", "exit 37"]).status().unwrap();
        assert_eq!(exit_code(exited), 37);

        let signaled = Command::new("sh")
            .args(["-c", "kill -TERM $$"])
            .status()
            .unwrap();
        assert_eq!(exit_code(signaled), 128 + signal_hook::consts::SIGTERM);
    }
}
