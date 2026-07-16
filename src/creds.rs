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
//! `Drop` does **not** run when the process is killed by SIGINT (Ctrl-C) or
//! SIGTERM: the default disposition terminates without unwinding. So we also
//! register every staged path in a process-global set and install a signal
//! handler that unlinks them and re-raises the signal. Between the two, a staged
//! credential is removed when the run finishes, errors, or receives SIGINT /
//! SIGTERM. Uncatchable termination (for example SIGKILL) cannot run process
//! cleanup, so secrets must never be written into profile homes.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Paths to unlink if a fatal signal arrives. A plain `Mutex<Vec<..>>` behind a
/// `OnceLock`; the signal handler only does `unlink`, which is async-signal-safe
/// via `rustix`, and a best-effort `try_lock` (see [`cleanup_from_signal`]).
static PENDING: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
static HANDLER_INSTALLED: OnceLock<()> = OnceLock::new();

fn pending() -> &'static Mutex<Vec<PathBuf>> {
    PENDING.get_or_init(|| Mutex::new(Vec::new()))
}

/// Install SIGINT/SIGTERM handlers (once per process) that unlink every pending
/// staged path and then re-raise the signal with the default disposition, so the
/// exit status still reflects the signal. Idempotent.
fn install_signal_handler() {
    if HANDLER_INSTALLED.set(()).is_err() {
        return; // already installed
    }
    for sig in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        // SAFETY: the handler only calls async-signal-safe operations (unlink via
        // rustix, and re-raise). See `cleanup_from_signal`.
        unsafe {
            let _ = signal_hook::low_level::register(sig, move || {
                cleanup_from_signal();
                // Re-raise with default handler so the process dies as if unhandled.
                let _ = signal_hook::low_level::emulate_default_handler(sig);
            });
        }
    }
}

/// Unlink every pending path. Called from a signal handler, so it avoids
/// allocation and uses only `unlink`. `try_lock` avoids deadlock if we were
/// interrupted mid-`register`; in the worst case a file is missed.
fn cleanup_from_signal() {
    if let Some(lock) = PENDING.get() {
        if let Ok(paths) = lock.try_lock() {
            for p in paths.iter() {
                let _ = rustix::fs::unlink(p);
            }
        }
    }
}

/// Remove `path` and drop it from the pending-cleanup set. Shared by both
/// guards' `Drop`, so the normal-exit cleanup can't diverge between them.
fn remove_and_unregister(path: &Path) {
    let _ = std::fs::remove_file(path);
    if let Ok(mut v) = pending().lock() {
        v.retain(|p| p != path);
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
        install_signal_handler();

        let dir = std::env::var("TMPDIR").unwrap_or_else(|_| "/tmp".to_string());
        // NamedTempFile is created 0600 on Unix. We persist it to a stable path
        // we control so we can hand the path to Docker, then manage deletion
        // ourselves (persist disarms tempfile's own drop-time unlink).
        let named = tempfile::Builder::new()
            .prefix(prefix)
            .rand_bytes(6)
            .tempfile_in(&dir)
            .with_context(|| format!("create temp file in {dir}"))?;
        // Ensure 0600 explicitly (defensive).
        crate::profile::set_600(named.path())?;
        std::fs::write(named.path(), contents)
            .with_context(|| format!("write staged file {}", named.path().display()))?;
        let (_, path) = named
            .keep()
            .map_err(|e| anyhow::anyhow!("persist temp file: {e}"))?;

        pending().lock().unwrap().push(path.clone());
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
pub struct GuardedPath {
    path: PathBuf,
    created: bool,
}

impl GuardedPath {
    /// Ensure `path` exists as a file. If it was absent, create it with
    /// `placeholder` contents at 0600 and mark it for removal on drop / signal.
    /// If it already existed, leave it untouched and don't remove it later.
    pub fn ensure(path: PathBuf, placeholder: &str) -> Result<Self> {
        install_signal_handler();
        let created = !path.exists();
        if created {
            std::fs::write(&path, placeholder)
                .with_context(|| format!("pre-create mount target {}", path.display()))?;
            crate::profile::set_600(&path)?;
            pending().lock().unwrap().push(path.clone());
        }
        Ok(GuardedPath { path, created })
    }
}

impl Drop for GuardedPath {
    fn drop(&mut self) {
        if self.created {
            remove_and_unregister(&self.path);
        }
    }
}
