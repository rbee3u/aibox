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

#[test]
fn nofile_soft_ceiling_never_lowers_and_respects_the_hard_limit() {
    assert_eq!(nofile_soft_to_apply(None, None), None);
    assert_eq!(nofile_soft_to_apply(None, Some(256)), None);
    assert_eq!(nofile_soft_to_apply(Some(256), Some(256)), None);
    assert_eq!(nofile_soft_to_apply(Some(256), Some(1_024)), Some(1_024));
    assert_eq!(
        nofile_soft_to_apply(Some(256), Some(102_400)),
        Some(DESIRED_NOFILE_SOFT)
    );
    assert_eq!(
        nofile_soft_to_apply(Some(256), None),
        Some(DESIRED_NOFILE_SOFT)
    );
    assert_eq!(nofile_soft_to_apply(Some(DESIRED_NOFILE_SOFT), None), None);
    assert_eq!(
        nofile_soft_to_apply(Some(DESIRED_NOFILE_SOFT + 1), None),
        None
    );
}

#[cfg(unix)]
#[test]
fn raise_nofile_limit_does_not_lower_the_inherited_soft_ceiling() {
    let before = rustix::process::getrlimit(rustix::process::Resource::Nofile);
    raise_nofile_limit();
    let after = rustix::process::getrlimit(rustix::process::Resource::Nofile);
    match (before.current, after.current) {
        (None, None) => {}
        (Some(_), None) => {}
        (None, Some(_)) => panic!("raise_nofile_limit replaced an unlimited soft limit"),
        (Some(before), Some(after)) => assert!(after >= before),
    }
}
