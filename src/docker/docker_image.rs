//! Docker image construction and exact local-image inspection.

use super::{DockerCli, LogCallback, forward_lines};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::io::Write;
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

#[cfg(test)]
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
    log: LogCallback,
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
        .output(["image", "inspect", "--format", "{{.Id}}", image])
        .context("inspect docker image (is docker installed?)")?;
    if inspect.status.success() {
        return Ok(true);
    }

    let list_ref = image_ref_for_exact_ls(image);
    let list = docker
        .output(["image", "ls", "--quiet", "--no-trunc", &list_ref])
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

/// Metadata available from an exact local Runtime Image inspection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeImageInspection {
    pub(crate) present: bool,
    pub(crate) id: Option<String>,
    pub(crate) created_at: Option<String>,
    pub(crate) size_bytes: Option<u64>,
    pub(crate) detail: Option<String>,
}

#[derive(Deserialize)]
struct DockerImageMetadata {
    id: String,
    created_at: String,
    size_bytes: u64,
}

/// Inspect one exact Runtime Image without exposing its configuration or layers.
pub(crate) fn inspect_runtime_image(image: &str) -> Result<RuntimeImageInspection> {
    inspect_runtime_image_with(&DockerCli::system(), image)
}

pub(crate) fn inspect_runtime_image_with(
    docker: &DockerCli,
    image: &str,
) -> Result<RuntimeImageInspection> {
    const FORMAT: &str =
        r#"{"id":{{json .Id}},"created_at":{{json .Created}},"size_bytes":{{.Size}}}"#;
    let inspect = docker
        .output(["image", "inspect", "--format", FORMAT, image])
        .context("inspect docker image metadata (is docker installed?)")?;
    if inspect.status.success() {
        return match serde_json::from_slice::<DockerImageMetadata>(&inspect.stdout) {
            Ok(metadata) => Ok(RuntimeImageInspection {
                present: true,
                id: Some(metadata.id),
                created_at: Some(metadata.created_at),
                size_bytes: Some(metadata.size_bytes),
                detail: None,
            }),
            Err(error) => Ok(RuntimeImageInspection {
                present: true,
                id: None,
                created_at: None,
                size_bytes: None,
                detail: Some(format!("parse docker image metadata: {error}")),
            }),
        };
    }

    let list_ref = image_ref_for_exact_ls(image);
    let list = docker
        .output(["image", "ls", "--quiet", "--no-trunc", &list_ref])
        .context("list docker image (is docker installed?)")?;
    if list.status.success() {
        let present = !String::from_utf8_lossy(&list.stdout).trim().is_empty();
        return Ok(RuntimeImageInspection {
            present,
            id: None,
            created_at: None,
            size_bytes: None,
            detail: present.then(|| {
                format!(
                    "docker image metadata unavailable: {}",
                    String::from_utf8_lossy(&inspect.stderr).trim()
                )
            }),
        });
    }

    bail!(
        "docker image inspect failed ({}): {}; docker image ls failed ({}): {}",
        inspect.status,
        String::from_utf8_lossy(&inspect.stderr).trim(),
        list.status,
        String::from_utf8_lossy(&list.stderr).trim()
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
