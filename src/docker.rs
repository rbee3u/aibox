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
//! container operation per aibox process, whether a Run or a
//! Component installation. Cleanup is best-effort for uncatchable termination
//! such as SIGKILL.

#[cfg(test)]
use crate::sync::lock_unpoisoned;
use anyhow::{Context, Result};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[path = "docker_image.rs"]
mod image_ops;

pub(crate) use image_ops::image_exists_with;
#[cfg(test)]
use image_ops::image_ref_for_exact_ls;
pub use image_ops::{BuildCache, image_exists};
pub(crate) use image_ops::{build_image_for_service, inspect_runtime_image};
#[cfg(test)]
pub(crate) use image_ops::{build_image_with, inspect_runtime_image_with};

/// Fixed local Runtime Image tag used by every Run and runtime Component installer.
pub const IMAGE: &str = "aibox:latest";

/// Shared base Runtime Image Dockerfile without Tenant-local runtimes.
pub const DOCKERFILE: &str = include_str!("../assets/aibox.Dockerfile");

const CONTAINER_CREATE_WAIT: Duration = Duration::from_secs(1);
const CONTAINER_CREATE_POLL_INTERVAL: Duration = Duration::from_millis(20);

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
    run_with_mode(docker, run_args, image, cmd, after_container_created, true)
}

pub(crate) fn run_for_service(
    docker: &DockerCli,
    run_args: &[String],
    image: &str,
    cmd: &[OsString],
    after_container_created: impl FnOnce(),
) -> Result<i32> {
    run_with_mode(docker, run_args, image, cmd, after_container_created, false)
}

fn run_with_mode(
    docker: &DockerCli,
    run_args: &[String],
    image: &str,
    cmd: &[OsString],
    after_container_created: impl FnOnce(),
    install_signals: bool,
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
    set_cidfile_mode(&cid_path, docker, install_signals)?;
    let spawned = docker
        .command()
        .arg("run")
        .arg("--cidfile")
        .arg(&cid_path)
        .args(run_args)
        .arg(image)
        .args(cmd)
        .spawn();
    let child = match spawned {
        Ok(child) => child,
        Err(error) => {
            clear_child();
            return Err(error).context("spawn docker run (is docker installed?)");
        }
    };

    set_child(child.id());
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
        let stopped_lingering_container = finish_child();
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
        let _ = finish_child();
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

const DOCKER_INSPECT_TIMEOUT: Duration = Duration::from_secs(1);
const DOCKER_KILL_TIMEOUT: Duration = Duration::from_secs(3);
const CIDFILE_WAIT: Duration = Duration::from_secs(1);
const LATE_CIDFILE_WAIT: Duration = Duration::from_secs(3);
const CIDFILE_POLL_INTERVAL: Duration = Duration::from_millis(20);
const CONTAINER_GRACE: Duration = Duration::from_secs(10);
const CONTAINER_POLL_INTERVAL: Duration = Duration::from_millis(100);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(20);
const COMMAND_OUTPUT_LIMIT: u64 = 1024 * 1024;
// Main-thread fallback when a signal raced with `docker run` exiting. Covers
// cid discovery on the *late-cidfile* path (the first bounded wait fails, then
// the longer late wait succeeds), a graceful kill + bounded state probe, the
// full grace window (including one last bounded probe), the final SIGKILL, and
// scheduling slack — so the main thread never exits before the watcher can
// finish its worst-case bounded cleanup.
const SIGNAL_FINISH_WAIT: Duration = Duration::from_secs(27);

/// Whether the watcher thread is up. A `Mutex<bool>` rather than a `OnceLock`
/// so a failed install (Signals::new or thread spawn error) isn't remembered as
/// "installed" — the next run registration gets to retry instead of silently
/// running without interrupt-path cleanup.
static HANDLER_INSTALLED: Mutex<bool> = Mutex::new(false);

/// Number of watched fatal signals delivered to this process. A raw handler
/// increments it before the iterator handler wakes the watcher, so the watcher
/// can distinguish the first signal from a second one without a clear/store
/// race. The latter skips the graceful container-stop wait.
static SIGNAL_COUNT: AtomicUsize = AtomicUsize::new(0);
static LAST_SIGNAL: AtomicI32 = AtomicI32::new(0);

const RUN_IDLE: usize = 0;
const RUN_ACTIVE: usize = 1;
const RUN_SIGNALLED: usize = 2;

/// Coordinates the signal watcher with the main thread reaping `docker run`.
/// A foreground Ctrl-C reaches both processes: the Docker CLI can exit before
/// the watcher has read the cidfile. Marking the active run here lets the main
/// thread keep the cidfile registered until the watcher has stopped the
/// container, instead of racing ahead and clearing the only daemon-side handle.
static RUN_STATE: AtomicUsize = AtomicUsize::new(RUN_IDLE);

#[cfg(test)]
static RUN_REGISTRY_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn run_registry_test_lock() -> std::sync::MutexGuard<'static, ()> {
    lock_unpoisoned(&RUN_REGISTRY_TEST_LOCK)
}

/// The pid of the running `docker run` child, or 0 when none. The watcher
/// forwards the fatal signal to it: with no TTY the Docker CLI proxies the
/// signal to the container process; with one it at least exits.
static CHILD_PID: AtomicI32 = AtomicI32::new(0);

/// The `--cidfile` path of the running `docker run`, if any. The watcher reads
/// the container id from it to stop the container through the daemon — the one
/// route that works whether or not the docker CLI has a TTY attached.
static CIDFILE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static ACTIVE_DOCKER: OnceLock<Mutex<Option<DockerCli>>> = OnceLock::new();

fn cidfile() -> &'static Mutex<Option<PathBuf>> {
    CIDFILE.get_or_init(|| Mutex::new(None))
}

fn active_docker() -> &'static Mutex<Option<DockerCli>> {
    ACTIVE_DOCKER.get_or_init(|| Mutex::new(None))
}

fn current_docker() -> Option<DockerCli> {
    active_docker().lock().ok()?.clone()
}

#[cfg(test)]
fn set_cidfile(cidfile_path: &Path, docker: &DockerCli) -> Result<()> {
    set_cidfile_mode(cidfile_path, docker, true)
}

/// Register the `--cidfile` of an upcoming `docker run` for signal handling.
/// Call *before* spawning the child: the path is known upfront, and registering
/// it first closes the window where a signal lands after spawn but before any
/// registration — the watcher could then stop the container via the daemon even
/// with no child pid recorded yet. If spawning fails, call [`clear_child`];
/// otherwise register the child with [`set_child`] and finish it with
/// [`finish_child`]. A second registration is rejected while a run is active.
fn set_cidfile_mode(cidfile_path: &Path, docker: &DockerCli, install_signals: bool) -> Result<()> {
    if install_signals {
        install_signal_handler()?;
    }
    let mut registered_cidfile = cidfile().lock().unwrap();
    let mut registered_docker = active_docker().lock().unwrap();
    if RUN_STATE.load(Ordering::SeqCst) != RUN_IDLE {
        anyhow::bail!("another docker run is already registered in this process");
    }
    *registered_cidfile = Some(cidfile_path.to_path_buf());
    *registered_docker = Some(docker.clone());
    // Publish the active state only after both cleanup handles are visible. A
    // signal can now either observe idle, or observe active and wait for these
    // locks before reading a complete registration.
    RUN_STATE.store(RUN_ACTIVE, Ordering::SeqCst);
    Ok(())
}

pub(crate) fn cancel_active_container_operation() {
    stop_active_run(signal_hook::consts::SIGTERM);
}

/// Register the spawned `docker run` child's pid for signal forwarding. Call
/// right after spawn (after [`set_cidfile_mode`]). Once the child has been reaped,
/// call [`finish_child`] so a container that outlived the Docker client is
/// detected before the registration is cleared.
fn set_child(pid: u32) {
    CHILD_PID.store(pid as i32, Ordering::SeqCst);
}

/// Abandon a run registration when spawning failed after [`set_cidfile_mode`].
///
/// After a successful spawn, reap the child and call [`finish_child`] instead;
/// clearing directly would skip the lingering-container check.
fn clear_child() {
    let mut registered_cidfile = cidfile().lock().unwrap();
    CHILD_PID.store(0, Ordering::SeqCst);
    RUN_STATE.store(RUN_IDLE, Ordering::SeqCst);
    *registered_cidfile = None;
    *active_docker().lock().unwrap() = None;
}

/// Finish a successfully spawned child after `wait` returns. An attached Docker
/// CLI can exit while its container remains alive (most visibly via Docker's
/// detach key sequence, but also after some client/daemon disconnects), so use
/// the cidfile to stop a still-running container before unregistering the run.
/// If a fatal signal raced with the wait, clear the now-stale pid, retain the
/// cidfile, and keep this thread alive until the watcher terminates the process
/// after daemon-side cleanup. Returns `true` when a live or uninspectable
/// lingering container required a kill attempt.
fn finish_child() -> bool {
    CHILD_PID.store(0, Ordering::SeqCst);
    let stopped_lingering_container = stop_container_left_by_child();
    let mut registered_cidfile = cidfile().lock().unwrap();
    match RUN_STATE.compare_exchange(RUN_ACTIVE, RUN_IDLE, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) | Err(RUN_IDLE) => {
            *registered_cidfile = None;
            *active_docker().lock().unwrap() = None;
            stopped_lingering_container
        }
        Err(RUN_SIGNALLED) => {
            drop(registered_cidfile);
            // The watcher is stopping the container and will terminate the
            // whole process (`process::exit(128+sig)`) once daemon-side cleanup
            // is done, tearing down this parked thread with it. Park until it
            // does — but not forever: if the watcher thread died unexpectedly
            // (e.g. it panicked), parking with no bound would hang the wrapper.
            // The deadline covers the container grace period plus slack for the
            // bounded docker commands; past it, exit here as the signal would.
            let deadline = Instant::now() + SIGNAL_FINISH_WAIT;
            while Instant::now() < deadline {
                std::thread::park_timeout(Duration::from_secs(1));
            }
            let sig = LAST_SIGNAL.load(Ordering::SeqCst);
            let _ = signal_hook::low_level::emulate_default_handler(sig);
            std::process::exit(128 + sig);
        }
        Err(_) => unreachable!("invalid run state"),
    }
}

/// Kill a container that outlived its attached `docker run` client. Checking
/// daemon state first keeps the normal path cheap: after an ordinary `--rm`
/// exit the id no longer resolves. If the client has already gone away, there
/// is no interactive session left to drain, so do not make an EOF-triggered
/// exit wait through the signal path's ten-second grace period.
fn stop_container_left_by_child() -> bool {
    if RUN_STATE.load(Ordering::SeqCst) != RUN_ACTIVE {
        return false;
    }
    let Some(cid) = current_cid() else {
        return false;
    };
    let Some(docker) = current_docker() else {
        return true;
    };
    match container_state(&docker, &cid) {
        ContainerState::Stopped => return false,
        ContainerState::Running => {
            eprintln!(
                ">> docker run exited while container {cid} was still running; killing the container"
            );
        }
        ContainerState::Unknown => {
            eprintln!(
                ">> docker run exited but container {cid} state could not be confirmed; killing the container"
            );
        }
    }

    let _ = docker_quiet(&docker, &["kill", &cid], DOCKER_KILL_TIMEOUT);
    true
}

/// Forward `sig` to the registered docker CLI child, if any.
fn signal_child(sig: i32) {
    let pid = CHILD_PID.load(Ordering::SeqCst);
    if pid <= 0 {
        return;
    }
    let Some(pid) = rustix::process::Pid::from_raw(pid) else {
        return;
    };
    let rsig = match sig {
        s if s == signal_hook::consts::SIGINT => rustix::process::Signal::INT,
        s if s == signal_hook::consts::SIGHUP => rustix::process::Signal::HUP,
        _ => rustix::process::Signal::TERM,
    };
    let _ = rustix::process::kill_process(pid, rsig);
}

/// The registered container id, read fresh from the cidfile: the file appears
/// (daemon-side create) shortly after spawn, so it can't be read once upfront.
/// `None` when no run is active, the container isn't created yet, or the file
/// is empty.
fn current_cid() -> Option<String> {
    let path = cidfile().lock().ok()?.clone()?;
    let cid = std::fs::read_to_string(path).ok()?;
    let cid = cid.trim().to_string();
    (!cid.is_empty()).then_some(cid)
}

/// Wait briefly for Docker to populate the cidfile after `docker run` starts.
/// The file is registered before spawn, but the daemon writes the id shortly
/// after create; a fatal signal can land in that gap.
fn wait_current_cid(timeout: Duration) -> Option<String> {
    let started = Instant::now();
    loop {
        if let Some(cid) = current_cid() {
            return Some(cid);
        }
        if started.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(CIDFILE_POLL_INTERVAL);
    }
}

/// Outcome of a bounded, silent subprocess run.
enum CommandOutcome {
    /// Exited zero; carries captured stdout.
    Succeeded(String),
    /// Ran to completion but exited non-zero.
    Failed,
    /// Did not finish within the timeout, or could not be spawned or reaped.
    /// The subprocess may be wedged, so callers must not read this as a
    /// definitive answer.
    Unfinished,
}

/// Run a command silently with a timeout. Used by the signal watcher, where
/// Docker may be wedged and must not prevent the wrapper from re-raising the
/// fatal signal. A fast non-zero exit is distinguished from a timeout so
/// callers can tell "definitively no" from "no answer yet".
#[cfg(test)]
fn command_quiet(program: &str, args: &[&str], timeout: Duration) -> CommandOutcome {
    command_quiet_with(Command::new(program), args, timeout)
}

fn command_quiet_with(mut command: Command, args: &[&str], timeout: Duration) -> CommandOutcome {
    // A pipe can remain open in a subprocess descendant after the command we
    // spawned has exited, making a post-wait `read_to_end` block forever.
    // Capture into a regular temporary file instead: reading a snapshot of a
    // regular file reaches EOF even while an inherited writer is still open.
    let Ok(output) = tempfile::NamedTempFile::new() else {
        return CommandOutcome::Unfinished;
    };
    let Ok(child_stdout) = output.reopen() else {
        return CommandOutcome::Unfinished;
    };
    let spawned = command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(child_stdout))
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = spawned else {
        return CommandOutcome::Unfinished;
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return CommandOutcome::Failed;
                }
                let Ok(stdout_file) = output.reopen() else {
                    return CommandOutcome::Unfinished;
                };
                let mut stdout = Vec::new();
                if stdout_file
                    .take(COMMAND_OUTPUT_LIMIT.saturating_add(1))
                    .read_to_end(&mut stdout)
                    .is_err()
                    || stdout.len() as u64 > COMMAND_OUTPUT_LIMIT
                {
                    return CommandOutcome::Unfinished;
                }
                return CommandOutcome::Succeeded(String::from_utf8_lossy(&stdout).into_owned());
            }
            Ok(None) => {}
            Err(_) => {
                kill_and_reap_in_background(child);
                return CommandOutcome::Unfinished;
            }
        }
        if output
            .as_file()
            .metadata()
            .map_or(true, |metadata| metadata.len() > COMMAND_OUTPUT_LIMIT)
        {
            kill_and_reap_in_background(child);
            return CommandOutcome::Unfinished;
        }
        if started.elapsed() >= timeout {
            kill_and_reap_in_background(child);
            return CommandOutcome::Unfinished;
        }
        std::thread::sleep(COMMAND_POLL_INTERVAL);
    }
}

/// Terminate a timed-out helper without letting process reaping defeat the
/// caller's deadline. `Child::wait` can itself remain blocked when a process is
/// stuck in uninterruptible kernel I/O, so a detached thread owns that
/// best-effort reap. Dropping the join handle does not delay wrapper shutdown.
fn kill_and_reap_in_background(mut child: Child) {
    let _ = child.kill();
    let _ = std::thread::Builder::new()
        .name("aibox-command-reaper".into())
        .spawn(move || {
            let _ = child.wait();
        });
}

/// Run `docker <args>` silently, returning stdout on success. The watcher's
/// container-stopping calls are all best-effort: a dead daemon or an
/// already-removed container just means there is nothing left to stop.
fn docker_quiet(docker: &DockerCli, args: &[&str], timeout: Duration) -> Option<String> {
    match command_quiet_with(docker.command(), args, timeout) {
        CommandOutcome::Succeeded(out) => Some(out),
        CommandOutcome::Failed | CommandOutcome::Unfinished => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainerState {
    Running,
    Stopped,
    Unknown,
}

fn parse_container_state(outcome: &CommandOutcome) -> ContainerState {
    match outcome {
        CommandOutcome::Succeeded(out) => match out.trim() {
            "true" => ContainerState::Running,
            "false" => ContainerState::Stopped,
            // A zero exit with an unexpected body shouldn't happen for this
            // format string; stay conservative rather than assume "stopped".
            _ => ContainerState::Unknown,
        },
        // A non-zero exit, timeout, or spawn failure is not enough to prove the
        // container is gone. `container_state` may disambiguate a fast inspect
        // failure with an exact list query; otherwise stay conservative.
        CommandOutcome::Failed | CommandOutcome::Unfinished => ContainerState::Unknown,
    }
}

fn parse_container_list_after_failed_inspect(outcome: CommandOutcome) -> ContainerState {
    match outcome {
        CommandOutcome::Succeeded(output) if output.trim().is_empty() => ContainerState::Stopped,
        CommandOutcome::Succeeded(_) | CommandOutcome::Failed | CommandOutcome::Unfinished => {
            ContainerState::Unknown
        }
    }
}

/// The daemon's view of the container. A wedged daemon is unknown, not stopped:
/// treating it as "done" can leave the container alive. A fast non-zero
/// `inspect` is ambiguous — the id may be gone, or the daemon may be failing —
/// so an exact `container ls` query distinguishes those cases.
fn container_state(docker: &DockerCli, cid: &str) -> ContainerState {
    let outcome = command_quiet_with(
        docker.command(),
        &["inspect", "-f", "{{.State.Running}}", cid],
        DOCKER_INSPECT_TIMEOUT,
    );
    let state = parse_container_state(&outcome);
    if state != ContainerState::Unknown || !matches!(outcome, CommandOutcome::Failed) {
        return state;
    }

    let filter = format!("id={cid}");
    parse_container_list_after_failed_inspect(command_quiet_with(
        docker.command(),
        &[
            "container",
            "ls",
            "--all",
            "--quiet",
            "--no-trunc",
            "--filter",
            &filter,
        ],
        DOCKER_INSPECT_TIMEOUT,
    ))
}

/// Stop the active run without letting a slow cidfile create window orphan the
/// container. Prefer the daemon-side container id path; if the id is not ready
/// yet, signal the Docker CLI child, then keep polling briefly for a late
/// cidfile so a just-created TTY container still gets killed through the daemon.
fn stop_active_run(sig: i32) {
    let Some(docker) = current_docker() else {
        signal_child(sig);
        return;
    };
    if let Some(cid) = wait_current_cid(CIDFILE_WAIT) {
        stop_container_id(&docker, sig, &cid);
        signal_child(sig);
        return;
    }

    signal_child(sig);
    if let Some(cid) = wait_current_cid(LATE_CIDFILE_WAIT) {
        stop_container_id(&docker, sig, &cid);
    }
}

/// Stop one container through the daemon: deliver `sig` to its PID 1 (what
/// `--sig-proxy` would have done, had the CLI not had a TTY), then escalate to a
/// plain `docker kill` (SIGKILL) if it lingers. A container process without a
/// handler for the signal never exits on it as PID 1. The 10s grace mirrors
/// `docker stop`'s default.
///
/// On the signal path, the main thread normally stays blocked in `child.wait()`
/// while the watcher performs this escalation, so the grace wait cannot race
/// the exit path; [`stop_active_run`] decides whether it runs before or after
/// the CLI child is signalled. The post-wait orphan check takes a separate
/// immediate-kill path because no attached client remains.
fn stop_container_id(docker: &DockerCli, sig: i32, cid: &str) {
    let name = match sig {
        s if s == signal_hook::consts::SIGINT => "INT",
        s if s == signal_hook::consts::SIGHUP => "HUP",
        _ => "TERM",
    };
    let _ = docker_quiet(
        docker,
        &["kill", "--signal", name, cid],
        DOCKER_KILL_TIMEOUT,
    );
    if container_state(docker, cid) == ContainerState::Stopped {
        return;
    }
    // Say what the silence is (the grace wait), and how to cut it short: a
    // second signal (Ctrl-C again, or a service manager re-kill) skips the
    // rest of the wait and SIGKILLs the container now — better than lingering
    // under a supervisor that would escalate to an uncatchable SIGKILL and
    // leave the container running unsupervised.
    eprintln!(">> stopping the container (up to 10s; signal again to kill it now)");
    let started = Instant::now();
    while started.elapsed() < CONTAINER_GRACE {
        if SIGNAL_COUNT.load(Ordering::SeqCst) > 1 {
            break;
        }
        std::thread::sleep(CONTAINER_POLL_INTERVAL);
        if container_state(docker, cid) == ContainerState::Stopped {
            return;
        }
    }
    let _ = docker_quiet(docker, &["kill", cid], DOCKER_KILL_TIMEOUT);
}

/// True if `sig` is currently ignored (SIG_IGN). Watching an ignored signal
/// would *un*-ignore it: signal-hook installs its own handler over SIG_IGN, so
/// under `nohup` (which sets SIGHUP to SIG_IGN) the watcher would turn a
/// survivable hangup back into a death. Read-only `sigaction` query.
fn signal_is_ignored(sig: i32) -> bool {
    // SAFETY: `sig` is one of the platform signal constants selected below;
    // `old` is valid writable storage, and a null action makes this a read-only
    // query whose return value is checked before inspecting the result.
    unsafe {
        let mut old: libc::sigaction = std::mem::zeroed();
        libc::sigaction(sig, std::ptr::null(), &mut old) == 0 && old.sa_sigaction == libc::SIG_IGN
    }
}

/// Spawn the SIGINT/SIGTERM/SIGHUP watcher thread (once per process). SIGHUP is
/// included only when not already ignored (see [`signal_is_ignored`]). The
/// thread parks in [`signal_hook::iterator::Signals::forever`] and never blocks
/// process exit. Idempotent; a failed install is retried on the next call
/// instead of being remembered as installed.
fn install_signal_handler() -> Result<()> {
    let mut installed = HANDLER_INSTALLED.lock().unwrap();
    if *installed {
        return Ok(());
    }
    let mut watched = vec![signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM];
    if !signal_is_ignored(signal_hook::consts::SIGHUP) {
        watched.push(signal_hook::consts::SIGHUP);
    }

    let initial_signal_count = SIGNAL_COUNT.load(Ordering::SeqCst);

    // Register the state/count action before the iterator actions. signal-hook
    // preserves registration order; this guarantees that once the watcher is
    // woken, RUN_STATE already records the signal that woke it.
    let mut registrations = Vec::new();
    for &sig in &watched {
        // SAFETY: the handler performs only lock-free atomic operations, which
        // are async-signal-safe, captures no borrowed state, and leaves all
        // allocation, I/O, locking, and Docker cleanup to the watcher thread.
        let registration = unsafe {
            signal_hook::low_level::register(sig, move || {
                LAST_SIGNAL.store(sig, Ordering::SeqCst);
                SIGNAL_COUNT.fetch_add(1, Ordering::SeqCst);
                let _ = RUN_STATE.compare_exchange(
                    RUN_ACTIVE,
                    RUN_SIGNALLED,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
            })
        };
        match registration {
            Ok(id) => registrations.push(id),
            Err(error) => {
                for id in registrations {
                    signal_hook::low_level::unregister(id);
                }
                return Err(error).context("install signal state handler");
            }
        }
    }

    let mut signals = match signal_hook::iterator::Signals::new(&watched) {
        Ok(signals) => signals,
        Err(error) => {
            for id in registrations {
                signal_hook::low_level::unregister(id);
            }
            return Err(error).context("install signal cleanup handler");
        }
    };
    let spawned = std::thread::Builder::new()
        .name("aibox-signals".into())
        .spawn(move || {
            if let Some(sig) = signals.forever().next() {
                // Stop the container via the daemon when possible, with a
                // late-cidfile fallback for signals that land during create.
                stop_active_run(sig);
                // Die as if unhandled, so the exit status reflects the signal.
                // If emulation returns in this environment, still exit with the
                // shell convention instead of going back to child.wait().
                let _ = signal_hook::low_level::emulate_default_handler(sig);
                std::process::exit(128 + sig);
            }
        });
    match spawned {
        Ok(_) => {
            *installed = true;
            // A signal can land in the tiny interval after the state action is
            // registered but before `Signals` installs its wakeup action. It
            // was recorded but could not wake the watcher, so re-deliver it
            // now that the complete handler is live. This happens before the
            // Docker child is spawned.
            if SIGNAL_COUNT.load(Ordering::SeqCst) > initial_signal_count {
                let sig = LAST_SIGNAL.load(Ordering::SeqCst);
                if sig != 0 {
                    let _ = signal_hook::low_level::raise(sig);
                }
            }
        }
        Err(error) => {
            for id in registrations {
                signal_hook::low_level::unregister(id);
            }
            return Err(error).context("spawn signal cleanup thread");
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "docker_tests.rs"]
mod tests;
