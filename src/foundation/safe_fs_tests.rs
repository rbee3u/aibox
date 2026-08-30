use super::*;

#[test]
fn file_snapshots_enforce_the_read_limit() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("config");
    fs::write(&path, b"12345").unwrap();

    let error = FileSnapshot::capture_with_limit(&path, 4)
        .unwrap_err()
        .to_string();
    assert!(error.contains("exceeds 4 bytes"), "{error}");
    assert_eq!(
        FileSnapshot::capture_with_limit(&path, 5).unwrap().content,
        b"12345"
    );
}

#[test]
fn file_snapshots_distinguish_absence_and_reject_non_files() {
    let root = tempfile::tempdir().unwrap();
    let missing = FileSnapshot::capture_with_limit(&root.path().join("missing"), 16).unwrap();
    assert!(!missing.present);
    assert!(missing.content.is_empty());
    assert_eq!(missing.mode, None);

    let error = FileSnapshot::capture_with_limit(root.path(), 16)
        .unwrap_err()
        .to_string();
    assert!(error.contains("not a regular file"), "{error}");
}

#[cfg(unix)]
#[test]
fn no_follow_file_primitives_reject_symlinks_and_fifos() {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target");
    fs::write(&target, b"target").unwrap();
    let link = root.path().join("link");
    symlink(&target, &link).unwrap();
    assert!(open_real_file(&link, "test file").is_err());
    assert!(create_new_file(&link, "test file", 0o600).is_err());

    let fifo = root.path().join("fifo");
    let fifo_path = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: `fifo_path` is a valid NUL-terminated path and mode has no pointers.
    assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
    assert!(open_real_file(&fifo, "test file").is_err());
    assert!(create_new_file(&fifo, "test file", 0o600).is_err());
}

#[cfg(unix)]
#[test]
fn prepared_atomic_write_publishes_content_and_requested_mode() {
    use std::os::unix::fs::PermissionsExt as _;

    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("metadata.json");
    let mut write =
        PreparedAtomicWrite::new(root.path(), ".metadata-", Some(0o600), "metadata").unwrap();
    write.write_all(b"{}\n").unwrap();
    write.commit(&target, "replace metadata file").unwrap();

    assert_eq!(fs::read(&target).unwrap(), b"{}\n");
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
