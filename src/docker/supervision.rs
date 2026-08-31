//! Process-wide Docker child/cidfile/signal supervision.
//!
//! This module owns the cleanup registry and all daemon-side container
//! probing. The run facade only composes registration, child waiting, and
//! output capture around this owner.

use super::DockerCli;
#[cfg(test)]
use crate::foundation::sync::lock_unpoisoned;
use anyhow::{Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub(crate) fn cidfile_has_id(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|cid| !cid.trim().is_empty())
}

pub(crate) const DOCKER_INSPECT_TIMEOUT: Duration = Duration::from_secs(1);
pub(crate) const DOCKER_KILL_TIMEOUT: Duration = Duration::from_secs(3);
pub(crate) const CIDFILE_WAIT: Duration = Duration::from_secs(1);
pub(crate) const LATE_CIDFILE_WAIT: Duration = Duration::from_secs(3);
pub(crate) const CIDFILE_POLL_INTERVAL: Duration = Duration::from_millis(20);
pub(crate) const CONTAINER_GRACE: Duration = Duration::from_secs(10);
pub(crate) const CONTAINER_POLL_INTERVAL: Duration = Duration::from_millis(100);
pub(crate) const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(20);
pub(crate) const COMMAND_OUTPUT_LIMIT: u64 = 1024 * 1024;
// Main-thread fallback when a signal raced with `docker run` exiting. Covers
// cid discovery on the *late-cidfile* path (the first bounded wait fails, then
// the longer late wait succeeds), a graceful kill + bounded state probe, the
// full grace window (including one last bounded probe), the final SIGKILL, and
// scheduling slack — so the main thread never exits before the watcher can
// finish its worst-case bounded cleanup.
pub(crate) const SIGNAL_FINISH_WAIT: Duration = Duration::from_secs(27);

/// Whether the watcher thread is up. A `Mutex<bool>` rather than a `OnceLock`
/// so a failed install (Signals::new or thread spawn error) isn't remembered as
/// "installed" — the next run registration gets to retry instead of silently
/// running without interrupt-path cleanup.
pub(crate) static HANDLER_INSTALLED: Mutex<bool> = Mutex::new(false);

/// Number of watched fatal signals delivered to this process. A raw handler
/// increments it before the iterator handler wakes the watcher, so the watcher
/// can distinguish the first signal from a second one without a clear/store
/// race. The latter skips the graceful container-stop wait.
pub(crate) static SIGNAL_COUNT: AtomicUsize = AtomicUsize::new(0);
pub(crate) static LAST_SIGNAL: AtomicI32 = AtomicI32::new(0);

pub(crate) const RUN_IDLE: usize = 0;
pub(crate) const RUN_ACTIVE: usize = 1;
pub(crate) const RUN_SIGNALLED: usize = 2;

/// Coordinates the signal watcher with the main thread reaping `docker run`.
/// A foreground Ctrl-C reaches both processes: the Docker CLI can exit before
/// the watcher has read the cidfile. Marking the active run here lets the main
/// thread keep the cidfile registered until the watcher has stopped the
/// container, instead of racing ahead and clearing the only daemon-side handle.
pub(crate) static RUN_STATE: AtomicUsize = AtomicUsize::new(RUN_IDLE);

#[cfg(test)]
pub(crate) static RUN_REGISTRY_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn run_registry_test_lock() -> std::sync::MutexGuard<'static, ()> {
    lock_unpoisoned(&RUN_REGISTRY_TEST_LOCK)
}

/// The pid of the running `docker run` child, or 0 when none. The watcher
/// forwards the fatal signal to it: with no TTY the Docker CLI proxies the
/// signal to the container process; with one it at least exits.
pub(crate) static CHILD_PID: AtomicI32 = AtomicI32::new(0);

/// The `--cidfile` path of the running `docker run`, if any. The watcher reads
/// the container id from it to stop the container through the daemon — the one
/// route that works whether or not the docker CLI has a TTY attached.
pub(crate) static CIDFILE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
pub(crate) static ACTIVE_DOCKER: OnceLock<Mutex<Option<DockerCli>>> = OnceLock::new();

pub(crate) fn cidfile() -> &'static Mutex<Option<PathBuf>> {
    CIDFILE.get_or_init(|| Mutex::new(None))
}

pub(crate) fn active_docker() -> &'static Mutex<Option<DockerCli>> {
    ACTIVE_DOCKER.get_or_init(|| Mutex::new(None))
}

pub(crate) fn current_docker() -> Option<DockerCli> {
    active_docker().lock().ok()?.clone()
}

#[cfg(test)]
pub(crate) fn set_cidfile(cidfile_path: &Path, docker: &DockerCli) -> Result<()> {
    set_cidfile_mode(cidfile_path, docker, true)
}

/// Register the `--cidfile` of an upcoming `docker run` for signal handling.
/// Call *before* spawning the child: the path is known upfront, and registering
/// it first closes the window where a signal lands after spawn but before any
/// registration — the watcher could then stop the container via the daemon even
/// with no child pid recorded yet. If spawning fails, call [`clear_child`];
/// otherwise register the child with [`set_child`] and finish it with
/// [`finish_child`]. A second registration is rejected while a run is active.
pub(crate) fn set_cidfile_mode(
    cidfile_path: &Path,
    docker: &DockerCli,
    install_signals: bool,
) -> Result<()> {
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
pub(crate) fn set_child(pid: u32) {
    CHILD_PID.store(pid as i32, Ordering::SeqCst);
}

/// Abandon a run registration when spawning failed after [`set_cidfile_mode`].
///
/// After a successful spawn, reap the child and call [`finish_child`] instead;
/// clearing directly would skip the lingering-container check.
pub(crate) fn clear_child() {
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
pub(crate) fn finish_child() -> bool {
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
pub(crate) fn stop_container_left_by_child() -> bool {
    if RUN_STATE.load(Ordering::SeqCst) != RUN_ACTIVE {
        return false;
    }
    let Some(cid) = current_cid() else {
        return false;
    };
    let Some(docker) = current_docker() else {
        return true;
    };
    let state = container_state(&docker, &cid);
    stop_lingering_container_with(&cid, state, |cid| {
        let _ = docker_quiet(&docker, &["kill", cid], DOCKER_KILL_TIMEOUT);
    })
}

/// Apply the immediate post-client-exit cleanup policy to an observed
/// container state. Keeping state inspection and the kill operation outside
/// this coordinator lets the ordering policy be tested without subprocess
/// scheduling, while production still uses the Docker CLI adapters above.
fn stop_lingering_container_with(
    cid: &str,
    state: ContainerState,
    mut kill_container: impl FnMut(&str),
) -> bool {
    match state {
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

    kill_container(cid);
    true
}

/// Forward `sig` to the registered docker CLI child, if any.
pub(crate) fn signal_child(sig: i32) {
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
pub(crate) fn current_cid() -> Option<String> {
    let path = cidfile().lock().ok()?.clone()?;
    let cid = std::fs::read_to_string(path).ok()?;
    let cid = cid.trim().to_string();
    (!cid.is_empty()).then_some(cid)
}

/// Wait briefly for Docker to populate the cidfile after `docker run` starts.
/// The file is registered before spawn, but the daemon writes the id shortly
/// after create; a fatal signal can land in that gap.
pub(crate) fn wait_current_cid(timeout: Duration) -> Option<String> {
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
pub(crate) enum CommandOutcome {
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
pub(crate) fn command_quiet(program: &str, args: &[&str], timeout: Duration) -> CommandOutcome {
    command_quiet_with(Command::new(program), args, timeout)
}

pub(crate) fn command_quiet_with(
    mut command: Command,
    args: &[&str],
    timeout: Duration,
) -> CommandOutcome {
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
pub(crate) fn kill_and_reap_in_background(mut child: Child) {
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
pub(crate) fn docker_quiet(docker: &DockerCli, args: &[&str], timeout: Duration) -> Option<String> {
    match command_quiet_with(docker.command(), args, timeout) {
        CommandOutcome::Succeeded(out) => Some(out),
        CommandOutcome::Failed | CommandOutcome::Unfinished => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContainerState {
    Running,
    Stopped,
    Unknown,
}

pub(crate) fn parse_container_state(outcome: &CommandOutcome) -> ContainerState {
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

pub(crate) fn parse_container_list_after_failed_inspect(outcome: CommandOutcome) -> ContainerState {
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
pub(crate) fn container_state(docker: &DockerCli, cid: &str) -> ContainerState {
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
pub(crate) fn stop_active_run(sig: i32) {
    let Some(docker) = current_docker() else {
        signal_child(sig);
        return;
    };
    stop_active_run_with(
        sig,
        wait_current_cid,
        |sig, cid| stop_container_id(&docker, sig, cid),
        signal_child,
    );
}

/// Coordinate the two cidfile discovery phases around Docker child signaling.
/// The production facade supplies the real polling, signal, and daemon-side
/// cleanup adapters; tests script their results so the create-window policy
/// does not depend on neighboring wall-clock deadlines.
fn stop_active_run_with(
    sig: i32,
    mut wait_for_cid: impl FnMut(Duration) -> Option<String>,
    mut stop_container: impl FnMut(i32, &str),
    mut stop_child: impl FnMut(i32),
) {
    if let Some(cid) = wait_for_cid(CIDFILE_WAIT) {
        stop_container(sig, &cid);
        stop_child(sig);
        return;
    }

    stop_child(sig);
    if let Some(cid) = wait_for_cid(LATE_CIDFILE_WAIT) {
        stop_container(sig, &cid);
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
pub(crate) fn stop_container_id(docker: &DockerCli, sig: i32, cid: &str) {
    let mut grace_started = None;
    stop_container_id_with(
        sig,
        cid,
        |name, cid| {
            let _ = docker_quiet(
                docker,
                &["kill", "--signal", name, cid],
                DOCKER_KILL_TIMEOUT,
            );
        },
        |cid| container_state(docker, cid),
        || {
            let started = grace_started.get_or_insert_with(Instant::now);
            continue_container_grace(SIGNAL_COUNT.load(Ordering::SeqCst), started.elapsed())
        },
        std::thread::sleep,
        |cid| {
            let _ = docker_quiet(docker, &["kill", cid], DOCKER_KILL_TIMEOUT);
        },
    );
}

fn continue_container_grace(signal_count: usize, elapsed: Duration) -> bool {
    signal_count <= 1 && elapsed < CONTAINER_GRACE
}

/// Apply graceful container-stop policy using caller-provided process and time
/// adapters. Production supplies Docker commands, the signal counter, and
/// bounded sleeping; tests script each observation and verify exact ordering.
fn stop_container_id_with(
    sig: i32,
    cid: &str,
    mut signal_container: impl FnMut(&str, &str),
    mut container_state: impl FnMut(&str) -> ContainerState,
    mut continue_grace: impl FnMut() -> bool,
    mut wait: impl FnMut(Duration),
    mut kill_container: impl FnMut(&str),
) {
    let name = match sig {
        s if s == signal_hook::consts::SIGINT => "INT",
        s if s == signal_hook::consts::SIGHUP => "HUP",
        _ => "TERM",
    };
    signal_container(name, cid);
    if container_state(cid) == ContainerState::Stopped {
        return;
    }
    // Say what the silence is (the grace wait), and how to cut it short: a
    // second signal (Ctrl-C again, or a service manager re-kill) skips the
    // rest of the wait and SIGKILLs the container now — better than lingering
    // under a supervisor that would escalate to an uncatchable SIGKILL and
    // leave the container running unsupervised.
    eprintln!(">> stopping the container (up to 10s; signal again to kill it now)");
    while continue_grace() {
        wait(CONTAINER_POLL_INTERVAL);
        if container_state(cid) == ContainerState::Stopped {
            return;
        }
    }
    kill_container(cid);
}

/// True if `sig` is currently ignored (SIG_IGN). Watching an ignored signal
/// would *un*-ignore it: signal-hook installs its own handler over SIG_IGN, so
/// under `nohup` (which sets SIGHUP to SIG_IGN) the watcher would turn a
/// survivable hangup back into a death. Read-only `sigaction` query.
pub(crate) fn signal_is_ignored(sig: i32) -> bool {
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
pub(crate) fn install_signal_handler() -> Result<()> {
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

/// RAII owner for the process-wide Docker cleanup registration. The cidfile
/// is armed before spawn, the child pid is attached immediately afterwards,
/// and dropping an armed registration clears both handles.
pub(super) struct RunRegistration {
    armed: bool,
}

impl RunRegistration {
    #[cfg(test)]
    pub(super) fn detached() -> Self {
        Self { armed: true }
    }

    pub(super) fn new(
        cidfile_path: &Path,
        docker: &DockerCli,
        install_signals: bool,
    ) -> Result<Self> {
        set_cidfile_mode(cidfile_path, docker, install_signals)?;
        Ok(Self { armed: true })
    }

    pub(super) fn attach(&self, pid: u32) {
        set_child(pid);
    }

    pub(super) fn finish(&mut self) -> bool {
        if !self.armed {
            return false;
        }
        self.armed = false;
        finish_child()
    }
}

impl Drop for RunRegistration {
    fn drop(&mut self) {
        if self.armed {
            clear_child();
            self.armed = false;
        }
    }
}

#[cfg(test)]
#[path = "supervision_tests.rs"]
mod tests;
