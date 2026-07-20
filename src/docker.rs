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
use std::process::{Command, Stdio};

/// Local base image tag that agent Dockerfiles build FROM.
pub const BASE_IMAGE: &str = "aibox-base:latest";

/// Shared development-runtime Dockerfile.
pub const BASE_DOCKERFILE: &str = include_str!("../assets/base.Dockerfile");

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

/// True if an image with this tag exists locally (`docker image ls -q` prints
/// nothing for a missing tag).
pub fn image_exists(image: &str) -> Result<bool> {
    let output = Command::new("docker")
        .args(["image", "ls", "-q", image])
        .output()
        .context("query docker image list (is docker installed?)")?;
    if !output.status.success() {
        bail!("docker image lookup failed ({})", output.status);
    }
    Ok(!output.stdout.is_empty())
}

/// Run `docker run <args> <image> <cmd...>` as a child process and return its
/// exit code. A child (not `exec`) so the caller's credential cleanup still runs
/// after it returns. The child's pid and `--cidfile` are registered with `creds`
/// for the run's duration, so a SIGINT/SIGTERM aimed at the wrapper alone stops
/// the container instead of leaving it running unsupervised — killing just the
/// docker CLI is not enough when a TTY is attached (the CLI only proxies
/// signals without one; see `creds`).
pub fn run(run_args: &[String], image: &str, cmd: &[String]) -> Result<i32> {
    // Docker refuses to reuse an existing cidfile, so ask for a fresh path
    // inside a temp dir. The id it holds is not a secret; if a signal kills us
    // before the dir's cleanup, the leftover is harmless.
    let cid_dir = tempfile::tempdir().context("create cidfile dir")?;
    let cid_path = cid_dir.path().join("cid");

    // Register the cidfile *before* spawning: a signal landing between spawn
    // and registration could otherwise find neither a pid nor a container id,
    // leaving the container running unsupervised.
    crate::creds::set_cidfile(&cid_path);
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
    let waited = child.wait();
    crate::creds::clear_child();
    let status = waited.context("wait for docker run")?;

    Ok(exit_code(status))
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
