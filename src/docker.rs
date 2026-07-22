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
//! `docker build -f - <ctx>` on stdin with an empty context directory. The
//! agent images build `FROM aibox-base:latest`, which is also built from an
//! embedded Dockerfile first.

use anyhow::{bail, Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Local base image tag that agent Dockerfiles build FROM.
pub const BASE_IMAGE: &str = "aibox-base:latest";

/// Shared development-runtime Dockerfile.
pub const BASE_DOCKERFILE: &str = include_str!("../assets/base.Dockerfile");

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
            crate::creds::finish_child();
            return Err(e);
        }
    };
    let status = match create {
        ContainerCreate::Created => {
            if let Some(callback) = after_container_created.take() {
                callback();
            }
            let waited = child.wait();
            crate::creds::finish_child();
            waited.context("wait for docker run")?
        }
        ContainerCreate::ChildExited(status) => {
            crate::creds::finish_child();
            status
        }
        ContainerCreate::TimedOut => {
            // If Docker is unusually slow to materialize the cidfile, keep any
            // pre-spawn mount-target locks until the daemon does record the
            // container id. If it never does, keep the conservative old behavior:
            // the locks stay held until the child exits.
            let waited = wait_with_delayed_container_create(
                &mut child,
                &cid_path,
                &mut after_container_created,
            );
            crate::creds::finish_child();
            waited.context("wait for docker run")?
        }
    };

    Ok(exit_code(status))
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
fn exit_code(status: std::process::ExitStatus) -> i32 {
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
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    struct EnvGuard {
        name: &'static str,
        old: Option<OsString>,
    }

    impl EnvGuard {
        fn set(name: &'static str, value: impl Into<OsString>) -> Self {
            let old = std::env::var_os(name);
            std::env::set_var(name, value.into());
            EnvGuard { name, old }
        }

        fn prepend_path(dir: &Path) -> Self {
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

    fn write_fake_docker(dir: &Path) {
        let path = dir.join("docker");
        fs::write(
            &path,
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
if [ "$1" != "run" ]; then
    exit 99
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
    *)
        exit 98
        ;;
esac
"#,
        )
        .unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
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
}
