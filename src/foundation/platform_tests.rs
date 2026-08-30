use super::*;

#[cfg(unix)]
#[test]
fn uid_gid_reports_the_invoking_process_identity() {
    assert_eq!(
        uid_gid(),
        (
            rustix::process::getuid().as_raw(),
            rustix::process::getgid().as_raw()
        )
    );
}
