//! Signal-aware cleanup for Docker runs and temporary files.
//!
//! The current run path no longer stages provider credentials: config is applied
//! persistently before launch. This module still owns the Docker child/cidfile
//! registry used to stop containers on wrapper-only signals, plus the older
//! temp-file and fixed-placeholder guards used by tests and available for future
//! ephemeral resources.
//!
//! ## The signal gap
//!
//! Rust's `Drop` covers the normal path (the guard drops after `docker run`
//! returns), because Docker runs as a child rather than an `exec`-replace. But
//! `Drop` does **not** run when the process is killed by SIGINT (Ctrl-C),
//! SIGTERM, or SIGHUP (closed terminal, dropped SSH session): the default
//! disposition terminates without unwinding. So those signals are routed to a
//! dedicated watcher thread ([`signal_hook::iterator::Signals`]) that unlinks
//! every registered staged path, stops the container (below), and re-raises the
//! signal so the process still dies with the signal's exit status, falling back
//! to `128 + signal` if the re-raise returns. SIGHUP is watched only when it
//! isn't already ignored — under `nohup` a handler would override the inherited
//! "ignore" and turn a survivable hangup back into a death. Between `Drop` and
//! the watcher, a staged credential is removed when the run finishes, errors,
//! or receives a handled fatal signal. Uncatchable termination (for example
//! SIGKILL) cannot run process cleanup, so anything registered here is still
//! best-effort rather than durable state.
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

use anyhow::{Context, Result};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
// Main-thread fallback when a signal raced with `docker run` exiting. Covers
// cid discovery on the *late-cidfile* path (the first bounded wait fails, then
// the longer late wait succeeds), a graceful kill + inspect, the full grace
// window (including one last bounded inspect), the final SIGKILL, and two
// seconds of scheduling slack — so the main thread never exits before the
// watcher can finish its worst-case bounded cleanup.
const SIGNAL_FINISH_WAIT: Duration = Duration::from_secs(25);
#[cfg(test)]
const AUTH_LOCK_WAIT: Duration = Duration::from_millis(750);
#[cfg(not(test))]
const AUTH_LOCK_WAIT: Duration = Duration::from_secs(2);
const DROP_LOCK_WAIT: Duration = Duration::from_millis(100);
const LOCK_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Paths to clean if a fatal signal arrives. The watcher thread runs in normal
/// (non-signal-handler) context — signal-hook's internal handler only sets a
/// flag — so a plain `Mutex` is fine; nothing here must be async-signal-safe.
static PENDING: OnceLock<Mutex<Vec<PendingCleanup>>> = OnceLock::new();

enum PendingCleanup {
    /// A unique temp file owned completely by this process.
    File(PathBuf),
    /// A fixed path we only own while it still contains this exact placeholder.
    Placeholder {
        path: PathBuf,
        contents: String,
        lock_path: PathBuf,
    },
}

impl PendingCleanup {
    fn path(&self) -> &Path {
        match self {
            Self::File(path) | Self::Placeholder { path, .. } => path,
        }
    }

    fn cleanup(&self) {
        match self {
            Self::File(path) => {
                let _ = std::fs::remove_file(path);
            }
            Self::Placeholder {
                path,
                contents,
                lock_path,
            } => {
                if !real_mount_target_parent_exists(path).unwrap_or(false) {
                    return;
                }
                let _lock = if lock_held_by_this_process(lock_path) {
                    None
                } else {
                    match FileLock::try_acquire(lock_path) {
                        Ok(Some(lock)) => Some(lock),
                        _ => return,
                    }
                };
                if placeholder_matches(path, contents) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
}

/// Match an owned placeholder only when the path itself is a small regular
/// file. Never follow a symlink or try to read a FIFO/socket: the fixed path is
/// inside a container-writable profile home and may have changed while a run
/// was active.
fn placeholder_matches(path: &Path, expected: &str) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.file_type().is_file() || meta.len() != expected.len() as u64 {
        return false;
    }

    let Ok(mut file) = open_placeholder_for_read(path) else {
        return false;
    };
    let Ok(meta) = file.metadata() else {
        return false;
    };
    if !meta.file_type().is_file() || meta.len() != expected.len() as u64 {
        return false;
    }

    let mut found = Vec::with_capacity(expected.len());
    file.read_to_end(&mut found)
        .is_ok_and(|_| found == expected.as_bytes())
}

#[cfg(unix)]
fn open_placeholder_for_read(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_placeholder_for_read(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().read(true).open(path)
}

fn mount_target_parent(path: &Path) -> Result<&Path> {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .with_context(|| format!("mount target has no parent: {}", path.display()))
}

fn real_mount_target_parent_exists(path: &Path) -> Result<bool> {
    let parent = mount_target_parent(path)?;
    crate::profile::real_dir_exists(parent, "mount target parent")
}

fn require_real_mount_target_parent(path: &Path) -> Result<()> {
    let parent = mount_target_parent(path)?;
    if crate::profile::real_dir_exists(parent, "mount target parent")? {
        Ok(())
    } else {
        anyhow::bail!("mount target parent does not exist: {}", parent.display())
    }
}

fn require_real_lock_parent(path: &Path) -> Result<()> {
    let parent = mount_target_parent(path)?;
    crate::profile::ensure_real_dir(parent, "auth lock directory")
}

/// Advisory file lock used only to protect the short window where Codex's
/// fixed auth.json mount target must exist before Docker establishes the bind.
struct FileLock {
    path: PathBuf,
    file: std::fs::File,
}

impl FileLock {
    fn acquire(path: &Path) -> Result<Self> {
        Self::acquire_with_timeout(path, AUTH_LOCK_WAIT)
    }

    fn acquire_for_drop(path: &Path) -> Result<Self> {
        Self::acquire_with_timeout(path, DROP_LOCK_WAIT)
    }

    fn acquire_with_timeout(path: &Path, timeout: Duration) -> Result<Self> {
        let started = Instant::now();
        loop {
            match Self::try_acquire(path)? {
                Some(lock) => return Ok(lock),
                None if started.elapsed() >= timeout => {
                    anyhow::bail!(
                        "auth mount target is busy (lock {}); wait for the other aibox run to start, or use a different profile with -p",
                        path.display()
                    );
                }
                None => std::thread::sleep(LOCK_POLL_INTERVAL),
            }
        }
    }

    fn try_acquire(path: &Path) -> Result<Option<Self>> {
        let file = open_lock_file(path)?;
        if !lock_file(&file, false).with_context(|| format!("lock {}", path.display()))? {
            return Ok(None);
        }
        let path = path.to_path_buf();
        HELD_LOCKS.lock().unwrap().push(path.clone());
        Ok(Some(FileLock { path, file }))
    }
}

fn lock_held_by_this_process(path: &Path) -> bool {
    HELD_LOCKS.lock().unwrap().iter().any(|p| p == path)
}

#[cfg(unix)]
fn open_lock_file(path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut opts = std::fs::OpenOptions::new();
    opts.read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW);
    opts.open(path)
        .with_context(|| format!("open lock file {}", path.display()))
}

#[cfg(not(unix))]
fn open_lock_file(path: &Path) -> Result<std::fs::File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.read(true).write(true).create(true);
    opts.open(path)
        .with_context(|| format!("open lock file {}", path.display()))
}

#[cfg(unix)]
fn lock_file(file: &std::fs::File, blocking: bool) -> Result<bool> {
    use std::os::fd::AsRawFd;

    let op = if blocking {
        libc::LOCK_EX
    } else {
        libc::LOCK_EX | libc::LOCK_NB
    };
    let rc = unsafe { libc::flock(file.as_raw_fd(), op) };
    if rc == 0 {
        return Ok(true);
    }
    let e = std::io::Error::last_os_error();
    if !blocking
        && matches!(
            e.raw_os_error(),
            Some(code) if code == libc::EWOULDBLOCK || code == libc::EAGAIN
        )
    {
        return Ok(false);
    }
    Err(e).context("flock")
}

#[cfg(not(unix))]
fn lock_file(_file: &std::fs::File, _blocking: bool) -> Result<bool> {
    Ok(true)
}

impl Drop for FileLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let _ = unsafe { libc::flock(self.file.as_raw_fd(), libc::LOCK_UN) };
        }
        if let Ok(mut locks) = HELD_LOCKS.lock() {
            if let Some(pos) = locks.iter().position(|p| p == &self.path) {
                locks.remove(pos);
            }
        }
    }
}

fn create_placeholder_file(path: &Path, placeholder: &str) -> Result<()> {
    let parent = mount_target_parent(path)?;
    let mut replacement = tempfile::Builder::new()
        .prefix(".aibox-placeholder.")
        .tempfile_in(parent)
        .with_context(|| format!("prepare mount target {}", path.display()))?;
    crate::profile::set_600(replacement.path())?;
    replacement
        .write_all(placeholder.as_bytes())
        .with_context(|| format!("write mount target {}", path.display()))?;
    replacement
        .as_file()
        .sync_all()
        .with_context(|| format!("sync mount target {}", path.display()))?;
    replacement
        .persist_noclobber(path)
        .map(|_| ())
        .map_err(|e| e.error)
        .with_context(|| format!("pre-create mount target {}", path.display()))
}

/// Serializes staged-file creation with signal/test cleanup. A signal arriving
/// while a credential file is being armed waits until the path is registered
/// and contents are written, then unlinks it.
static STAGING: Mutex<()> = Mutex::new(());
static HELD_LOCKS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Whether the watcher thread is up. A `Mutex<bool>` rather than a `OnceLock`
/// so a failed install (Signals::new or thread spawn error) isn't remembered as
/// "installed" — the next staging call gets to retry instead of silently
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
        .unwrap_or_else(|e| e.into_inner())
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
/// with no child pid recorded yet. Pair with [`clear_child`].
pub fn set_cidfile(cidfile_path: &Path) -> Result<()> {
    install_signal_handler()?;
    *cidfile().lock().unwrap() = Some(cidfile_path.to_path_buf());
    RUN_STATE.store(RUN_ACTIVE, Ordering::SeqCst);
    Ok(())
}

/// Register the spawned `docker run` child's pid for signal forwarding. Call
/// right after spawn (after [`set_cidfile`]); pair with [`clear_child`] once
/// the child has been reaped.
pub fn set_child(pid: u32) {
    CHILD_PID.store(pid as i32, Ordering::SeqCst);
}

/// Forget the child and its cidfile (it exited and was reaped, so its pid may
/// be recycled). Also the cleanup for a spawn that failed after [`set_cidfile`].
pub fn clear_child() {
    CHILD_PID.store(0, Ordering::SeqCst);
    RUN_STATE.store(RUN_IDLE, Ordering::SeqCst);
    *cidfile().lock().unwrap() = None;
}

/// Finish a successfully spawned child after `wait` returns. An attached Docker
/// CLI can exit while its container remains alive (most visibly via Docker's
/// detach key sequence, but also after some client/daemon disconnects), so use
/// the cidfile to stop a still-running container before unregistering the run.
/// If a fatal signal raced with the wait, clear the now-stale pid, retain the
/// cidfile, and keep this thread alive until the watcher terminates the process
/// after daemon-side cleanup.
pub fn finish_child() -> bool {
    CHILD_PID.store(0, Ordering::SeqCst);
    let stopped_lingering_container = stop_container_left_by_child();
    match RUN_STATE.compare_exchange(RUN_ACTIVE, RUN_IDLE, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) | Err(RUN_IDLE) => {
            *cidfile().lock().unwrap() = None;
            stopped_lingering_container
        }
        Err(RUN_SIGNALLED) => {
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
    if container_state(&cid) != ContainerState::Running {
        return false;
    }

    eprintln!(
        ">> docker run exited while container {cid} was still running; killing the container"
    );
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
    Ok(String),
    /// Ran to completion but exited non-zero — a definitive failure, e.g.
    /// `docker inspect` on a container id that no longer resolves.
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
    let spawned = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(_) => return CommandOutcome::Unfinished,
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return CommandOutcome::Failed;
                }
                let mut stdout = Vec::new();
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_end(&mut stdout);
                }
                return CommandOutcome::Ok(String::from_utf8_lossy(&stdout).into_owned());
            }
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return CommandOutcome::Unfinished;
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return CommandOutcome::Unfinished;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Run `docker <args>` silently, returning stdout on success. The watcher's
/// container-stopping calls are all best-effort: a dead daemon or an
/// already-removed container just means there is nothing left to stop.
fn docker_quiet(args: &[&str], timeout: Duration) -> Option<String> {
    match command_quiet("docker", args, timeout) {
        CommandOutcome::Ok(out) => Some(out),
        CommandOutcome::Failed | CommandOutcome::Unfinished => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainerState {
    Running,
    Stopped,
    Unknown,
}

fn parse_container_state(outcome: CommandOutcome) -> ContainerState {
    match outcome {
        CommandOutcome::Ok(out) => match out.trim() {
            "true" => ContainerState::Running,
            "false" => ContainerState::Stopped,
            // A zero exit with an unexpected body shouldn't happen for this
            // format string; stay conservative rather than assume "stopped".
            _ => ContainerState::Unknown,
        },
        // `inspect` ran and exited non-zero: the id no longer resolves because
        // the container is gone (the common `--rm` case). That is effectively
        // stopped — reporting it lets the grace loop exit immediately instead
        // of waiting out the full window for a container that no longer exists.
        CommandOutcome::Failed => ContainerState::Stopped,
        // Timed out / unspawnable: the daemon may be wedged. Treating that as
        // "done" could leave a container alive, so keep it unknown.
        CommandOutcome::Unfinished => ContainerState::Unknown,
    }
}

/// The daemon's view of the container. A wedged daemon is unknown, not stopped:
/// treating it as "done" can leave the container alive. A definitive `inspect`
/// failure (the id no longer resolves) is stopped: the container is gone.
fn container_state(cid: &str) -> ContainerState {
    let outcome = command_quiet(
        "docker",
        &["inspect", "-f", "{{.State.Running}}", cid],
        DOCKER_INSPECT_TIMEOUT,
    );
    parse_container_state(outcome)
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
/// also uses this function directly on the main thread when the CLI detached.
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

fn pending() -> &'static Mutex<Vec<PendingCleanup>> {
    PENDING.get_or_init(|| Mutex::new(Vec::new()))
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
                // Secrets first: gone even if everything below goes wrong.
                cleanup_pending();
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
            // now that the complete handler is live. This happens before any
            // credential is staged or Docker child is spawned.
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

/// Unlink every pending path. Runs on the watcher thread in normal context
/// (signal-hook consumed the actual signal), so plain locking and `remove_file`
/// are fine here.
fn cleanup_pending() {
    if let Ok(_staging) = STAGING.lock() {
        if let Some(lock) = PENDING.get() {
            if let Ok(paths) = lock.lock() {
                // Unique staged files contain credentials; unlink every one
                // before inspecting fixed-path placeholders, whose directory
                // entries are writable from inside the container.
                for p in paths
                    .iter()
                    .filter(|p| matches!(p, PendingCleanup::File(_)))
                {
                    p.cleanup();
                }
                for p in paths
                    .iter()
                    .filter(|p| matches!(p, PendingCleanup::Placeholder { .. }))
                {
                    p.cleanup();
                }
            }
        }
    }
}

/// Remove `path` and drop it from the pending-cleanup set. Shared by both
/// guards' `Drop`, so the normal-exit cleanup can't diverge between them.
fn remove_and_unregister(path: &Path) {
    let _ = std::fs::remove_file(path);
    unregister(path);
}

fn unregister(path: &Path) {
    if let Ok(mut v) = pending().lock() {
        v.retain(|p| p.path() != path);
    }
}

fn remove_placeholder_and_unregister(path: &Path, placeholder: &str) {
    if real_mount_target_parent_exists(path).unwrap_or(false)
        && placeholder_matches(path, placeholder)
    {
        let _ = std::fs::remove_file(path);
    }
    unregister(path);
}

fn staging_temp_dir() -> PathBuf {
    match std::env::var("TMPDIR") {
        Ok(dir) if Path::new(&dir).is_absolute() => PathBuf::from(dir),
        _ => PathBuf::from("/tmp"),
    }
}

/// A staged 0600 temp file that is unlinked on drop (normal path) and registered
/// for signal cleanup (interrupt path). Hold it for as long as the file must
/// exist — typically until `docker run` returns.
pub struct StagedFile {
    path: PathBuf,
}

impl StagedFile {
    /// Create a 0600 temp file under `$TMPDIR` (or `/tmp`) whose name starts with
    /// `prefix`, write `contents`, and arm cleanup. The file is created with the
    /// mode already restricted (never briefly world-readable).
    pub fn create(prefix: &str, contents: &str) -> Result<Self> {
        Self::create_after_register(prefix, contents, |_| Ok(()))
    }

    fn create_after_register(
        prefix: &str,
        contents: &str,
        after_register: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<Self> {
        install_signal_handler()?;

        let dir = staging_temp_dir();
        // Hold the staging lock before the file exists. Otherwise a fatal
        // signal could land after tempfile creation but before registration;
        // the watcher would have no path to unlink before exiting the process.
        let _staging = STAGING.lock().unwrap();
        // NamedTempFile is created 0600 on Unix. `keep()` disarms tempfile's
        // drop-time unlink so the file survives for Docker to read; deletion is
        // ours (StagedFile's Drop + the signal watcher) from here on.
        let mut named = tempfile::Builder::new()
            .prefix(prefix)
            .rand_bytes(6)
            .tempfile_in(&dir)
            .with_context(|| format!("create temp file in {}", dir.display()))?;
        let path = named.path().to_path_buf();
        pending()
            .lock()
            .unwrap()
            .push(PendingCleanup::File(path.clone()));
        if let Err(e) = after_register(&path) {
            remove_and_unregister(&path);
            return Err(e);
        }

        // Ensure 0600 explicitly (defensive).
        if let Err(e) = crate::profile::set_600(&path) {
            remove_and_unregister(&path);
            return Err(e);
        }
        if let Err(e) = named.write_all(contents.as_bytes()) {
            remove_and_unregister(&path);
            return Err(e).with_context(|| format!("write staged file {}", path.display()));
        }
        let (_, path) = match named.keep() {
            Ok(kept) => kept,
            Err(e) => {
                remove_and_unregister(&path);
                return Err(anyhow::anyhow!("persist temp file: {e}"));
            }
        };

        Ok(StagedFile { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        remove_and_unregister(&self.path);
    }
}

/// A file at a *fixed* path that we may need to pre-create as a bind-mount
/// target, removed on cleanup only while it contains our placeholder.
///
/// This is Codex's `auth.json` case: Docker Desktop's virtiofs can't create a
/// bind-mount target nested inside another bind mount (`/home/aibox`), so we
/// pre-create the file at `<home>/.codex/auth.json` for Docker to over-mount.
/// If a real `codex login` auth.json already exists there, we leave it alone —
/// only a placeholder we created is removed. Registered for signal cleanup like
/// [`StagedFile`], so an interrupt doesn't leave our placeholder behind.
///
/// Concurrent auth.json-mode/env-key runs on the same profile share this path.
/// A host-only profile lock protects the short pre-spawn window where the
/// placeholder must not be mistaken for stale and removed under another process
/// before Docker has established its nested bind mount.
pub struct GuardedPath {
    path: PathBuf,
    owns_placeholder: bool,
    placeholder: String,
    lock_path: PathBuf,
    spawn_lock: Option<FileLock>,
}

impl GuardedPath {
    /// Ensure `path` exists as a file. If it was absent, create it with
    /// `placeholder` contents at 0600 and mark it for removal on drop / signal.
    /// If it already existed, leave it untouched and don't remove it later —
    /// unless it holds exactly `placeholder`: that's our own leftover from a
    /// run killed before cleanup (SIGKILL skips both `Drop` and the signal
    /// watcher), so re-adopt it rather than mistake it for a real login file.
    pub fn ensure(path: PathBuf, lock_path: PathBuf, placeholder: &str) -> Result<Self> {
        Self::ensure_after_register(path, lock_path, placeholder, |_| Ok(()))
    }

    fn ensure_after_register(
        path: PathBuf,
        lock_path: PathBuf,
        placeholder: &str,
        after_register: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<Self> {
        #[derive(Clone, Copy)]
        enum MountTarget {
            Missing,
            Placeholder,
            RealFile,
        }

        install_signal_handler()?;
        require_real_mount_target_parent(&path)?;
        require_real_lock_parent(&lock_path)?;
        let spawn_lock = FileLock::acquire(&lock_path)?;

        let target = match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_file() => {
                if placeholder_matches(&path, placeholder) {
                    MountTarget::Placeholder
                } else {
                    MountTarget::RealFile
                }
            }
            Ok(_) => anyhow::bail!("mount target is not a regular file: {}", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => MountTarget::Missing,
            Err(e) => {
                return Err(e).with_context(|| format!("inspect mount target {}", path.display()));
            }
        };

        let owns_placeholder = match target {
            MountTarget::RealFile => false,
            MountTarget::Placeholder | MountTarget::Missing => {
                let needs_create = matches!(target, MountTarget::Missing);
                let _staging = STAGING.lock().unwrap();
                pending().lock().unwrap().push(PendingCleanup::Placeholder {
                    path: path.clone(),
                    contents: placeholder.to_string(),
                    lock_path: lock_path.clone(),
                });
                if let Err(e) = after_register(&path) {
                    unregister(&path);
                    return Err(e);
                }
                if needs_create {
                    if let Err(e) = create_placeholder_file(&path, placeholder) {
                        unregister(&path);
                        return Err(e);
                    }
                }
                if !placeholder_matches(&path, placeholder) {
                    unregister(&path);
                    anyhow::bail!(
                        "mount target changed before use, expected placeholder: {}",
                        path.display()
                    );
                }
                true
            }
        };
        Ok(GuardedPath {
            path,
            owns_placeholder,
            placeholder: placeholder.to_string(),
            lock_path,
            spawn_lock: Some(spawn_lock),
        })
    }

    /// Once Docker has spawned successfully, the nested bind mount no longer
    /// needs the host-side placeholder entry to remain locked.
    pub fn release_spawn_lock(&mut self) {
        self.spawn_lock.take();
    }
}

impl Drop for GuardedPath {
    fn drop(&mut self) {
        if self.owns_placeholder {
            if !real_mount_target_parent_exists(&self.path).unwrap_or(false) {
                unregister(&self.path);
                return;
            }
            let _lock = if let Some(lock) = self.spawn_lock.take() {
                lock
            } else {
                let Ok(lock) = FileLock::acquire_for_drop(&self.lock_path) else {
                    unregister(&self.path);
                    return;
                };
                lock
            };
            remove_placeholder_and_unregister(&self.path, &self.placeholder);
        }
    }
}

/// Remove a leftover placeholder at `path` from a run killed before cleanup
/// (SIGKILL skips both `Drop` and the signal watcher). Only a file holding
/// exactly `placeholder` is ours to delete — same ownership rule as
/// [`GuardedPath::ensure`]'s re-adoption; anything else is left alone. For
/// callers that won't guard the path this run but shouldn't ship a stale
/// placeholder either (Codex env_key mode vs a dead auth.json-mode run).
pub fn remove_stale_placeholder(path: &Path, lock_path: &Path, placeholder: &str) -> Result<()> {
    if !real_mount_target_parent_exists(path)? {
        return Ok(());
    }
    require_real_lock_parent(lock_path)?;
    let _lock = FileLock::acquire(lock_path)?;
    if placeholder_matches(path, placeholder) {
        std::fs::remove_file(path)
            .with_context(|| format!("remove stale placeholder {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::EnvGuard;
    use std::sync::Mutex;

    static PENDING_CLEANUP_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn pending_contains(path: &Path) -> bool {
        pending().lock().unwrap().iter().any(|p| p.path() == path)
    }

    struct CidfileGuard;

    impl Drop for CidfileGuard {
        fn drop(&mut self) {
            clear_child();
        }
    }

    fn stable_tempdir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("aibox-test.")
            .tempdir_in(staging_temp_dir())
            .unwrap()
    }

    fn auth_lock_path(auth: &Path) -> PathBuf {
        auth.parent()
            .unwrap()
            .join(".locks")
            .join("codex-auth-json.lock")
    }

    fn placeholder_temp_count(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(".aibox-placeholder."))
            })
            .count()
    }

    #[cfg(unix)]
    const LOCK_HELPER_LOCK: &str = "AIBOX_TEST_LOCK_HELPER_LOCK";
    #[cfg(unix)]
    const LOCK_HELPER_READY: &str = "AIBOX_TEST_LOCK_HELPER_READY";
    #[cfg(unix)]
    const LOCK_HELPER_RELEASE: &str = "AIBOX_TEST_LOCK_HELPER_RELEASE";

    #[cfg(unix)]
    #[test]
    fn lock_helper_process() {
        let Some(lock_path) = std::env::var_os(LOCK_HELPER_LOCK) else {
            return;
        };
        let ready = PathBuf::from(std::env::var_os(LOCK_HELPER_READY).unwrap());
        let release = PathBuf::from(std::env::var_os(LOCK_HELPER_RELEASE).unwrap());

        let _lock = FileLock::acquire(Path::new(&lock_path)).unwrap();
        std::fs::write(&ready, "ready\n").unwrap();
        let started = Instant::now();
        while !release.exists() && started.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    fn spawn_lock_holder(lock_path: &Path) -> (std::process::Child, PathBuf, tempfile::TempDir) {
        std::fs::create_dir_all(lock_path.parent().unwrap()).unwrap();
        let scratch = stable_tempdir();
        let ready = scratch.path().join("ready");
        let release = scratch.path().join("release");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("creds::tests::lock_helper_process")
            .env(LOCK_HELPER_LOCK, lock_path)
            .env(LOCK_HELPER_READY, &ready)
            .env(LOCK_HELPER_RELEASE, &release)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();

        let started = Instant::now();
        while !ready.exists() {
            if started.elapsed() > Duration::from_secs(2) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("lock helper did not acquire {}", lock_path.display());
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        (child, release, scratch)
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

    /// Under `nohup` (or an equivalent parent), SIGHUP starts as SIG_IGN. The
    /// cleanup watcher must not install over that inherited disposition: doing
    /// so would turn a deliberately survivable hangup into a fatal signal.
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

    /// Body of the SIGTERM subprocess: arm exactly what a real run arms — a
    /// staged credential, a guarded auth.json placeholder, and a populated
    /// cidfile — then report ready and block. It must never exit on its own; the
    /// parent's signal is what ends it, and the parent asserts on what survived.
    #[cfg(unix)]
    #[test]
    fn signal_helper_process() {
        let Some(dir) = std::env::var_os(SIGNAL_HELPER_DIR) else {
            return;
        };
        let dir = PathBuf::from(dir);
        let auth = dir.join("home").join(".codex").join("auth.json");
        std::fs::create_dir_all(auth.parent().unwrap()).unwrap();

        let cid_path = dir.join("cid");
        std::fs::write(&cid_path, "signal-container\n").unwrap();
        // Installs the watcher thread and marks the run active, exactly as
        // `docker::run` does before spawning the Docker CLI.
        set_cidfile(&cid_path).unwrap();

        let staged = StagedFile::create("aibox-signal-key.", "OPENAI_API_KEY=sk-secret\n").unwrap();
        let guarded = GuardedPath::ensure(
            auth.clone(),
            dir.join(".locks").join("codex-auth-json.lock"),
            "{}\n",
        )
        .unwrap();
        std::fs::write(dir.join("staged-path"), staged.path().display().to_string()).unwrap();

        std::fs::write(dir.join("ready"), "ready\n").unwrap();
        // Hold both guards past the signal. `Drop` must not be what cleans up
        // here — a signal death skips it — so park until the watcher exits us.
        std::thread::sleep(Duration::from_secs(60));
        drop(staged);
        drop(guarded);
    }

    /// Run `signal_helper_process` as a subprocess with a stubbed `docker`, wait
    /// for it to arm its credentials, then deliver `sig`. Returns the scratch dir
    /// and the child's exit status.
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
            // Staged credentials land here, so the parent can assert the dir is
            // empty afterwards rather than guessing the random temp name.
            .env("TMPDIR", scratch.path().join("staging"))
            .env("PATH", &fake_docker)
            .env("AIBOX_FAKE_DOCKER_LOG", scratch.path().join("docker.log"))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::fs::create_dir_all(scratch.path().join("staging")).unwrap();

        let started = Instant::now();
        while !ready.exists() {
            if started.elapsed() > Duration::from_secs(10) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("signal helper never armed its credentials");
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

    /// The module's headline invariant, on the path `Drop` cannot cover. Ctrl-C
    /// (and a service-manager SIGTERM) must leave no secret on disk and no
    /// container running — AGENTS.md flags this as the manual check for any
    /// `creds.rs` change, so cover it here instead.
    #[cfg(unix)]
    #[test]
    fn fatal_signal_removes_credentials_and_stops_the_container() {
        use std::os::unix::process::ExitStatusExt;

        for sig in [
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGHUP,
        ] {
            let (scratch, status) = run_signal_helper(sig);

            // The staged credential is gone, and so is the whole temp name —
            // not merely truncated or left behind for the next process.
            let staged =
                std::fs::read_to_string(scratch.path().join("staged-path")).expect("staged path");
            assert!(
                !Path::new(&staged).exists(),
                "sig {sig}: staged credential survived the signal: {staged}"
            );
            assert!(
                std::fs::read_dir(scratch.path().join("staging"))
                    .unwrap()
                    .next()
                    .is_none(),
                "sig {sig}: staging dir must be empty after signal cleanup"
            );

            // The guarded auth.json placeholder is ours, so it goes too.
            assert!(
                !scratch.path().join("home/.codex/auth.json").exists(),
                "sig {sig}: auth.json placeholder survived the signal"
            );

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

    /// A `docker` stub whose `inspect` can report a *running* container, which
    /// [`write_signal_fake_docker`] never does (it always says stopped, so
    /// `stop_container_id` returns before its grace/escalation logic). Reports
    /// running for the first `AIBOX_FAKE_DOCKER_RUNNING_INSPECTS` inspects, then
    /// stopped — or forever when that count is `always`.
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

    /// Restores [`SIGNAL_COUNT`] on drop. The counter is process-global and the
    /// watcher reads it, so a test that fakes a second signal must put it back.
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

    /// The graceful half of the escalation: deliver the signal to PID 1, and if
    /// the container is still running, wait it out rather than SIGKILLing an
    /// agent that is shutting down cleanly.
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

    /// The escalation: the agent is the container's PID 1, and PID 1 ignores
    /// signals it has no handler for, so a graceful signal alone can never
    /// land. A second signal must cut the wait short and SIGKILL now.
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

    /// The wrapper's fatal signal has to reach the container as the *same*
    /// signal; a mismapped name would make the agent miss its shutdown path.
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

    #[test]
    fn watcher_commands_are_bounded() {
        // Worst case is the late-cidfile path in `stop_active_run`: the first
        // bounded wait fails (CIDFILE_WAIT), then after signalling the child the
        // longer late wait succeeds (LATE_CIDFILE_WAIT), then `stop_container_id`
        // runs its full graceful/escalating cleanup.
        let worst_case_stop_container_id = DOCKER_KILL_TIMEOUT
            + DOCKER_INSPECT_TIMEOUT
            + CONTAINER_GRACE
            + CONTAINER_POLL_INTERVAL
            + DOCKER_INSPECT_TIMEOUT
            + DOCKER_KILL_TIMEOUT;
        let worst_case_signal_cleanup =
            CIDFILE_WAIT + LATE_CIDFILE_WAIT + worst_case_stop_container_id;
        assert!(
            SIGNAL_FINISH_WAIT > worst_case_signal_cleanup,
            "the main thread must not exit before the watcher can finish its bounded cleanup"
        );

        assert!(matches!(
            command_quiet("/bin/sh", &["-c", "printf ok"], Duration::from_secs(1)),
            CommandOutcome::Ok(out) if out == "ok"
        ));

        // A fast non-zero exit is a definitive failure, distinct from a timeout.
        assert!(matches!(
            command_quiet("/bin/sh", &["-c", "exit 1"], Duration::from_secs(1)),
            CommandOutcome::Failed
        ));

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
    fn staging_temp_dir_ignores_empty_or_relative_tmpdir() {
        let _env_lock = crate::test_env_lock();
        {
            let _guard = EnvGuard::set("TMPDIR", "");
            assert_eq!(staging_temp_dir(), PathBuf::from("/tmp"));
        }
        {
            let _guard = EnvGuard::set("TMPDIR", "relative-tmp");
            assert_eq!(staging_temp_dir(), PathBuf::from("/tmp"));
        }
    }

    #[test]
    fn staged_file_uses_absolute_tmpdir_and_cleans_up() {
        let _pending_lock = PENDING_CLEANUP_TEST_LOCK.lock().unwrap();
        let _env_lock = crate::test_env_lock();
        let dir = tempfile::tempdir().unwrap();
        let _tmpdir = EnvGuard::set("TMPDIR", dir.path().to_str().unwrap());

        let staged = StagedFile::create("aibox-test-tmpdir.", "secret\n").unwrap();
        let path = staged.path().to_path_buf();

        assert_eq!(path.parent(), Some(dir.path()));
        drop(staged);
        assert!(!path.exists(), "staged credentials must be removed on drop");
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

    /// The orphan-on-detach guarantee: an attached `docker run` CLI can exit
    /// (Docker's detach key sequence, or a client/daemon disconnect) while its
    /// container keeps running. `finish_child` must notice the live container
    /// and kill it rather than leave it running unsupervised after the wrapper
    /// exits — the exact failure the "kill lingering containers" fix targets.
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

    /// The common case must stay cheap and side-effect-free: after an ordinary
    /// `--rm` exit the container id no longer resolves, so `finish_child` must
    /// return without a `docker kill` and without entering any grace wait. A
    /// regression here would stall or kill on every clean run.
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

    #[test]
    fn container_state_parser_distinguishes_running_stopped_unknown() {
        assert_eq!(
            parse_container_state(CommandOutcome::Ok("true\n".into())),
            ContainerState::Running
        );
        assert_eq!(
            parse_container_state(CommandOutcome::Ok("false\n".into())),
            ContainerState::Stopped
        );
        assert_eq!(
            parse_container_state(CommandOutcome::Ok(String::new())),
            ContainerState::Unknown
        );
        assert_eq!(
            parse_container_state(CommandOutcome::Ok("docker error".into())),
            ContainerState::Unknown
        );
        // A definitive non-zero `inspect` means the id no longer resolves — the
        // container is gone, so the grace loop can stop waiting immediately.
        assert_eq!(
            parse_container_state(CommandOutcome::Failed),
            ContainerState::Stopped
        );
        // A timeout / unspawnable docker is not an answer: stay Unknown so a
        // wedged daemon can't be mistaken for a stopped container.
        assert_eq!(
            parse_container_state(CommandOutcome::Unfinished),
            ContainerState::Unknown
        );
    }

    #[cfg(unix)]
    #[test]
    fn placeholder_matcher_does_not_follow_mutated_paths() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.json");
        let link = dir.path().join("link.json");
        std::fs::write(&target, "{}\n").unwrap();
        symlink(&target, &link).unwrap();
        assert!(!placeholder_matches(&link, "{}\n"));

        let fifo = dir.path().join("auth.fifo");
        let fifo_c = CString::new(fifo.as_os_str().as_bytes()).unwrap();
        let rc = unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) };
        assert_eq!(
            rc,
            0,
            "mkfifo {}: {}",
            fifo.display(),
            std::io::Error::last_os_error()
        );
        assert!(
            !placeholder_matches(&fifo, "{}\n"),
            "special files must be rejected without a blocking read"
        );
    }

    #[test]
    fn placeholder_matcher_requires_exact_contents() {
        // Ownership of the fixed auth.json path is decided *only* by an exact
        // content match. Anything else is the user's real login file, so every
        // near miss below must read as "not ours" and be left alone.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.json");

        std::fs::write(&path, "{}\n").unwrap();
        assert!(placeholder_matches(&path, "{}\n"), "our own placeholder");

        // Same length, different bytes: the size check passes and the content
        // comparison is what has to reject it.
        std::fs::write(&path, "{ }\n").unwrap();
        assert!(
            !placeholder_matches(&path, "{}\n"),
            "same-length content must still be compared byte for byte"
        );

        // A real login file (longer) and a truncated one (shorter) are both
        // rejected on size, without reading further.
        std::fs::write(&path, "{\"OPENAI_API_KEY\":\"sk-real\"}\n").unwrap();
        assert!(!placeholder_matches(&path, "{}\n"));
        std::fs::write(&path, "{").unwrap();
        assert!(!placeholder_matches(&path, "{}\n"));

        // A path that does not exist is not a placeholder either.
        std::fs::remove_file(&path).unwrap();
        assert!(!placeholder_matches(&path, "{}\n"));
    }

    #[test]
    fn guarded_path_requires_an_existing_mount_target_parent() {
        // Docker cannot create a bind-mount target nested in another mount, so
        // the parent must already exist. Saying so beats a later opaque Docker
        // error naming a path the user never typed.
        let dir = tempfile::tempdir().unwrap();
        let auth = dir.path().join("never-created").join("auth.json");

        let err = GuardedPath::ensure(auth.clone(), auth_lock_path(&auth), "{}\n")
            .map(|_| ())
            .unwrap_err()
            .to_string();

        assert!(err.contains("mount target parent does not exist"), "{err}");
        assert!(!auth.exists());

        // The stale-cleanup path treats the same absent parent as "nothing to
        // clean" rather than an error: there cannot be a leftover under it.
        remove_stale_placeholder(&auth, &auth_lock_path(&auth), "{}\n").unwrap();
    }

    #[test]
    fn remove_stale_placeholder_leaves_a_real_login_file_untouched() {
        // The stale-cleanup path (env_key mode inheriting a dead auth.json-mode
        // run's leftover) must delete *only* a file that is byte-for-byte our
        // placeholder. A real `codex login` under the same fixed path is the
        // user's credentials — wiping it would silently log them out.
        let dir = tempfile::tempdir().unwrap();
        let auth = dir.path().join("auth.json");
        let real = "{\"OPENAI_API_KEY\":\"sk-real-login\"}\n";
        std::fs::write(&auth, real).unwrap();

        remove_stale_placeholder(&auth, &auth_lock_path(&auth), "{}\n").unwrap();

        assert!(
            auth.exists(),
            "a real login file must survive stale cleanup"
        );
        assert_eq!(
            std::fs::read_to_string(&auth).unwrap(),
            real,
            "stale cleanup must not modify a non-placeholder file"
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_placeholder_file_persists_complete_restricted_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let auth = dir.path().join("auth.json");

        create_placeholder_file(&auth, "{}\n").unwrap();

        assert_eq!(std::fs::read_to_string(&auth).unwrap(), "{}\n");
        let mode = std::fs::metadata(&auth).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        assert_eq!(
            placeholder_temp_count(dir.path()),
            0,
            "atomic placeholder temp file must not remain beside the target"
        );
    }

    #[test]
    fn create_placeholder_file_does_not_clobber_existing_target() {
        let dir = tempfile::tempdir().unwrap();
        let auth = dir.path().join("auth.json");
        std::fs::write(&auth, "{\"OPENAI_API_KEY\":\"real\"}\n").unwrap();

        let err = create_placeholder_file(&auth, "{}\n")
            .unwrap_err()
            .to_string();

        assert!(err.contains("pre-create mount target"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&auth).unwrap(),
            "{\"OPENAI_API_KEY\":\"real\"}\n"
        );
        assert_eq!(
            placeholder_temp_count(dir.path()),
            0,
            "failed atomic persist must clean up its temp file"
        );
    }

    #[cfg(unix)]
    #[test]
    fn create_placeholder_file_does_not_follow_existing_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let auth = dir.path().join("auth.json");
        let outside = dir.path().join("outside.json");
        std::fs::write(&outside, "real\n").unwrap();
        symlink(&outside, &auth).unwrap();

        let err = create_placeholder_file(&auth, "{}\n")
            .unwrap_err()
            .to_string();

        assert!(err.contains("pre-create mount target"), "{err}");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "real\n");
        assert!(std::fs::symlink_metadata(&auth)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(placeholder_temp_count(dir.path()), 0);
    }

    #[cfg(unix)]
    #[test]
    fn guarded_path_create_new_does_not_follow_raced_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let auth = dir.path().join("auth.json");
        let outside = dir.path().join("outside.json");
        std::fs::write(&outside, "real\n").unwrap();

        let raced_auth = auth.clone();
        let err =
            GuardedPath::ensure_after_register(auth.clone(), auth_lock_path(&auth), "{}\n", |_| {
                symlink(&outside, &raced_auth).unwrap();
                Ok(())
            })
            .map(|_| ())
            .unwrap_err()
            .to_string();

        assert!(err.contains("pre-create mount target"), "{err}");
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "real\n");
        assert!(std::fs::symlink_metadata(&auth)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn guarded_path_rejects_symlinked_parent_without_touching_target() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let codex = dir.path().join(".codex");
        symlink(outside.path(), &codex).unwrap();
        let auth = codex.join("auth.json");

        let lock_path = auth_lock_path(&auth);
        let err = GuardedPath::ensure(auth.clone(), lock_path.clone(), "{}\n")
            .map(|_| ())
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("mount target parent is not a real directory"),
            "{err}"
        );
        assert!(
            !outside.path().join("auth.json").exists(),
            "guarded path must not create a placeholder through a symlinked parent"
        );
        assert!(
            !lock_path.parent().unwrap().exists(),
            "guarded path must not create a lock after rejecting the mount parent"
        );

        let outside_auth = outside.path().join("auth.json");
        std::fs::write(&outside_auth, "{}\n").unwrap();
        let err = remove_stale_placeholder(&auth, &lock_path, "{}\n")
            .unwrap_err()
            .to_string();

        assert!(
            err.contains("mount target parent is not a real directory"),
            "{err}"
        );
        assert!(
            outside_auth.exists(),
            "stale cleanup must not delete through a symlinked parent"
        );
        assert!(
            !lock_path.parent().unwrap().exists(),
            "stale cleanup must not create a lock after rejecting the mount parent"
        );
    }

    #[test]
    fn guarded_path_rejects_non_directory_parent() {
        let dir = tempfile::tempdir().unwrap();
        let codex = dir.path().join(".codex");
        std::fs::write(&codex, "not a directory\n").unwrap();
        let auth = codex.join("auth.json");

        let lock_path = auth_lock_path(&auth);
        let err = GuardedPath::ensure(auth.clone(), lock_path.clone(), "{}\n")
            .map(|_| ())
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("mount target parent is not a real directory"),
            "{err}"
        );

        let err = remove_stale_placeholder(&auth, &lock_path, "{}\n")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("mount target parent is not a real directory"),
            "{err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn guarded_path_waits_before_reusing_an_active_placeholder() {
        let _pending_lock = PENDING_CLEANUP_TEST_LOCK.lock().unwrap();
        let dir = stable_tempdir();
        let auth = dir.path().join("auth.json");
        std::fs::write(&auth, "{}\n").unwrap();
        let lock_path = auth_lock_path(&auth);
        let (mut child, release, _scratch) = spawn_lock_holder(&lock_path);
        let (tx, rx) = std::sync::mpsc::channel();

        let auth_for_thread = auth.clone();
        let lock_path_for_thread = lock_path.clone();
        let handle = std::thread::spawn(move || {
            let guarded =
                GuardedPath::ensure(auth_for_thread, lock_path_for_thread, "{}\n").unwrap();
            tx.send(()).unwrap();
            guarded
        });

        assert!(
            rx.recv_timeout(Duration::from_millis(150)).is_err(),
            "active placeholder should not be reused while its lock is held"
        );
        assert!(
            auth.exists(),
            "active placeholder must not be removed early"
        );

        std::fs::write(&release, "release\n").unwrap();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let guarded = handle.join().unwrap();
        assert!(auth.exists(), "reused placeholder remains until guard drop");
        drop(guarded);
        assert!(!auth.exists(), "re-adopted stale placeholder is cleaned up");
        assert!(child.wait().unwrap().success());
    }

    #[cfg(unix)]
    #[test]
    fn stale_placeholder_cleanup_waits_for_active_lock() {
        let _pending_lock = PENDING_CLEANUP_TEST_LOCK.lock().unwrap();
        let dir = stable_tempdir();
        let auth = dir.path().join("auth.json");
        std::fs::write(&auth, "{}\n").unwrap();
        let lock_path = auth_lock_path(&auth);
        let (mut child, release, _scratch) = spawn_lock_holder(&lock_path);
        let (tx, rx) = std::sync::mpsc::channel();

        let auth_for_thread = auth.clone();
        let lock_path_for_thread = lock_path.clone();
        let handle = std::thread::spawn(move || {
            remove_stale_placeholder(&auth_for_thread, &lock_path_for_thread, "{}\n").unwrap();
            tx.send(()).unwrap();
        });

        assert!(
            rx.recv_timeout(Duration::from_millis(150)).is_err(),
            "env_key cleanup should wait for an active auth.json-mode spawn"
        );
        assert!(
            auth.exists(),
            "active placeholder must not be removed early"
        );

        std::fs::write(&release, "release\n").unwrap();
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        handle.join().unwrap();
        assert!(
            !auth.exists(),
            "stale placeholder is removed after lock release"
        );
        assert!(child.wait().unwrap().success());
    }

    #[cfg(unix)]
    #[test]
    fn guarded_path_times_out_when_auth_lock_stays_busy() {
        let _pending_lock = PENDING_CLEANUP_TEST_LOCK.lock().unwrap();
        let dir = stable_tempdir();
        let auth = dir.path().join("auth.json");
        std::fs::write(&auth, "{}\n").unwrap();
        let lock_path = auth_lock_path(&auth);
        let (mut child, release, _scratch) = spawn_lock_holder(&lock_path);

        let started = Instant::now();
        let err = GuardedPath::ensure(auth.clone(), lock_path.clone(), "{}\n")
            .map(|_| ())
            .unwrap_err()
            .to_string();

        assert!(err.contains("auth mount target is busy"), "{err}");
        assert!(
            started.elapsed() < AUTH_LOCK_WAIT + Duration::from_secs(1),
            "busy auth lock should fail within the bounded wait"
        );
        assert!(
            auth.exists(),
            "timeout must not delete another run's active placeholder"
        );

        std::fs::write(&release, "release\n").unwrap();
        assert!(child.wait().unwrap().success());
    }

    #[cfg(unix)]
    #[test]
    fn stale_placeholder_cleanup_times_out_when_auth_lock_stays_busy() {
        let _pending_lock = PENDING_CLEANUP_TEST_LOCK.lock().unwrap();
        let dir = stable_tempdir();
        let auth = dir.path().join("auth.json");
        std::fs::write(&auth, "{}\n").unwrap();
        let lock_path = auth_lock_path(&auth);
        let (mut child, release, _scratch) = spawn_lock_holder(&lock_path);

        let started = Instant::now();
        let err = remove_stale_placeholder(&auth, &lock_path, "{}\n")
            .unwrap_err()
            .to_string();

        assert!(err.contains("auth mount target is busy"), "{err}");
        assert!(
            started.elapsed() < AUTH_LOCK_WAIT + Duration::from_secs(1),
            "stale cleanup should fail within the bounded wait"
        );
        assert!(
            auth.exists(),
            "timeout must preserve the placeholder because ownership is unclear"
        );

        std::fs::write(&release, "release\n").unwrap();
        assert!(child.wait().unwrap().success());
    }

    // One test, not two: the pending set is process-global, and
    // `cleanup_pending` unlinks everything registered — interleaving with a
    // parallel test staging its own file must stay within this single test.
    #[test]
    fn staged_file_cleanup_paths() {
        let _pending_lock = PENDING_CLEANUP_TEST_LOCK.lock().unwrap();
        // Drop path: file unlinked and unregistered.
        let staged = StagedFile::create("aibox-test-drop.", "secret\n").unwrap();
        let path = staged.path().to_path_buf();
        assert!(pending_contains(&path));
        // Staged secrets must never be readable by anyone else.
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        drop(staged);
        assert!(!path.exists());
        assert!(!pending_contains(&path));

        // The path is registered before secret contents are written, closing the
        // signal window where a just-created temp file could otherwise survive
        // process death untracked.
        let mut registered = None;
        let err = StagedFile::create_after_register("aibox-test-register.", "secret\n", |p| {
            registered = Some(p.to_path_buf());
            assert!(
                pending_contains(p),
                "path must be armed for signal cleanup before writing contents"
            );
            Err(anyhow::anyhow!("simulated failure after registration"))
        })
        .map(|_| ())
        .unwrap_err();
        let path = registered.unwrap();
        assert!(
            err.to_string().contains("simulated failure"),
            "creation should surface the simulated failure: {err}"
        );
        assert!(!path.exists());
        assert!(!pending_contains(&path));

        // Signal path: the watcher-side sweep removes a registered file.
        let staged = StagedFile::create("aibox-test-signal.", "secret\n").unwrap();
        let path = staged.path().to_path_buf();
        assert!(path.exists());
        cleanup_pending();
        assert!(!path.exists(), "signal cleanup should unlink staged file");
        drop(staged); // remove is a no-op on the already-gone file

        // GuardedPath: an absent path is created as our placeholder and
        // removed again on drop.
        let dir = tempfile::tempdir().unwrap();
        let fresh = dir.path().join("fresh.json");
        let g = GuardedPath::ensure(fresh.clone(), auth_lock_path(&fresh), "{}\n").unwrap();
        assert!(g.owns_placeholder, "absent path means we own the file");
        assert!(fresh.is_file());
        assert!(pending_contains(&fresh));
        drop(g);
        assert!(!fresh.exists(), "created placeholder is removed on drop");

        // Docker only needs this lock until it has established the nested bind
        // mount. Releasing it must let another run on the same profile adopt the
        // placeholder while the first guard is still alive; both guards must
        // continue to agree that the placeholder is wrapper-owned and clean it
        // up once the runs finish.
        let concurrent = dir.path().join("concurrent.json");
        let concurrent_lock = auth_lock_path(&concurrent);
        let mut first =
            GuardedPath::ensure(concurrent.clone(), concurrent_lock.clone(), "{}\n").unwrap();
        first.release_spawn_lock();
        let second = GuardedPath::ensure(concurrent.clone(), concurrent_lock, "{}\n").unwrap();
        drop(second);
        drop(first);
        assert!(
            !concurrent.exists(),
            "released spawn locks must not give up final placeholder ownership"
        );

        // GuardedPath also registers before it writes placeholder contents, so a
        // failure after registration doesn't leave an untracked cleanup entry.
        let guard_register_fail = dir.path().join("guard-register-fail.json");
        let err = GuardedPath::ensure_after_register(
            guard_register_fail.clone(),
            auth_lock_path(&guard_register_fail),
            "{}\n",
            |p| {
                assert!(
                    pending_contains(p),
                    "placeholder path must be armed before writing contents"
                );
                Err(anyhow::anyhow!(
                    "simulated guarded failure after registration"
                ))
            },
        )
        .map(|_| ())
        .unwrap_err();
        assert!(
            err.to_string().contains("simulated guarded failure"),
            "creation should surface the simulated failure: {err}"
        );
        assert!(!guard_register_fail.exists());
        assert!(!pending_contains(&guard_register_fail));

        // If something replaces the placeholder while the guard is alive, it is
        // no longer ours to unlink. Still unregister it from signal cleanup.
        let replaced = dir.path().join("replaced.json");
        let g = GuardedPath::ensure(replaced.clone(), auth_lock_path(&replaced), "{}\n").unwrap();
        std::fs::write(&replaced, "{\"OPENAI_API_KEY\":\"real\"}\n").unwrap();
        drop(g);
        assert!(replaced.exists(), "replacement content must not be deleted");
        assert!(!pending_contains(&replaced));

        // GuardedPath is also registered for signal cleanup while held.
        let signal_placeholder = dir.path().join("signal.json");
        let g = GuardedPath::ensure(
            signal_placeholder.clone(),
            auth_lock_path(&signal_placeholder),
            "{}\n",
        )
        .unwrap();
        assert!(pending_contains(&signal_placeholder));
        cleanup_pending();
        assert!(
            !signal_placeholder.exists(),
            "signal cleanup removes guarded placeholders too"
        );
        drop(g);
        assert!(!pending_contains(&signal_placeholder));

        let signal_replaced = dir.path().join("signal-replaced.json");
        let g = GuardedPath::ensure(
            signal_replaced.clone(),
            auth_lock_path(&signal_replaced),
            "{}\n",
        )
        .unwrap();
        std::fs::write(&signal_replaced, "{\"OPENAI_API_KEY\":\"real\"}\n").unwrap();
        cleanup_pending();
        assert!(
            signal_replaced.exists(),
            "signal cleanup must not delete replacement content"
        );
        drop(g);
        assert!(!pending_contains(&signal_replaced));

        // GuardedPath: a pre-existing real file is never touched.
        let real = dir.path().join("auth.json");
        std::fs::write(&real, "{\"OPENAI_API_KEY\":\"k\"}\n").unwrap();
        let g = GuardedPath::ensure(real.clone(), auth_lock_path(&real), "{}\n").unwrap();
        assert!(!g.owns_placeholder);
        drop(g);
        assert!(real.exists(), "real auth.json must survive");

        // GuardedPath: a leftover placeholder (SIGKILL'd run) is re-adopted
        // and cleaned up instead of being treated as a real file forever.
        let leftover = dir.path().join("leftover.json");
        std::fs::write(&leftover, "{}\n").unwrap();
        let g = GuardedPath::ensure(leftover.clone(), auth_lock_path(&leftover), "{}\n").unwrap();
        assert!(
            g.owns_placeholder,
            "placeholder contents mean we own the file"
        );
        drop(g);
        assert!(!leftover.exists(), "re-adopted placeholder is removed");
    }
}
