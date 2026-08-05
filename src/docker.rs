//! Building and running cleanup-aware containers.
//!
//! Image inspection, [`build_image`] (invoked by `aibox build`), and [`run`]
//! (which spawns `docker run` for a Coding Agent or toolchain installer) all
//! shell out to the Docker CLI.
//!
//! ## Why the Dockerfile comes from stdin
//!
//! The embedded Dockerfile has no `COPY`; it fetches everything with
//! apt/curl/npm. So the build context is unused, and we feed the Dockerfile to
//! `docker build -f - <ctx>` on stdin with an empty context directory.

use anyhow::{bail, Context, Result};
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Local image tag used when no image override is supplied.
pub const IMAGE: &str = "aibox:latest";

/// Shared development-runtime Dockerfile with both Coding Agent CLIs installed.
pub const DOCKERFILE: &str = include_str!("../assets/aibox.Dockerfile");

const CONTAINER_CREATE_WAIT: Duration = Duration::from_secs(1);
const CONTAINER_CREATE_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Cache policy for a Docker build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildCache {
    /// Keep Docker's cache enabled.
    Cached,
    /// Re-run every layer, but do not pull the `FROM` image.
    #[cfg(test)]
    NoCache,
    /// Re-run every layer and pull a fresh `FROM` image.
    NoCachePull,
}

impl BuildCache {
    fn docker_args(self) -> &'static [&'static str] {
        match self {
            BuildCache::Cached => &[],
            #[cfg(test)]
            BuildCache::NoCache => &["--no-cache"],
            BuildCache::NoCachePull => &["--no-cache", "--pull"],
        }
    }
}

/// Build `dockerfile` into `image` using `cache`.
///
/// The Dockerfile is piped in on stdin with an empty temporary build context.
/// It must therefore be context-free and cannot use `COPY` or `ADD` for local
/// sources.
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

/// Whether an image reference exists locally.
///
/// A failed exact inspection is checked with an exact `docker image ls` query;
/// if Docker cannot complete either query, the daemon error is returned rather
/// than treating the image as absent.
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
/// exit code. A child (not `exec`) so the caller's container cleanup still runs
/// after it returns. The child's pid and `--cidfile` are registered with `creds`
/// for the run's duration, so a SIGINT/SIGTERM aimed at the wrapper alone stops
/// the container instead of leaving it running unsupervised — killing just the
/// docker CLI is not enough when a TTY is attached (the CLI only proxies
/// signals without one; see `creds`).
///
/// `after_container_created` runs at most once, after Docker has written a
/// container id and before this function waits for the container to exit. It is
/// not called if the Docker child exits before creating a container. If the
/// Docker child leaves a live or uninspectable container behind, aibox attempts
/// to kill it. A zero child exit becomes non-zero; an existing failure code is
/// preserved.
///
/// This function uses a process-wide child/cidfile registry and must not be
/// called concurrently.
pub fn run(
    run_args: &[String],
    image: &str,
    cmd: &[OsString],
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
    let child = match spawned {
        Ok(c) => c,
        Err(e) => {
            crate::creds::clear_child();
            return Err(e).context("spawn docker run (is docker installed?)");
        }
    };

    crate::creds::set_child(child.id());
    let mut registered_run = RegisteredRun::new(child);
    let create = wait_for_container_create(registered_run.child_mut(), &cid_path)?;
    let waited: Result<ExitStatus> = match create {
        ContainerCreate::Created => {
            if let Some(callback) = after_container_created.take() {
                callback();
            }
            registered_run
                .child_mut()
                .wait()
                .map_err(anyhow::Error::from)
        }
        ContainerCreate::ChildExited(status) => Ok(status),
        ContainerCreate::TimedOut => {
            // If Docker is slow to materialize the cidfile, defer the callback
            // until the daemon records a container id. If the child exits
            // without one, the callback must not run.
            wait_with_delayed_container_create(
                registered_run.child_mut(),
                &cid_path,
                &mut after_container_created,
            )
        }
    };
    let (status, stopped_lingering_container) = registered_run.finish_after_wait(waited)?;

    let code = exit_code(status);
    Ok(if stopped_lingering_container && code == 0 {
        1
    } else {
        code
    })
}

struct RegisteredRun {
    child: Child,
    finished: bool,
}

impl RegisteredRun {
    fn new(child: Child) -> Self {
        Self {
            child,
            finished: false,
        }
    }

    fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    fn finish(&mut self) -> bool {
        let stopped_lingering_container = crate::creds::finish_child();
        self.finished = true;
        stopped_lingering_container
    }

    fn finish_after_wait(&mut self, waited: Result<ExitStatus>) -> Result<(ExitStatus, bool)> {
        let status = waited.context("wait for docker run")?;
        Ok((status, self.finish()))
    }
}

impl Drop for RegisteredRun {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = crate::creds::finish_child();
    }
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
#[path = "docker_tests.rs"]
mod tests;
