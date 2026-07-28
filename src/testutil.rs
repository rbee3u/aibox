//! Shared test scaffolding.
//!
//! [`EnvGuard`]: nearly every module's tests need to set an env var
//! (`$AIBOX_ROOT`, `$HOME`, `$TMPDIR`, `$PATH`) for the duration of one
//! test and put it back afterwards. Five near-identical copies had drifted apart
//! in what they supported; one shared version keeps a test from reaching for a
//! helper its module's copy happens to lack.
//!
//! [`write_stub_script`]: run-path tests stub `docker` on `$PATH` (the check
//! AGENTS.md prescribes for anything the unit tests can't reach). Nine copies of
//! the same write-then-chmod-0755 boilerplate had accumulated; the *scripts*
//! stay in their own modules, since each encodes what that test needs Docker to
//! do, but the mechanics live here once.
//!
//! [`contains_pair`] / [`pair_pos`]: argv assertions for flag/value pairs such
//! as `-v src:dst`. Multiple modules need the same two-token window search.
//!
//! [`write_jsonl`]: the two session backends' tests each write a transcript
//! fixture from a list of JSON lines; the mkdir-then-writeln mechanics are the
//! same, only the relative path and the lines differ.
//!
//! Env vars are process-global, so a test that installs a guard must also hold
//! [`crate::test_env_lock`] to keep a parallel test from observing the change.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Write `body` to `dir/name` as an executable stub, returning its path. Used to
/// put a fake `docker` on `$PATH` (with [`EnvGuard::prepend_path`]).
#[cfg(unix)]
pub(crate) fn write_stub_script(dir: &Path, name: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    std::fs::write(&path, body).expect("write stub script");
    let mut perms = std::fs::metadata(&path)
        .expect("stat stub script")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&path, perms).expect("chmod stub script");
    path
}

/// True if `args` contains `a` immediately followed by `b` — the shape every
/// Docker/agent flag-with-value takes in an assembled argv.
pub(crate) fn contains_pair(args: &[String], a: &str, b: &str) -> bool {
    pair_pos(args, a, b).is_some()
}

/// The index of the `a` in an `a b` pair, for asserting relative order.
pub(crate) fn pair_pos(args: &[String], a: &str, b: &str) -> Option<usize> {
    args.windows(2).position(|w| w[0] == a && w[1] == b)
}

/// Write a JSONL transcript fixture at `dir/rel`, one `lines` entry per line,
/// creating parent directories. Shared by both session backends' tests.
pub(crate) fn write_jsonl(dir: &Path, rel: &str, lines: &[&str]) -> PathBuf {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("transcript has a parent"))
        .expect("create transcript dir");
    let mut body = String::new();
    for line in lines {
        body.push_str(line);
        body.push('\n');
    }
    std::fs::write(&path, body).expect("write transcript");
    path
}

/// Makes a directory unreadable/unsearchable (mode 0) and restores its original
/// mode on drop. This is how the tests reach the `Err(e)` arms that report a
/// real `PermissionDenied` from `lstat`/`read_dir` — the paths that distinguish
/// "this is broken" from "this is absent", which a plain missing path can't
/// exercise. Restoring on drop (rather than at the end of the test body) keeps a
/// failing assertion from leaving a directory the tempdir cleanup can't remove.
#[cfg(unix)]
pub(crate) struct UnreadableDir {
    path: PathBuf,
    mode: u32,
}

#[cfg(unix)]
impl UnreadableDir {
    pub(crate) fn new(path: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;

        let mode = std::fs::metadata(path)
            .expect("stat dir to lock")
            .permissions()
            .mode();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000))
            .expect("chmod 000 dir");
        UnreadableDir {
            path: path.to_path_buf(),
            mode,
        }
    }

    /// Restore the mode early, for a test that must assert on a readable tree
    /// after provoking one failure.
    pub(crate) fn restore(self) {}
}

#[cfg(unix)]
impl Drop for UnreadableDir {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;

        let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(self.mode));
    }
}

/// Sets (or removes) one env var, restoring the previous value on drop.
pub(crate) struct EnvGuard {
    name: &'static str,
    old: Option<OsString>,
}

impl EnvGuard {
    pub(crate) fn set(name: &'static str, value: impl Into<OsString>) -> Self {
        let old = std::env::var_os(name);
        std::env::set_var(name, value.into());
        EnvGuard { name, old }
    }

    pub(crate) fn remove(name: &'static str) -> Self {
        let old = std::env::var_os(name);
        std::env::remove_var(name);
        EnvGuard { name, old }
    }

    /// Put `dir` first on `$PATH`, so a stub `docker` there wins over a real one.
    pub(crate) fn prepend_path(dir: &Path) -> Self {
        let old = std::env::var_os("PATH");
        let mut paths = vec![dir.to_path_buf()];
        if let Some(old_path) = &old {
            paths.extend(std::env::split_paths(old_path));
        }
        let joined = std::env::join_paths(paths).expect("join PATH");
        std::env::set_var("PATH", joined);
        EnvGuard { name: "PATH", old }
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
