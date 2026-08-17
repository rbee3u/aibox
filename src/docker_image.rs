//! Docker image construction and exact local-image inspection.

use super::DockerCli;
use anyhow::{Context, Result, bail};
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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
    build_image_with(&DockerCli::system(), dockerfile, image, cache)
}

pub(crate) fn build_image_with(
    docker: &DockerCli,
    dockerfile: &str,
    image: &str,
    cache: BuildCache,
) -> Result<()> {
    let ctx = tempfile::tempdir().context("create empty build context")?;

    let mut cmd = docker.command();
    cmd.arg("build");
    cmd.args(cache.docker_args());
    cmd.args(["-f", "-", "-t", image]);
    cmd.arg(ctx.path());
    cmd.stdin(Stdio::piped());

    let mut child = cmd
        .spawn()
        .context("spawn docker build (is docker installed?)")?;

    // Feed the Dockerfile, then drop stdin so docker sees EOF. If docker exited
    // early (bad flag, daemon down) the write fails with EPIPE; reap the child
    // first and report its status, which carries the real error.
    let mut stdin = child.stdin.take().expect("stdin piped");
    let write_result = stdin.write_all(dockerfile.as_bytes());
    drop(stdin);

    let status = child.wait().context("wait for docker build")?;
    if !status.success() {
        bail!("docker build failed ({status})");
    }
    write_result.context("write Dockerfile to docker build stdin")?;
    Ok(())
}

pub(crate) fn build_image_for_service(
    docker: &DockerCli,
    dockerfile: &str,
    image: &str,
    cache: BuildCache,
    cancelled: Arc<AtomicBool>,
    log: Arc<dyn Fn(String) + Send + Sync>,
) -> Result<()> {
    let ctx = tempfile::tempdir().context("create empty build context")?;
    let mut cmd = docker.command();
    cmd.arg("build");
    cmd.args(cache.docker_args());
    cmd.args(["-f", "-", "-t", image]);
    cmd.arg(ctx.path());
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .context("spawn docker build (is docker installed?)")?;
    let stdout = child.stdout.take().context("capture docker build stdout")?;
    let stderr = child.stderr.take().context("capture docker build stderr")?;
    let stdout_log = log.clone();
    let stdout_thread = std::thread::spawn(move || forward_lines(stdout, stdout_log));
    let stderr_log = log.clone();
    let stderr_thread = std::thread::spawn(move || forward_lines(stderr, stderr_log));

    let mut stdin = child.stdin.take().expect("stdin piped");
    let write_result = stdin.write_all(dockerfile.as_bytes());
    drop(stdin);

    let status = loop {
        if let Some(status) = child.try_wait().context("poll docker build")? {
            break status;
        }
        if cancelled.load(Ordering::SeqCst) {
            let _ = child.kill();
            break child.wait().context("wait for cancelled docker build")?;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    if cancelled.load(Ordering::SeqCst) {
        anyhow::bail!("Docker image build cancelled");
    }
    if !status.success() {
        bail!("docker build failed ({status})");
    }
    write_result.context("write Dockerfile to docker build stdin")?;
    Ok(())
}

fn forward_lines(reader: impl std::io::Read, log: Arc<dyn Fn(String) + Send + Sync>) {
    let mut reader = BufReader::new(reader);
    let mut bytes = Vec::new();
    loop {
        bytes.clear();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) | Err(_) => break,
            Ok(_) => log(String::from_utf8_lossy(&bytes).trim_end().to_string()),
        }
    }
}

/// Whether an image reference exists locally.
///
/// A failed exact inspection is checked with an exact `docker image ls` query;
/// if Docker cannot complete either query, the daemon error is returned rather
/// than treating the image as absent.
pub fn image_exists(image: &str) -> Result<bool> {
    image_exists_with(&DockerCli::system(), image)
}

pub(crate) fn image_exists_with(docker: &DockerCli, image: &str) -> Result<bool> {
    let inspect = docker
        .command()
        .args(["image", "inspect", "--format", "{{.Id}}", image])
        .output()
        .context("inspect docker image (is docker installed?)")?;
    if inspect.status.success() {
        return Ok(true);
    }

    let list_ref = image_ref_for_exact_ls(image);
    let list = docker
        .command()
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

pub(super) fn image_ref_for_exact_ls(image: &str) -> String {
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
