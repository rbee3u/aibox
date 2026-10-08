//! Runtime Image construction and supervised Docker execution.
//!
//! The process-wide child/cidfile registry supports one active container operation.
//! Signals stop the container through the daemon before terminating the Docker
//! client; inherited ignored SIGHUP stays ignored. Cleanup cannot cover SIGKILL.

use anyhow::{Context, Result};
use std::ffi::OsString;
#[cfg(test)]
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
#[cfg(test)]
use std::time::{Duration, Instant};

mod image;

pub(crate) use image::image_exists_with;
#[cfg(test)]
use image::image_ref_for_exact_ls;
pub(crate) use image::{BuildCache, image_exists};
pub(crate) use image::{RuntimeImageInspection, build_image_for_service, inspect_runtime_image};
#[cfg(test)]
pub(crate) use image::{build_image_with, inspect_runtime_image_with};

/// Fixed local Runtime Image tag used by Runs, Debug Shells, and container-based
/// Component installations.
pub(crate) const IMAGE: &str = "aibox:latest";

/// Shared base Runtime Image Dockerfile without Tenant-local runtimes.
pub(crate) const DOCKERFILE: &str = include_str!("../../assets/aibox.Dockerfile");

pub(crate) type LogCallback = Arc<dyn Fn(String) + Send + Sync>;

mod run;
mod supervision;
use run::{
    ContainerCreate, RegisteredRun, exit_code, forward_lines, wait_for_container_create,
    wait_with_delayed_container_create,
};
use supervision::RunRegistration;
pub(crate) use supervision::cancel_active_container_operation;

/// The process-wide run-registry lock, for suites outside `docker/` that start
/// a container. `docker/`'s own suite reaches `supervision` directly.
#[cfg(test)]
pub(crate) use supervision::run_registry_test_lock;

#[derive(Clone, Debug)]
pub(crate) struct DockerCli {
    program: OsString,
    isolated_env: Option<Vec<(OsString, OsString)>>,
}

impl DockerCli {
    pub(crate) fn system() -> Self {
        Self {
            program: "docker".into(),
            isolated_env: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn isolated(
        program: impl Into<OsString>,
        env: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Self {
        Self {
            program: program.into(),
            isolated_env: Some(env.into_iter().collect()),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        if let Some(env) = &self.isolated_env {
            command.env_clear().envs(env.iter().cloned());
        }
        command
    }

    /// Run `docker <args>` to completion and collect its output, with the same
    /// `ETXTBSY` tolerance as [`Self::spawn`].
    fn output<I, S>(&self, args: I) -> std::io::Result<std::process::Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let mut command = self.command();
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        self.spawn(&mut command)?.wait_with_output()
    }

    /// Retry transient `ETXTBSY` only for isolated test CLIs: concurrent forks
    /// can briefly inherit a freshly written stub's writable descriptor.
    /// Production Docker spawn errors pass through unchanged.
    fn spawn(&self, command: &mut Command) -> std::io::Result<Child> {
        let mut result = command.spawn();
        if self.isolated_env.is_none() {
            return result;
        }
        for attempt in 0..50u64 {
            match result {
                Err(ref error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                    std::thread::sleep(std::time::Duration::from_millis(attempt.min(4) + 1));
                    result = command.spawn();
                }
                _ => break,
            }
        }
        result
    }
}

/// Run a supervised Docker child and return its exit code. Do not call concurrently:
/// the child/cidfile registry is process-wide. Daemon-side cleanup is required
/// because killing an attached Docker client may leave its container running.
///
/// `after_container_created` runs at most once after the cidfile receives an id;
/// it is skipped if the child exits without one. A live or uninspectable container
/// left by the child triggers a kill attempt and turns a zero exit code into failure.
pub(crate) fn run(
    run_args: &[String],
    image: &str,
    cmd: &[OsString],
    after_container_created: impl FnOnce(),
) -> Result<i32> {
    run_with(
        &DockerCli::system(),
        run_args,
        image,
        cmd,
        after_container_created,
    )
}

pub(crate) fn run_with(
    docker: &DockerCli,
    run_args: &[String],
    image: &str,
    cmd: &[OsString],
    after_container_created: impl FnOnce(),
) -> Result<i32> {
    run_with_mode(
        docker,
        run_args,
        image,
        cmd,
        after_container_created,
        true,
        None,
    )
}

pub(crate) fn run_for_service(
    docker: &DockerCli,
    run_args: &[String],
    image: &str,
    cmd: &[OsString],
    after_container_created: impl FnOnce(),
    log: LogCallback,
) -> Result<i32> {
    run_with_mode(
        docker,
        run_args,
        image,
        cmd,
        after_container_created,
        false,
        Some(log),
    )
}

fn run_with_mode(
    docker: &DockerCli,
    run_args: &[String],
    image: &str,
    cmd: &[OsString],
    after_container_created: impl FnOnce(),
    install_signals: bool,
    log: Option<LogCallback>,
) -> Result<i32> {
    let mut after_container_created = Some(after_container_created);
    // Docker requires a cidfile path that does not yet exist.
    let cid_dir = tempfile::tempdir().context("create cidfile dir")?;
    let cid_path = cid_dir.path().join("cid");

    // Register before spawn so a signal always has a container cleanup handle.
    let registration = RunRegistration::new(&cid_path, docker, install_signals)?;
    let mut command = docker.command();
    command
        .arg("run")
        .arg("--cidfile")
        .arg(&cid_path)
        .args(run_args)
        .arg(image)
        .args(cmd);
    if log.is_some() {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    let child = docker
        .spawn(&mut command)
        .context("spawn docker run (is docker installed?)")?;

    let mut registered_run = RegisteredRun::with_registration(child, registration);
    if let Some(log) = log {
        registered_run.capture_output(log)?;
    }
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

#[cfg(test)]
#[path = "docker_tests.rs"]
mod tests;
