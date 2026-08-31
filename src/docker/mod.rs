//! Building and running cleanup-aware containers.
//!
//! Image inspection, Console-triggered image construction, and [`run`] (which
//! spawns `docker run` for a Coding Agent or Component installer) all shell out
//! to the Docker CLI. Image build and inspection live in the `docker_image.rs`
//! submodule, which documents the context-free build; this module owns the run
//! path and its cleanup.
//!
//! ## Signal-aware cleanup
//!
//! The process-wide Docker child/cidfile registry stops containers on signals
//! delivered only to the wrapper and detects a container that outlives its
//! attached Docker client. Fatal signals are routed to a watcher thread that
//! stops the container through the daemon, forwards the signal to the Docker
//! CLI child, and then restores the signal's normal process result. SIGHUP is
//! watched only when it was not already ignored, preserving `nohup` behavior.
//!
//! The child pid, cidfile, and run state intentionally support one active
//! container operation per `aibox` process, whether a Run or a
//! Component installation. Cleanup is best-effort for uncatchable termination
//! such as SIGKILL.

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

#[path = "docker_image.rs"]
mod image_ops;

pub(crate) use image_ops::image_exists_with;
#[cfg(test)]
use image_ops::image_ref_for_exact_ls;
pub use image_ops::{BuildCache, image_exists};
pub(crate) use image_ops::{
    RuntimeImageInspection, build_image_for_service, inspect_runtime_image,
};
#[cfg(test)]
pub(crate) use image_ops::{build_image_with, inspect_runtime_image_with};

/// Fixed local Runtime Image tag used by every Run and runtime Component installer.
pub const IMAGE: &str = "aibox:latest";

/// Shared base Runtime Image Dockerfile without Tenant-local runtimes.
pub const DOCKERFILE: &str = include_str!("../../assets/aibox.Dockerfile");

pub(crate) type LogCallback = Arc<dyn Fn(String) + Send + Sync>;

mod run;
mod supervision;
use run::{
    ContainerCreate, RegisteredRun, exit_code, forward_lines, wait_for_container_create,
    wait_with_delayed_container_create,
};
use supervision::RunRegistration;
pub(crate) use supervision::cancel_active_container_operation;

#[cfg(test)]
pub(crate) use supervision::*;

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

    /// Spawn `command`, absorbing a transient `ETXTBSY` from a stub program.
    ///
    /// Only an isolated CLI retries. A test writes its stub script and execs it
    /// immediately, while `fork` on any other test thread briefly duplicates
    /// that file's writable descriptor into the child; exec fails with
    /// `ETXTBSY` until the descriptor closes. Stub writes and forks happen in
    /// different modules, so no test-local lock can order them. A real
    /// `docker` binary is never mid-write, so `ETXTBSY` there is a genuine
    /// failure and is reported unchanged.
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

/// Run `docker run <args> <image> <cmd...>` as a child process and return its
/// exit code. A child (not `exec`) so the caller's container cleanup still runs
/// after it returns. The child's pid and `--cidfile` are registered in the
/// process-wide run registry (`set_cidfile_mode`, `set_child`, `finish_child`) for
/// the run's duration, so a SIGINT/SIGTERM aimed at the wrapper alone stops
/// the container instead of leaving it running unsupervised — killing just the
/// docker CLI is not enough when a TTY is attached (the CLI only proxies
/// signals without one).
///
/// `after_container_created` runs at most once, after Docker has written a
/// container id and before this function waits for the container to exit. It is
/// not called if the Docker child exits before creating a container. If the
/// Docker child leaves a live or uninspectable container behind, AIBox attempts
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
    // Docker refuses to reuse an existing cidfile, so ask for a fresh path
    // inside a temp dir. The id it holds is not a secret; if a signal kills us
    // before the dir's cleanup, the leftover is harmless.
    let cid_dir = tempfile::tempdir().context("create cidfile dir")?;
    let cid_path = cid_dir.path().join("cid");

    // Register the cidfile *before* spawning: a signal landing between spawn
    // and registration could otherwise find neither a pid nor a container id,
    // leaving the container running unsupervised.
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
    let spawned = docker.spawn(&mut command);
    let child = match spawned {
        Ok(child) => child,
        Err(error) => {
            return Err(error).context("spawn docker run (is docker installed?)");
        }
    };

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
