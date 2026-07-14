//! Building and running the container.
//!
//! Two entry points: [`build_image`] (invoked by `--build`, or automatically
//! when the image is missing) and [`run`] (assemble `docker run` and exec the
//! agent). Both shell out to the `docker` CLI via [`std::process::Command`].
//!
//! ## Why the Dockerfile comes from stdin
//!
//! Neither Dockerfile has a `COPY`; they fetch everything with apt/curl/npm. So
//! the build context is unused, and we feed the embedded Dockerfile
//! (`AgentKind::dockerfile`) to `docker build -f - <ctx>` on stdin with an empty
//! context directory. This is what lets the Bash `readlink` self-location dance
//! (needed only to find the Dockerfile beside the script) disappear entirely.

use crate::agent::AgentKind;
use anyhow::{bail, Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

/// Build the image for `agent` into `image`. `fresh` maps to the Bash `--build`
/// path: `--no-cache --pull` so the Node/Go/Rust/agent "latest" layers actually
/// re-resolve instead of reusing frozen cached versions. `fresh = false` is the
/// auto-build-when-missing path (cached, fast).
///
/// The Dockerfile is piped in on stdin; the context is an empty temp dir since
/// no Dockerfile references it.
pub fn build_image(agent: AgentKind, image: &str, fresh: bool) -> Result<()> {
    let ctx = tempfile::tempdir().context("create empty build context")?;

    let mut cmd = Command::new("docker");
    cmd.arg("build");
    if fresh {
        cmd.args(["--no-cache", "--pull"]);
    }
    cmd.args(["-f", "-", "-t", image]);
    cmd.arg(ctx.path());
    cmd.stdin(Stdio::piped());

    let mut child = cmd
        .spawn()
        .context("spawn docker build (is docker installed?)")?;
    child
        .stdin
        .take()
        .expect("stdin piped")
        .write_all(agent.dockerfile().as_bytes())
        .context("write Dockerfile to docker build stdin")?;

    let status = child.wait().context("wait for docker build")?;
    if !status.success() {
        bail!("docker build failed ({status})");
    }
    Ok(())
}

/// True if an image with this tag exists locally. Mirrors the Bash
/// `docker image ls -q "$IMAGE"` emptiness check.
pub fn image_exists(image: &str) -> bool {
    Command::new("docker")
        .args(["image", "ls", "-q", image])
        .output()
        .map(|o| o.status.success() && !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Run `docker run <args> <image> <cmd...>` as a child process and return its
/// exit code. A child (not `exec`) so the caller's credential cleanup still runs
/// after it returns — the Rust equivalent of the Bash "can't `exec docker`" rule,
/// except here it falls out naturally from the process model.
pub fn run(run_args: &[String], image: &str, cmd: &[String]) -> Result<i32> {
    let status = Command::new("docker")
        .arg("run")
        .args(run_args)
        .arg(image)
        .args(cmd)
        .status()
        .context("spawn docker run (is docker installed?)")?;
    Ok(status.code().unwrap_or(1))
}
