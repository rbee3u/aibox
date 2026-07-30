//! Signal-aware cleanup for Docker runs.
//!
//! This module owns the process-wide Docker child/cidfile registry used to stop
//! containers on wrapper-only signals and to detect a container that outlives
//! its attached Docker client.
//!
//! ## The signal gap
//!
//! The normal path is explicit: Docker runs as a child rather than an
//! `exec`-replace, and [`finish_child`] runs after `docker run` returns. But the
//! normal path does **not** run when the process is killed by SIGINT (Ctrl-C),
//! SIGTERM, or SIGHUP (closed terminal, dropped SSH session): the default
//! disposition terminates without unwinding. So those signals are routed to a
//! dedicated watcher thread ([`signal_hook::iterator::Signals`]) that stops the
//! container (below) and re-raises the signal so the process still dies with the
//! signal's exit status, falling back to `128 + signal` if the re-raise returns.
//! SIGHUP is watched only when it isn't already ignored — under `nohup` a
//! handler would override the inherited "ignore" and turn a survivable hangup
//! back into a death. Uncatchable termination (for example SIGKILL) cannot run
//! process cleanup, so this is still best-effort rather than durable state.
//!
//! ## Stopping the container
//!
//! Ctrl-C signals the whole foreground process group, but a `kill` aimed at the
//! wrapper alone (CI timeout, service manager) hits only the wrapper — without
//! help, the container would keep running unsupervised after the wrapper died.
//! Forwarding the signal to the `docker run` child covers the no-TTY case (the
//! CLI proxies signals to the container), but a TTY-attached CLI (`-it`) does
//! **not** proxy — killing it verifiably orphans the container. So the watcher
//! also signals the *container* through the daemon: a `docker kill --signal`
//! with the id from `--cidfile` (registered via [`set_cidfile`]), escalating to a
//! plain `docker kill` (SIGKILL) if the agent hasn't exited shortly after —
//! the agent is the container's PID 1, and PID 1 ignores signals it has no
//! handler installed for, so waiting on the graceful signal alone could wait
//! forever.
//!
//! ## Process model
//!
//! The child pid, cidfile, and run state form one process-wide registry. The
//! aibox CLI starts at most one agent run, so this module intentionally does not
//! support concurrent `docker run` children.

use anyhow::{Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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
pub(crate) static RUN_REGISTRY_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn run_registry_test_lock() -> std::sync::MutexGuard<'static, ()> {
    RUN_REGISTRY_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The pid of the running `docker run` child, or 0 when none. The watcher
/// forwards the fatal signal to it: with no TTY the docker CLI proxies the
/// signal to the agent (graceful shutdown); with one it at least exits.
static CHILD_PID: AtomicI32 = AtomicI32::new(0);

/// The `--cidfile` path of the running `docker run`, if any. The watcher reads
/// the container id from it to stop the container through the daemon — the one
/// route that works whether or not the docker CLI has a TTY attached.
static CIDFILE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

fn cidfile() -> &'static Mutex<Option<PathBuf>> {
    CIDFILE.get_or_init(|| Mutex::new(None))
}

/// Register the `--cidfile` of an upcoming `docker run` for signal handling.
/// Call *before* spawning the child: the path is known upfront, and registering
/// it first closes the window where a signal lands after spawn but before any
/// registration — the watcher could then stop the container via the daemon even
/// with no child pid recorded yet. If spawning fails, call [`clear_child`];
/// otherwise register the child with [`set_child`] and finish it with
/// [`finish_child`]. A second registration is rejected while a run is active.
pub fn set_cidfile(cidfile_path: &Path) -> Result<()> {
    install_signal_handler()?;
    let mut registered_cidfile = cidfile().lock().unwrap();
    if RUN_STATE
        .compare_exchange(RUN_IDLE, RUN_ACTIVE, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        anyhow::bail!("another docker run is already registered in this process");
    }
    *registered_cidfile = Some(cidfile_path.to_path_buf());
    Ok(())
}

/// Register the spawned `docker run` child's pid for signal forwarding. Call
/// right after spawn (after [`set_cidfile`]). Once the child has been reaped,
/// call [`finish_child`] so a container that outlived the Docker client is
/// detected before the registration is cleared.
pub fn set_child(pid: u32) {
    CHILD_PID.store(pid as i32, Ordering::SeqCst);
}

/// Abandon a run registration when spawning failed after [`set_cidfile`].
///
/// After a successful spawn, reap the child and call [`finish_child`] instead;
/// clearing directly would skip the lingering-container check.
pub fn clear_child() {
    let mut registered_cidfile = cidfile().lock().unwrap();
    CHILD_PID.store(0, Ordering::SeqCst);
    RUN_STATE.store(RUN_IDLE, Ordering::SeqCst);
    *registered_cidfile = None;
}

/// Finish a successfully spawned child after `wait` returns. An attached Docker
/// CLI can exit while its container remains alive (most visibly via Docker's
/// detach key sequence, but also after some client/daemon disconnects), so use
/// the cidfile to stop a still-running container before unregistering the run.
/// If a fatal signal raced with the wait, clear the now-stale pid, retain the
/// cidfile, and keep this thread alive until the watcher terminates the process
/// after daemon-side cleanup. Returns `true` when a live or uninspectable
/// lingering container required a kill attempt.
pub fn finish_child() -> bool {
    CHILD_PID.store(0, Ordering::SeqCst);
    let stopped_lingering_container = stop_container_left_by_child();
    let mut registered_cidfile = cidfile().lock().unwrap();
    match RUN_STATE.compare_exchange(RUN_ACTIVE, RUN_IDLE, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) | Err(RUN_IDLE) => {
            *registered_cidfile = None;
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
    match container_state(&cid) {
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

    let _ = docker_quiet(&["kill", &cid], DOCKER_KILL_TIMEOUT);
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
        s if s == signal_hook::consts::SIGINT => rustix::process::Signal::Int,
        s if s == signal_hook::consts::SIGHUP => rustix::process::Signal::Hup,
        _ => rustix::process::Signal::Term,
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
fn command_quiet(program: &str, args: &[&str], timeout: Duration) -> CommandOutcome {
    // A pipe can remain open in a subprocess descendant after the command we
    // spawned has exited, making a post-wait `read_to_end` block forever.
    // Capture into a regular temporary file instead: reading a snapshot of a
    // regular file reaches EOF even while an inherited writer is still open.
    let output = match tempfile::NamedTempFile::new() {
        Ok(output) => output,
        Err(_) => return CommandOutcome::Unfinished,
    };
    let child_stdout = match output.reopen() {
        Ok(file) => file,
        Err(_) => return CommandOutcome::Unfinished,
    };
    let spawned = Command::new(program)
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
fn docker_quiet(args: &[&str], timeout: Duration) -> Option<String> {
    match command_quiet("docker", args, timeout) {
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

/// The daemon's view of the container. A wedged daemon is unknown, not stopped:
/// treating it as "done" can leave the container alive. A fast non-zero
/// `inspect` is ambiguous — the id may be gone, or the daemon may be failing —
/// so an exact `container ls` query distinguishes those cases.
fn container_state(cid: &str) -> ContainerState {
    let outcome = command_quiet(
        "docker",
        &["inspect", "-f", "{{.State.Running}}", cid],
        DOCKER_INSPECT_TIMEOUT,
    );
    let state = parse_container_state(&outcome);
    if state != ContainerState::Unknown || !matches!(outcome, CommandOutcome::Failed) {
        return state;
    }

    let filter = format!("id={cid}");
    match command_quiet(
        "docker",
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
    ) {
        CommandOutcome::Succeeded(output) if output.trim().is_empty() => ContainerState::Stopped,
        CommandOutcome::Succeeded(_) | CommandOutcome::Failed | CommandOutcome::Unfinished => {
            ContainerState::Unknown
        }
    }
}

/// Stop the active run without letting a slow cidfile create window orphan the
/// container. Prefer the daemon-side container id path; if the id is not ready
/// yet, signal the Docker CLI child, then keep polling briefly for a late
/// cidfile so a just-created TTY container still gets killed through the daemon.
fn stop_active_run(sig: i32) {
    if let Some(cid) = wait_current_cid(CIDFILE_WAIT) {
        stop_container_id(sig, &cid);
        signal_child(sig);
        return;
    }

    signal_child(sig);
    if let Some(cid) = wait_current_cid(LATE_CIDFILE_WAIT) {
        stop_container_id(sig, &cid);
    }
}

/// Stop one container through the daemon: deliver `sig` to its PID 1 (what
/// `--sig-proxy` would have done, had the CLI not had a TTY), then escalate to a
/// plain `docker kill` (SIGKILL) if it lingers — an agent without a handler for
/// the signal never exits on it as PID 1. The 10s grace mirrors `docker stop`'s
/// default.
///
/// On the signal path, the main thread normally stays blocked in `child.wait()`
/// while the watcher performs this escalation; that is why the watcher stops
/// the container *before* touching the CLI child. The post-wait orphan check
/// takes a separate immediate-kill path because no attached client remains.
fn stop_container_id(sig: i32, cid: &str) {
    let name = match sig {
        s if s == signal_hook::consts::SIGINT => "INT",
        s if s == signal_hook::consts::SIGHUP => "HUP",
        _ => "TERM",
    };
    let _ = docker_quiet(&["kill", "--signal", name, cid], DOCKER_KILL_TIMEOUT);
    if container_state(cid) == ContainerState::Stopped {
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
        if container_state(cid) == ContainerState::Stopped {
            return;
        }
    }
    let _ = docker_quiet(&["kill", cid], DOCKER_KILL_TIMEOUT);
}

/// True if `sig` is currently ignored (SIG_IGN). Watching an ignored signal
/// would *un*-ignore it: signal-hook installs its own handler over SIG_IGN, so
/// under `nohup` (which sets SIGHUP to SIG_IGN) the watcher would turn a
/// survivable hangup back into a death. Read-only `sigaction` query.
fn signal_is_ignored(sig: i32) -> bool {
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
            Err(e) => {
                for id in registrations {
                    signal_hook::low_level::unregister(id);
                }
                return Err(e).context("install signal state handler");
            }
        }
    }

    let mut signals = match signal_hook::iterator::Signals::new(&watched) {
        Ok(s) => s,
        Err(e) => {
            for id in registrations {
                signal_hook::low_level::unregister(id);
            }
            return Err(e).context("install signal cleanup handler");
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
        Err(e) => {
            for id in registrations {
                signal_hook::low_level::unregister(id);
            }
            return Err(e).context("spawn signal cleanup thread");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::EnvGuard;

    struct CidfileGuard;

    impl Drop for CidfileGuard {
        fn drop(&mut self) {
            clear_child();
        }
    }

    fn stable_tempdir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("aibox-test.")
            .tempdir()
            .unwrap()
    }

    #[cfg(unix)]
    const SIGNAL_HELPER_DIR: &str = "AIBOX_TEST_SIGNAL_HELPER_DIR";
    #[cfg(unix)]
    const IGNORED_HUP_HELPER_DIR: &str = "AIBOX_TEST_IGNORED_HUP_HELPER_DIR";

    #[cfg(unix)]
    #[test]
    fn ignored_hup_helper_process() {
        let Some(dir) = std::env::var_os(IGNORED_HUP_HELPER_DIR) else {
            return;
        };
        let dir = PathBuf::from(dir);

        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = libc::SIG_IGN;
            libc::sigemptyset(&mut action.sa_mask);
            let rc = libc::sigaction(signal_hook::consts::SIGHUP, &action, std::ptr::null_mut());
            assert_eq!(
                rc,
                0,
                "set SIGHUP to SIG_IGN: {}",
                std::io::Error::last_os_error()
            );
        }

        assert!(signal_is_ignored(signal_hook::consts::SIGHUP));
        install_signal_handler().unwrap();
        assert!(
            signal_is_ignored(signal_hook::consts::SIGHUP),
            "installing cleanup handlers must not un-ignore inherited SIGHUP"
        );

        unsafe {
            libc::raise(signal_hook::consts::SIGHUP);
        }
        std::thread::sleep(Duration::from_millis(200));
        std::fs::write(dir.join("survived"), "survived\n").unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn ignored_sighup_stays_ignored_when_handlers_are_installed() {
        let scratch = stable_tempdir();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("creds::tests::ignored_hup_helper_process")
            .env(IGNORED_HUP_HELPER_DIR, scratch.path())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();

        assert!(
            status.success(),
            "helper should survive ignored SIGHUP, got {status:?}"
        );
        assert!(
            scratch.path().join("survived").exists(),
            "helper did not continue after raising ignored SIGHUP"
        );
    }

    // Arm the same daemon-side cleanup handle as a real run, report ready, and
    // block until the parent delivers the signal under test.
    #[cfg(unix)]
    #[test]
    fn signal_helper_process() {
        let Some(dir) = std::env::var_os(SIGNAL_HELPER_DIR) else {
            return;
        };
        let dir = PathBuf::from(dir);
        let cid_path = dir.join("cid");
        std::fs::write(&cid_path, "signal-container\n").unwrap();
        // Installs the watcher thread and marks the run active, exactly as
        // `docker::run` does before spawning the Docker CLI.
        set_cidfile(&cid_path).unwrap();

        std::fs::write(dir.join("ready"), "ready\n").unwrap();
        // Park until the watcher exits us.
        std::thread::sleep(Duration::from_secs(60));
    }

    // Run the helper with a stubbed Docker, wait for its cidfile, then signal it.
    #[cfg(unix)]
    fn run_signal_helper(sig: i32) -> (tempfile::TempDir, std::process::ExitStatus) {
        let scratch = stable_tempdir();
        let fake_docker = scratch.path().join("bin");
        std::fs::create_dir_all(&fake_docker).unwrap();
        write_signal_fake_docker(&fake_docker);
        let ready = scratch.path().join("ready");

        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("creds::tests::signal_helper_process")
            .env(SIGNAL_HELPER_DIR, scratch.path())
            .env("PATH", &fake_docker)
            .env("AIBOX_FAKE_DOCKER_LOG", scratch.path().join("docker.log"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        let started = Instant::now();
        while !ready.exists() {
            if started.elapsed() > Duration::from_secs(10) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("signal helper never armed its cidfile");
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let pid = rustix::process::Pid::from_raw(child.id() as i32).unwrap();
        let rsig = match sig {
            s if s == signal_hook::consts::SIGINT => rustix::process::Signal::Int,
            s if s == signal_hook::consts::SIGHUP => rustix::process::Signal::Hup,
            _ => rustix::process::Signal::Term,
        };
        rustix::process::kill_process(pid, rsig).unwrap();

        let status = child.wait().unwrap();
        (scratch, status)
    }

    // Exercise the fatal-signal path in a subprocess because unwinding and
    // `Drop` cannot cover it.
    #[cfg(unix)]
    #[test]
    fn fatal_signal_stops_the_container() {
        use std::os::unix::process::ExitStatusExt;

        for sig in [
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGHUP,
        ] {
            let (scratch, status) = run_signal_helper(sig);

            // The container is stopped through the daemon — the only route that
            // works when the Docker CLI has a TTY and does not proxy signals.
            let log =
                std::fs::read_to_string(scratch.path().join("docker.log")).unwrap_or_default();
            assert!(
                log.contains("signal-container"),
                "sig {sig}: container was not stopped via the daemon; docker log:\n{log}"
            );

            // Death still looks like the signal to the caller's shell, whether
            // by re-raise or the 128+n fallback.
            let died_of_signal = status.signal() == Some(sig);
            assert!(
                died_of_signal || status.code() == Some(128 + sig),
                "sig {sig}: exit status must reflect the signal, got {status:?}"
            );
        }
    }

    #[cfg(unix)]
    fn write_signal_fake_docker(dir: &Path) {
        crate::testutil::write_stub_script(
            dir,
            "docker",
            r#"#!/bin/sh
if [ "$1" = "kill" ] && [ -n "$AIBOX_FAKE_DOCKER_KILL_START_DELAY" ]; then
    sleep "$AIBOX_FAKE_DOCKER_KILL_START_DELAY"
fi
if [ -n "$AIBOX_FAKE_DOCKER_LOG" ]; then
    printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
fi
case "$1" in
    kill)
        exit 0
        ;;
    inspect)
        printf 'false\n'
        exit 0
        ;;
    *)
        exit 99
        ;;
esac
"#,
        );
    }

    // Report a running container for a configurable number of inspections so
    // tests can reach the grace and escalation paths.
    #[cfg(unix)]
    fn write_running_container_docker(dir: &Path) {
        crate::testutil::write_stub_script(
            dir,
            "docker",
            r#"#!/bin/sh
if [ -n "$AIBOX_FAKE_DOCKER_LOG" ]; then
    printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
fi
case "$1" in
    kill)
        exit 0
        ;;
    inspect)
        want="$AIBOX_FAKE_DOCKER_RUNNING_INSPECTS"
        if [ "$want" = "always" ]; then
            printf 'true\n'
            exit 0
        fi
        seen=0
        if [ -f "$AIBOX_FAKE_DOCKER_INSPECT_COUNT" ]; then
            seen=$(cat "$AIBOX_FAKE_DOCKER_INSPECT_COUNT")
        fi
        seen=$((seen + 1))
        printf '%s' "$seen" > "$AIBOX_FAKE_DOCKER_INSPECT_COUNT"
        if [ "$seen" -le "${want:-0}" ]; then
            printf 'true\n'
        else
            printf 'false\n'
        fi
        exit 0
        ;;
    *)
        exit 99
        ;;
esac
"#,
        );
    }

    // SIGNAL_COUNT is process-global, so tests that fake a signal restore it.
    struct SignalCountGuard(usize);

    impl SignalCountGuard {
        fn set(value: usize) -> Self {
            let old = SIGNAL_COUNT.swap(value, Ordering::SeqCst);
            SignalCountGuard(old)
        }
    }

    impl Drop for SignalCountGuard {
        fn drop(&mut self) {
            SIGNAL_COUNT.store(self.0, Ordering::SeqCst);
        }
    }

    #[cfg(unix)]
    #[test]
    fn stop_container_id_waits_out_a_container_that_stops_on_the_signal() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = run_registry_test_lock();
        let scratch = stable_tempdir();
        let log_path = scratch.path().join("docker.log");
        let count_path = scratch.path().join("inspects");
        let fake_docker = stable_tempdir();
        write_running_container_docker(fake_docker.path());
        let _path = EnvGuard::prepend_path(fake_docker.path());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());
        let _count = EnvGuard::set("AIBOX_FAKE_DOCKER_INSPECT_COUNT", count_path.as_os_str());
        // Running on the first inspect, stopped on the next: the container took
        // the signal, just not instantly.
        let _running = EnvGuard::set("AIBOX_FAKE_DOCKER_RUNNING_INSPECTS", "1");

        let started = Instant::now();
        stop_container_id(signal_hook::consts::SIGTERM, "graceful-container");

        assert!(
            started.elapsed() < CONTAINER_GRACE,
            "a container that stops during the grace window must not wait it out"
        );
        let log = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            log.contains("kill --signal TERM graceful-container"),
            "the graceful signal goes through the daemon: {log}"
        );
        assert!(
            !log.lines().any(|l| l == "kill graceful-container"),
            "a container that exited on the signal must not also be SIGKILLed:\n{log}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stop_container_id_escalates_to_sigkill_on_a_second_signal() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = run_registry_test_lock();
        let scratch = stable_tempdir();
        let log_path = scratch.path().join("docker.log");
        let count_path = scratch.path().join("inspects");
        let fake_docker = stable_tempdir();
        write_running_container_docker(fake_docker.path());
        let _path = EnvGuard::prepend_path(fake_docker.path());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());
        let _count = EnvGuard::set("AIBOX_FAKE_DOCKER_INSPECT_COUNT", count_path.as_os_str());
        let _running = EnvGuard::set("AIBOX_FAKE_DOCKER_RUNNING_INSPECTS", "always");
        // A second delivered signal (Ctrl-C again, or a supervisor re-kill).
        let _signals = SignalCountGuard::set(2);

        let started = Instant::now();
        stop_container_id(signal_hook::consts::SIGINT, "stubborn-container");

        assert!(
            started.elapsed() < CONTAINER_GRACE,
            "a second signal must skip the rest of the grace wait"
        );
        let log = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            log.contains("kill --signal INT stubborn-container"),
            "SIGINT maps to the container's INT: {log}"
        );
        assert!(
            log.lines().any(|l| l == "kill stubborn-container"),
            "a container that ignored the signal must be SIGKILLed:\n{log}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stop_container_id_forwards_each_signal_under_its_own_name() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = run_registry_test_lock();
        let fake_docker = stable_tempdir();
        write_signal_fake_docker(fake_docker.path());
        let _path = EnvGuard::prepend_path(fake_docker.path());

        for (sig, name) in [
            (signal_hook::consts::SIGINT, "INT"),
            (signal_hook::consts::SIGHUP, "HUP"),
            (signal_hook::consts::SIGTERM, "TERM"),
        ] {
            let scratch = stable_tempdir();
            let log_path = scratch.path().join("docker.log");
            let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());

            stop_container_id(sig, "named-container");

            let log = std::fs::read_to_string(&log_path).unwrap();
            assert!(
                log.contains(&format!("kill --signal {name} named-container")),
                "sig {sig} must reach the container as {name}:\n{log}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn signal_child_forwards_each_supported_signal_to_the_registered_process() {
        use std::os::unix::process::ExitStatusExt;

        let _run_lock = run_registry_test_lock();
        let _guard = CidfileGuard;

        for signal in [
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGHUP,
            signal_hook::consts::SIGTERM,
        ] {
            let mut child = std::process::Command::new("/bin/sleep")
                .arg("60")
                .spawn()
                .unwrap();
            set_child(child.id());

            signal_child(signal);

            let deadline = Instant::now() + Duration::from_secs(2);
            let status = loop {
                if let Some(status) = child.try_wait().unwrap() {
                    break status;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("signal {signal} did not terminate the registered child");
                }
                std::thread::sleep(Duration::from_millis(10));
            };
            assert_eq!(
                status.signal(),
                Some(signal),
                "the Docker CLI child must receive the wrapper's original signal"
            );
        }
    }

    #[test]
    fn watcher_commands_are_bounded() {
        // The `/bin/sh` helpers below resolve `sleep` through `$PATH`, so this
        // must hold the env lock even though it installs no guard of its own: a
        // parallel test that replaces `$PATH` with a stub directory would make
        // `sleep` unresolvable and turn an expected timeout into a fast exit 127.
        let _env_lock = crate::test_env_lock();

        // Worst case is the late-cidfile path in `stop_active_run`: the first
        // bounded wait fails (CIDFILE_WAIT), then after signalling the child the
        // longer late wait succeeds (LATE_CIDFILE_WAIT), then `stop_container_id`
        // runs its full graceful/escalating cleanup.
        let container_state_timeout = DOCKER_INSPECT_TIMEOUT + DOCKER_INSPECT_TIMEOUT;
        let worst_case_stop_container_id = DOCKER_KILL_TIMEOUT
            + container_state_timeout
            + CONTAINER_GRACE
            + CONTAINER_POLL_INTERVAL
            + container_state_timeout
            + DOCKER_KILL_TIMEOUT;
        let worst_case_signal_cleanup =
            CIDFILE_WAIT + LATE_CIDFILE_WAIT + worst_case_stop_container_id;
        assert!(
            SIGNAL_FINISH_WAIT > worst_case_signal_cleanup,
            "the main thread must not exit before the watcher can finish its bounded cleanup"
        );

        assert!(matches!(
            command_quiet("/bin/sh", &["-c", "printf ok"], Duration::from_secs(1)),
            CommandOutcome::Succeeded(out) if out == "ok"
        ));

        let inherited_stdout_started = Instant::now();
        assert!(matches!(
            command_quiet(
                "/bin/sh",
                &["-c", "sleep 5 & printf ok"],
                Duration::from_secs(1)
            ),
            CommandOutcome::Succeeded(out) if out == "ok"
        ));
        assert!(
            inherited_stdout_started.elapsed() < Duration::from_secs(2),
            "an inherited stdout handle must not outlive the command timeout"
        );

        // A fast non-zero exit is a definitive failure, distinct from a timeout.
        assert!(matches!(
            command_quiet("/bin/sh", &["-c", "exit 1"], Duration::from_secs(1)),
            CommandOutcome::Failed
        ));

        let oversized_output = format!(
            "dd if=/dev/zero bs={} count=1 2>/dev/null",
            COMMAND_OUTPUT_LIMIT + 1
        );
        assert!(
            matches!(
                command_quiet(
                    "/bin/sh",
                    &["-c", &oversized_output],
                    Duration::from_secs(1)
                ),
                CommandOutcome::Unfinished
            ),
            "watcher helpers must not retain unbounded command output"
        );

        let started = Instant::now();
        let out = command_quiet("/bin/sh", &["-c", "sleep 5"], Duration::from_millis(50));

        assert!(
            matches!(out, CommandOutcome::Unfinished),
            "timed-out command should be treated as best-effort failure"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "timeout should not wait for the child script to finish"
        );
    }

    #[test]
    fn wait_current_cid_reads_delayed_cidfile() {
        let _run_lock = run_registry_test_lock();
        let _guard = CidfileGuard;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cid");
        *cidfile().lock().unwrap() = Some(path.clone());

        let writer = std::thread::spawn({
            let path = path.clone();
            move || {
                std::thread::sleep(Duration::from_millis(50));
                std::fs::write(path, "abc123\n").unwrap();
            }
        });

        let got = wait_current_cid(Duration::from_secs(1));
        writer.join().unwrap();

        assert_eq!(got.as_deref(), Some("abc123"));
    }

    #[cfg(unix)]
    #[test]
    fn stop_active_run_kills_late_cidfile_container() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = run_registry_test_lock();
        let _guard = CidfileGuard;
        let scratch = stable_tempdir();
        let cid_path = scratch.path().join("cid");
        let log_path = scratch.path().join("docker.log");
        let fake_docker = stable_tempdir();
        write_signal_fake_docker(fake_docker.path());
        *cidfile().lock().unwrap() = Some(cid_path.clone());
        let _path = EnvGuard::prepend_path(fake_docker.path());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());
        let _kill_delay = EnvGuard::set("AIBOX_FAKE_DOCKER_KILL_START_DELAY", "1.2");

        let writer = std::thread::spawn(move || {
            std::thread::sleep(CIDFILE_WAIT + Duration::from_millis(100));
            std::fs::write(cid_path, "late-container\n").unwrap();
        });

        stop_active_run(signal_hook::consts::SIGTERM);
        writer.join().unwrap();

        let log = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            log.contains("kill --signal TERM late-container"),
            "late cidfile should still trigger daemon-side kill; log:\n{log}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn finish_child_kills_a_container_that_outlived_its_detached_client() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = run_registry_test_lock();
        let _guard = CidfileGuard;
        let scratch = stable_tempdir();
        let cid_path = scratch.path().join("cid");
        let log_path = scratch.path().join("docker.log");
        let fake_docker = stable_tempdir();
        write_running_container_docker(fake_docker.path());
        std::fs::write(&cid_path, "detached-container\n").unwrap();
        let _path = EnvGuard::prepend_path(fake_docker.path());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());
        // Report running forever: the client detached but the container lives on.
        let _running = EnvGuard::set("AIBOX_FAKE_DOCKER_RUNNING_INSPECTS", "always");
        // Arm the run exactly as `docker::run` does before spawning the client.
        set_cidfile(&cid_path).unwrap();

        let stopped = finish_child();

        assert!(
            stopped,
            "a still-running container after a detached client must be reported killed"
        );
        let log = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            log.lines().any(|l| l == "kill detached-container"),
            "the lingering container must be SIGKILLed by id:\n{log}"
        );
    }

    #[test]
    fn a_second_active_run_registration_is_rejected_without_losing_the_first() {
        let _run_lock = run_registry_test_lock();
        let _guard = CidfileGuard;
        let first = stable_tempdir();
        let second = stable_tempdir();
        let first_cid = first.path().join("cid");
        let second_cid = second.path().join("cid");
        std::fs::write(&first_cid, "first-container\n").unwrap();
        std::fs::write(&second_cid, "second-container\n").unwrap();

        set_cidfile(&first_cid).unwrap();
        let error = set_cidfile(&second_cid).unwrap_err().to_string();

        assert!(error.contains("another docker run is already registered"));
        assert_eq!(
            current_cid().as_deref(),
            Some("first-container"),
            "a rejected registration must not overwrite the active run's cleanup handle"
        );
    }

    #[cfg(unix)]
    #[test]
    fn finish_child_leaves_a_cleanly_exited_run_alone() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = run_registry_test_lock();
        let _guard = CidfileGuard;
        let scratch = stable_tempdir();
        let cid_path = scratch.path().join("cid");
        let log_path = scratch.path().join("docker.log");
        let fake_docker = stable_tempdir();
        // This stub's `inspect` always reports stopped, mimicking a `--rm`
        // container whose id no longer resolves.
        write_signal_fake_docker(fake_docker.path());
        std::fs::write(&cid_path, "gone-container\n").unwrap();
        let _path = EnvGuard::prepend_path(fake_docker.path());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());
        set_cidfile(&cid_path).unwrap();

        let started = Instant::now();
        let stopped = finish_child();

        assert!(
            !stopped,
            "a container that already exited must not be reported killed"
        );
        assert!(
            started.elapsed() < CONTAINER_GRACE,
            "a clean exit must not enter the grace wait"
        );
        let log = std::fs::read_to_string(&log_path).unwrap_or_default();
        assert!(
            !log.lines().any(|l| l.starts_with("kill")),
            "a cleanly exited run must trigger no docker kill:\n{log}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn finish_child_kills_when_container_state_is_unknown() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = run_registry_test_lock();
        let _guard = CidfileGuard;
        let scratch = stable_tempdir();
        let cid_path = scratch.path().join("cid");
        let log_path = scratch.path().join("docker.log");
        let fake_docker = stable_tempdir();
        crate::testutil::write_stub_script(
            fake_docker.path(),
            "docker",
            r#"#!/bin/sh
if [ -n "$AIBOX_FAKE_DOCKER_LOG" ]; then
    printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
fi
case "$1" in
    kill)
        exit 0
        ;;
    inspect)
        printf 'unknown\n'
        exit 0
        ;;
    *)
        exit 99
        ;;
esac
"#,
        );
        std::fs::write(&cid_path, "uncertain-container\n").unwrap();
        let _path = EnvGuard::prepend_path(fake_docker.path());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.to_str().unwrap());
        set_cidfile(&cid_path).unwrap();

        let stopped = finish_child();

        assert!(
            stopped,
            "an unknown daemon state after client exit must be treated as unclean"
        );
        let log = std::fs::read_to_string(&log_path).unwrap();
        assert!(
            log.lines().any(|line| line == "kill uncertain-container"),
            "unknown state should still trigger a daemon-side kill:\n{log}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn finish_child_treats_a_missing_docker_cli_as_unclean_and_resets_the_registry() {
        let _env_lock = crate::test_env_lock();
        let _run_lock = run_registry_test_lock();
        let _guard = CidfileGuard;
        let scratch = stable_tempdir();
        let cid_path = scratch.path().join("cid");
        std::fs::write(&cid_path, "uninspectable-container\n").unwrap();
        let empty_path = stable_tempdir();
        let _path = EnvGuard::set("PATH", empty_path.path().as_os_str());
        set_cidfile(&cid_path).unwrap();

        assert!(
            finish_child(),
            "losing the Docker CLI after a child exit must be treated as an unclean run"
        );
        assert!(
            !finish_child(),
            "finishing the run must clear the process-global registration"
        );
    }

    #[cfg(unix)]
    #[test]
    fn failed_inspect_distinguishes_a_missing_container_from_a_daemon_failure() {
        let _env_lock = crate::test_env_lock();
        let fake_docker = stable_tempdir();
        let log_path = fake_docker.path().join("docker.log");
        crate::testutil::write_stub_script(
            fake_docker.path(),
            "docker",
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
case "$1" in
    inspect)
        exit 1
        ;;
    container)
        if [ "$AIBOX_FAKE_DOCKER_STATE" = "missing" ]; then
            exit 0
        fi
        exit 1
        ;;
    *)
        exit 99
        ;;
esac
"#,
        );
        let _path = EnvGuard::prepend_path(fake_docker.path());
        let _log = EnvGuard::set("AIBOX_FAKE_DOCKER_LOG", log_path.as_os_str());

        {
            let _state = EnvGuard::set("AIBOX_FAKE_DOCKER_STATE", "missing");
            assert_eq!(
                container_state("gone-container"),
                ContainerState::Stopped,
                "an exact empty container list confirms that the id is gone"
            );
        }

        {
            let _state = EnvGuard::set("AIBOX_FAKE_DOCKER_STATE", "daemon-error");
            assert_eq!(
                container_state("possibly-running-container"),
                ContainerState::Unknown,
                "two failed daemon queries must not be mistaken for a stopped container"
            );
        }

        let log = std::fs::read_to_string(log_path).unwrap();
        for cid in ["gone-container", "possibly-running-container"] {
            assert!(
                log.lines()
                    .any(|line| line == format!("inspect -f {{{{.State.Running}}}} {cid}")),
                "container state must inspect the exact id first:\n{log}"
            );
            assert!(
                log.lines().any(|line| {
                    line == format!("container ls --all --quiet --no-trunc --filter id={cid}")
                }),
                "a failed inspect must use one exact, non-truncated id filter:\n{log}"
            );
        }
    }

    #[test]
    fn container_state_parser_distinguishes_running_stopped_unknown() {
        assert_eq!(
            parse_container_state(&CommandOutcome::Succeeded("true\n".into())),
            ContainerState::Running
        );
        assert_eq!(
            parse_container_state(&CommandOutcome::Succeeded("false\n".into())),
            ContainerState::Stopped
        );
        assert_eq!(
            parse_container_state(&CommandOutcome::Succeeded(String::new())),
            ContainerState::Unknown
        );
        assert_eq!(
            parse_container_state(&CommandOutcome::Succeeded("docker error".into())),
            ContainerState::Unknown
        );
        assert_eq!(
            parse_container_state(&CommandOutcome::Failed),
            ContainerState::Unknown
        );
        // A timeout / unspawnable docker is not an answer: stay Unknown so a
        // wedged daemon can't be mistaken for a stopped container.
        assert_eq!(
            parse_container_state(&CommandOutcome::Unfinished),
            ContainerState::Unknown
        );
    }
}
