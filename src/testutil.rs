//! Shared test helpers for executable stubs, argv assertions, filesystem
//! permissions, and JSONL fixtures.

use std::path::{Path, PathBuf};

/// Write `body` to `dir/name` as an executable stub, returning its path.
#[cfg(unix)]
pub(crate) fn write_stub_script(dir: &Path, name: &str, body: &str) -> PathBuf {
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

/// Return the only item in a slice, with a useful failure when a scenario
/// unexpectedly produces zero or multiple results.
#[track_caller]
pub(crate) fn only<T>(items: &[T]) -> &T {
    let [item] = items else {
        panic!("expected exactly one item, got {}", items.len());
    };
    item
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

/// Make a directory unreadable so tests can exercise `PermissionDenied`, then
/// restore its original mode on drop so failed assertions do not break tempdir
/// cleanup.
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
        Self {
            path: path.to_path_buf(),
            mode,
        }
    }

    /// Restore the mode early, for a test that must assert on a readable tree
    /// after provoking one failure.
    pub(crate) fn restore(self) {
        drop(self);
    }
}

#[cfg(unix)]
impl Drop for UnreadableDir {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;

        let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(self.mode));
    }
}
