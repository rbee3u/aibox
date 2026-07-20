//! Ephemeral credential staging with cleanup on normal exits and handled signals.
//!
//! Secrets are staged in 0600 temp files: Claude's merged env file, Codex's
//! key-only env file, or Codex's throwaway `auth.json`. They must never outlive
//! the agent run.
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
//! SIGKILL) cannot run process cleanup, so secrets must never be written into
//! profile homes.
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
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const DOCKER_COMMAND_TIMEOUT: Duration = Duration::from_secs(1);
const CIDFILE_WAIT: Duration = Duration::from_secs(1);
const CIDFILE_POLL_INTERVAL: Duration = Duration::from_millis(20);
const CONTAINER_GRACE: Duration = Duration::from_secs(10);
const CONTAINER_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Paths to clean if a fatal signal arrives. The watcher thread runs in normal
/// (non-signal-handler) context — signal-hook's internal handler only sets a
/// flag — so a plain `Mutex` is fine; nothing here must be async-signal-safe.
static PENDING: OnceLock<Mutex<Vec<PendingCleanup>>> = OnceLock::new();

enum PendingCleanup {
    /// A unique temp file owned completely by this process.
    File(PathBuf),
    /// A fixed path we only own while it still contains this exact placeholder.
    Placeholder { path: PathBuf, contents: String },
}

impl PendingCleanup {
    fn path(&self) -> &Path {
        match self {
            PendingCleanup::File(path) => path,
            PendingCleanup::Placeholder { path, .. } => path,
        }
    }

    fn cleanup(&self) {
        match self {
            PendingCleanup::File(path) => {
                let _ = std::fs::remove_file(path);
            }
            PendingCleanup::Placeholder { path, contents } => {
                if std::fs::read_to_string(path).is_ok_and(|found| found == *contents) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
}

/// Serializes staged-file creation with signal/test cleanup. A signal arriving
/// while a credential file is being armed waits until the path is registered
/// and contents are written, then unlinks it.
static STAGING: Mutex<()> = Mutex::new(());

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

/// Finish a successfully spawned child after `wait` returns. When no fatal
/// signal raced with the wait, unregister it normally. If a signal did race,
/// the Docker CLI may already be reaped while its container is still running;
/// clear the now-stale pid, retain the cidfile, and keep this thread alive until
/// the watcher terminates the process after daemon-side cleanup.
pub fn finish_child() {
    CHILD_PID.store(0, Ordering::SeqCst);
    match RUN_STATE.compare_exchange(RUN_ACTIVE, RUN_IDLE, Ordering::SeqCst, Ordering::SeqCst) {
        Ok(_) | Err(RUN_IDLE) => {
            *cidfile().lock().unwrap() = None;
        }
        Err(RUN_SIGNALLED) => {
            // The watcher is stopping the container and will terminate the
            // whole process (`process::exit(128+sig)`) once daemon-side cleanup
            // is done, tearing down this parked thread with it. Park until it
            // does — but not forever: if the watcher thread died unexpectedly
            // (e.g. it panicked), parking with no bound would hang the wrapper.
            // The deadline covers the container grace period plus slack for the
            // bounded docker commands; past it, exit here as the signal would.
            let deadline = Instant::now() + CONTAINER_GRACE + Duration::from_secs(5);
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

/// Run a command silently with a timeout, returning stdout on success. Used by
/// the signal watcher, where Docker may be wedged and must not prevent the
/// wrapper from re-raising the fatal signal.
fn command_quiet(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                if let Some(mut pipe) = child.stdout.take() {
                    let _ = pipe.read_to_end(&mut stdout);
                }
                return status
                    .success()
                    .then(|| String::from_utf8_lossy(&stdout).into_owned());
            }
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Run `docker <args>` silently, returning stdout on success. The watcher's
/// container-stopping calls are all best-effort: a dead daemon or an
/// already-removed container just means there is nothing left to stop.
fn docker_quiet(args: &[&str]) -> Option<String> {
    command_quiet("docker", args, DOCKER_COMMAND_TIMEOUT)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainerState {
    Running,
    Stopped,
    Unknown,
}

fn parse_container_state(output: Option<&str>) -> ContainerState {
    match output.map(str::trim) {
        Some("true") => ContainerState::Running,
        Some("false") => ContainerState::Stopped,
        _ => ContainerState::Unknown,
    }
}

/// The daemon's view of the container. Docker command failures are unknown, not
/// stopped: treating a wedged daemon as "done" can leave the container alive.
fn container_state(cid: &str) -> ContainerState {
    let out = docker_quiet(&["inspect", "-f", "{{.State.Running}}", cid]);
    parse_container_state(out.as_deref())
}

/// Stop the container through the daemon: deliver `sig` to its PID 1 (what
/// `--sig-proxy` would have done, had the CLI not had a TTY), then escalate to
/// a plain `docker kill` (SIGKILL) if it lingers — an agent without a handler
/// for the signal never exits on it as PID 1. The 10s grace mirrors
/// `docker stop`'s default.
///
/// While this runs, the main thread stays blocked in `child.wait()` (the CLI
/// only exits once its container does), so the process can't exit under the
/// escalation. That's also why the watcher stops the container *before*
/// touching the CLI child.
fn stop_container(sig: i32) {
    let Some(cid) = wait_current_cid(CIDFILE_WAIT) else {
        return;
    };
    let name = match sig {
        s if s == signal_hook::consts::SIGINT => "INT",
        s if s == signal_hook::consts::SIGHUP => "HUP",
        _ => "TERM",
    };
    let _ = docker_quiet(&["kill", "--signal", name, &cid]);
    if container_state(&cid) == ContainerState::Stopped {
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
        if container_state(&cid) == ContainerState::Stopped {
            return;
        }
    }
    let _ = docker_quiet(&["kill", &cid]);
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
                // Stop the container via the daemon. Must come before touching
                // the CLI child: killing the CLI first would unblock the main
                // thread's `wait()` and the process could exit mid-escalation.
                stop_container(sig);
                // Forward to the CLI child — the fallback for a run with no
                // container id yet (or a docker CLI wedged before create).
                signal_child(sig);
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
                for p in paths.iter() {
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
    if std::fs::read_to_string(path).is_ok_and(|contents| contents == placeholder) {
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
        // NamedTempFile is created 0600 on Unix. `keep()` disarms tempfile's
        // drop-time unlink so the file survives for Docker to read; deletion is
        // ours (StagedFile's Drop + the signal watcher) from here on.
        let named = tempfile::Builder::new()
            .prefix(prefix)
            .rand_bytes(6)
            .tempfile_in(&dir)
            .with_context(|| format!("create temp file in {}", dir.display()))?;
        let path = named.path().to_path_buf();
        let _staging = STAGING.lock().unwrap();
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
        if let Err(e) = std::fs::write(&path, contents) {
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
/// target, removed on cleanup only if we were the ones who created it.
///
/// This is Codex's `auth.json` case: Docker Desktop's virtiofs can't create a
/// bind-mount target nested inside another bind mount (`/home/codex`), so we
/// pre-create the file at `<home>/.codex/auth.json` for Docker to over-mount.
/// If a real `codex login` auth.json already exists there, we leave it alone —
/// only a placeholder we created is removed. Registered for signal cleanup like
/// [`StagedFile`], so an interrupt doesn't leave our placeholder behind.
///
/// Two concurrent auth.json-mode runs on the same profile share this path: the
/// first to exit removes the placeholder under the other. Harmless — each run's
/// bind mount was established at `docker run` time and doesn't need the host
/// file afterwards.
pub struct GuardedPath {
    path: PathBuf,
    created: bool,
    placeholder: String,
}

impl GuardedPath {
    /// Ensure `path` exists as a file. If it was absent, create it with
    /// `placeholder` contents at 0600 and mark it for removal on drop / signal.
    /// If it already existed, leave it untouched and don't remove it later —
    /// unless it holds exactly `placeholder`: that's our own leftover from a
    /// run killed before cleanup (SIGKILL skips both `Drop` and the signal
    /// watcher), so re-adopt it rather than mistake it for a real login file.
    pub fn ensure(path: PathBuf, placeholder: &str) -> Result<Self> {
        Self::ensure_after_register(path, placeholder, |_| Ok(()))
    }

    fn ensure_after_register(
        path: PathBuf,
        placeholder: &str,
        after_register: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<Self> {
        install_signal_handler()?;
        let created = !path.exists()
            || std::fs::read_to_string(&path).is_ok_and(|contents| contents == placeholder);
        if created {
            let _staging = STAGING.lock().unwrap();
            pending().lock().unwrap().push(PendingCleanup::Placeholder {
                path: path.clone(),
                contents: placeholder.to_string(),
            });
            if let Err(e) = after_register(&path) {
                remove_placeholder_and_unregister(&path, placeholder);
                return Err(e);
            }
            if let Err(e) = std::fs::write(&path, placeholder) {
                remove_placeholder_and_unregister(&path, placeholder);
                return Err(e)
                    .with_context(|| format!("pre-create mount target {}", path.display()));
            }
            if let Err(e) = crate::profile::set_600(&path) {
                remove_placeholder_and_unregister(&path, placeholder);
                return Err(e);
            }
        }
        Ok(GuardedPath {
            path,
            created,
            placeholder: placeholder.to_string(),
        })
    }
}

impl Drop for GuardedPath {
    fn drop(&mut self) {
        if self.created {
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
pub fn remove_stale_placeholder(path: &Path, placeholder: &str) {
    if std::fs::read_to_string(path).is_ok_and(|contents| contents == placeholder) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    struct EnvGuard {
        name: &'static str,
        old: Option<OsString>,
    }

    impl EnvGuard {
        fn set(name: &'static str, value: &str) -> Self {
            let old = std::env::var_os(name);
            std::env::set_var(name, value);
            EnvGuard { name, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    fn pending_contains(path: &Path) -> bool {
        pending().lock().unwrap().iter().any(|p| p.path() == path)
    }

    struct CidfileGuard;

    impl Drop for CidfileGuard {
        fn drop(&mut self) {
            clear_child();
        }
    }

    #[test]
    fn watcher_commands_are_bounded() {
        assert_eq!(
            command_quiet("/bin/sh", &["-c", "printf ok"], Duration::from_secs(1)).as_deref(),
            Some("ok")
        );

        let started = Instant::now();
        let out = command_quiet("/bin/sh", &["-c", "sleep 5"], Duration::from_millis(50));

        assert!(
            out.is_none(),
            "timed-out command should be treated as best-effort failure"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "timeout should not wait for the child script to finish"
        );
    }

    #[test]
    fn staging_temp_dir_ignores_empty_or_relative_tmpdir() {
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
    fn wait_current_cid_reads_delayed_cidfile() {
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

    #[test]
    fn container_state_parser_distinguishes_running_stopped_unknown() {
        assert_eq!(
            parse_container_state(Some("true\n")),
            ContainerState::Running
        );
        assert_eq!(
            parse_container_state(Some("false\n")),
            ContainerState::Stopped
        );
        assert_eq!(parse_container_state(Some("")), ContainerState::Unknown);
        assert_eq!(
            parse_container_state(Some("docker error")),
            ContainerState::Unknown
        );
        assert_eq!(parse_container_state(None), ContainerState::Unknown);
    }

    // One test, not two: the pending set is process-global, and
    // `cleanup_pending` unlinks everything registered — interleaving with a
    // parallel test staging its own file must stay within this single test.
    #[test]
    fn staged_file_cleanup_paths() {
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
        let g = GuardedPath::ensure(fresh.clone(), "{}\n").unwrap();
        assert!(g.created, "absent path means we own the file");
        assert!(fresh.is_file());
        assert!(pending_contains(&fresh));
        drop(g);
        assert!(!fresh.exists(), "created placeholder is removed on drop");

        // GuardedPath also registers before it writes placeholder contents, so a
        // failure after registration doesn't leave an untracked cleanup entry.
        let guard_register_fail = dir.path().join("guard-register-fail.json");
        let err = GuardedPath::ensure_after_register(guard_register_fail.clone(), "{}\n", |p| {
            assert!(
                pending_contains(p),
                "placeholder path must be armed before writing contents"
            );
            Err(anyhow::anyhow!(
                "simulated guarded failure after registration"
            ))
        })
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
        let g = GuardedPath::ensure(replaced.clone(), "{}\n").unwrap();
        std::fs::write(&replaced, "{\"OPENAI_API_KEY\":\"real\"}\n").unwrap();
        drop(g);
        assert!(replaced.exists(), "replacement content must not be deleted");
        assert!(!pending_contains(&replaced));

        // GuardedPath is also registered for signal cleanup while held.
        let signal_placeholder = dir.path().join("signal.json");
        let g = GuardedPath::ensure(signal_placeholder.clone(), "{}\n").unwrap();
        assert!(pending_contains(&signal_placeholder));
        cleanup_pending();
        assert!(
            !signal_placeholder.exists(),
            "signal cleanup removes guarded placeholders too"
        );
        drop(g);
        assert!(!pending_contains(&signal_placeholder));

        let signal_replaced = dir.path().join("signal-replaced.json");
        let g = GuardedPath::ensure(signal_replaced.clone(), "{}\n").unwrap();
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
        let g = GuardedPath::ensure(real.clone(), "{}\n").unwrap();
        assert!(!g.created);
        drop(g);
        assert!(real.exists(), "real auth.json must survive");

        // GuardedPath: a leftover placeholder (SIGKILL'd run) is re-adopted
        // and cleaned up instead of being treated as a real file forever.
        let leftover = dir.path().join("leftover.json");
        std::fs::write(&leftover, "{}\n").unwrap();
        let g = GuardedPath::ensure(leftover.clone(), "{}\n").unwrap();
        assert!(g.created, "placeholder contents mean we own the file");
        drop(g);
        assert!(!leftover.exists(), "re-adopted placeholder is removed");
    }
}
