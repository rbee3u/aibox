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
