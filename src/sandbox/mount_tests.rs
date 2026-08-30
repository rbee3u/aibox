use super::*;
use std::fs;
use std::path::PathBuf;

#[test]
fn resolve_workspace_none_uses_cwd() {
    let got = resolve_workspace(None).unwrap();
    assert_eq!(
        got,
        std::fs::canonicalize(std::env::current_dir().unwrap())
            .unwrap()
            .to_string_lossy()
    );
}

#[test]
fn resolve_workspace_absolutizes_relative_paths() {
    let cwd = std::env::current_dir().unwrap();

    let got = resolve_workspace(Some("src")).unwrap();

    assert_eq!(
        got,
        std::fs::canonicalize(cwd.join("src"))
            .unwrap()
            .to_string_lossy()
    );
}

#[test]
fn resolve_workspace_rejects_missing_and_non_directory_paths() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file");
    let missing = dir.path().join("missing");
    fs::write(&file, "not a directory\n").unwrap();

    for path in [&file, &missing] {
        let error = resolve_workspace(Some(path.to_str().unwrap()))
            .unwrap_err()
            .to_string();
        assert!(error.contains("workspace is not a directory"), "{error}");
        assert!(
            error.contains(&path.display().to_string()),
            "the rejected path should be identifiable: {error}"
        );
    }
}

#[test]
fn resolve_mounts_absolutizes_and_validates_host_side() {
    let cwd = std::env::current_dir().unwrap();
    let got = resolve_mounts(&["src:/src".to_string(), "src:/readonly:ro".to_string()]).unwrap();
    assert_eq!(
        got,
        vec![
            format!(
                "{}:/src",
                std::fs::canonicalize(cwd.join("src")).unwrap().display()
            ),
            format!(
                "{}:/readonly:ro",
                std::fs::canonicalize(cwd.join("src")).unwrap().display()
            )
        ]
    );

    let error = resolve_mounts(&["/no/such/dir:/data".to_string()])
        .unwrap_err()
        .to_string();
    assert!(error.contains("mount host path does not exist"), "{error}");
}

#[cfg(unix)]
#[test]
fn resolved_bind_sources_do_not_leave_symlinks_for_docker() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    let link = dir.path().join("link");
    fs::create_dir(&target).unwrap();
    symlink(&target, &link).unwrap();
    let canonical_target = fs::canonicalize(&target).unwrap();

    assert_eq!(
        resolve_workspace(Some(link.to_str().unwrap())).unwrap(),
        canonical_target.to_string_lossy()
    );
    assert_eq!(
        resolve_mounts(&[format!("{}:/data:ro", link.display())]).unwrap(),
        [format!("{}:/data:ro", canonical_target.display())]
    );
}

#[test]
fn resolve_mounts_rejects_malformed_short_syntax() {
    for (mount, expected) in [
        ("src", "invalid mount"),
        (":/cache", "invalid mount"),
        ("src:", "invalid mount"),
        ("src:relative", "invalid mount"),
        ("src:/cache:", "invalid mount mode"),
        ("src:/cache:rw", "only :ro is supported"),
        ("src:/cache:ro:extra", "invalid mount"),
    ] {
        let error = resolve_mounts(&[mount.to_string()])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(expected),
            "{mount:?} should fail with {expected:?}, got {error:?}"
        );
    }
}

#[test]
fn extra_mounts_must_not_replace_managed_targets() {
    for target in [
        "/workspace",
        "/workspace/",
        "/workspace/.",
        "/tmp/../workspace",
        "/",
        "/home",
        "/home/aibox",
        "/home/aibox/",
        "/home/aibox/.",
        "/home/aibox/..",
        "/home/aibox/.cache/../..",
    ] {
        let error = validate_extra_mount_targets(&[format!("/host:{target}:ro")])
            .unwrap_err()
            .to_string();
        assert!(error.contains("would override or shadow"));
    }
    validate_extra_mount_targets(&[
        "/host:/legacy-work:ro".to_string(),
        "/host:/workspace-cache:ro".to_string(),
        "/host:/workspace/.cache:ro".to_string(),
        "/host:/home/aibox-cache:ro".to_string(),
        "/host:/home/aibox2:ro".to_string(),
        "/host:/home/aibox/.cache:ro".to_string(),
    ])
    .unwrap();
}

#[test]
fn aibox_mount_sources_only_allow_managed_tenant_home_trees() {
    let root = tempfile::tempdir().unwrap();
    crate::tenant::ManagedTenant::resolve(root.path(), "default")
        .unwrap()
        .ensure_initialized()
        .unwrap();
    let tenant_home = root.path().join("tenants/default");
    let home_child = tenant_home.join("projects/demo");
    let codex_catalog = root.path().join("codex/default");
    let host_catalog = root.path().join("claude/__host");
    fs::create_dir_all(&home_child).unwrap();
    fs::create_dir_all(&codex_catalog).unwrap();
    fs::create_dir_all(&host_catalog).unwrap();

    for rejected in [
        root.path(),
        &root.path().join("tenants"),
        &codex_catalog,
        &root.path().join("codex"),
        &root.path().join("claude"),
        &host_catalog,
        root.path().parent().unwrap(),
    ] {
        let error = validate_aibox_mount_sources(rejected.to_str().unwrap(), &[], root.path())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("AIBox internal data"),
            "{rejected:?}: {error}"
        );
    }

    validate_aibox_mount_sources(tenant_home.to_str().unwrap(), &[], root.path()).unwrap();
    validate_aibox_mount_sources(home_child.to_str().unwrap(), &[], root.path()).unwrap();
}

#[cfg(unix)]
#[test]
fn aibox_mount_check_resolves_symlinked_sources() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let links = tempfile::tempdir().unwrap();
    crate::tenant::ManagedTenant::resolve(root.path(), "default")
        .unwrap()
        .ensure_initialized()
        .unwrap();
    let catalog = root.path().join("codex/default");
    fs::create_dir_all(&catalog).unwrap();
    let linked_config = links.path().join("config");
    symlink(&catalog, &linked_config).unwrap();

    let error = validate_aibox_mount_sources(linked_config.to_str().unwrap(), &[], root.path())
        .unwrap_err()
        .to_string();

    assert!(error.contains("AIBox internal data"), "{error}");
}

#[cfg(unix)]
#[test]
fn aibox_mount_check_rejects_invalid_tenant_collection_entries() {
    let root = tempfile::tempdir().unwrap();
    let invalid_home = root.path().join("tenants/bad_name");
    fs::create_dir_all(&invalid_home).unwrap();

    let error = validate_aibox_mount_sources(invalid_home.to_str().unwrap(), &[], root.path())
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("only Managed Tenant Home trees may be mounted"),
        "{error}"
    );
}

#[test]
fn aibox_mount_check_normalizes_dotdot_sources() {
    let root = tempfile::tempdir().unwrap();
    crate::tenant::ManagedTenant::resolve(root.path(), "default")
        .unwrap()
        .ensure_initialized()
        .unwrap();
    let tenant_home = root.path().join("tenants/default");
    let catalog = root.path().join("codex/default");
    fs::create_dir_all(&catalog).unwrap();

    let workspace = tenant_home.join("..");
    let error = validate_aibox_mount_sources(workspace.to_str().unwrap(), &[], root.path())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("AIBox internal data"),
        "a dotdot Workspace path that resolves to the tenant root must fail: {error}"
    );

    let mount_source = tenant_home.join("../../codex/default");
    let error = validate_aibox_mount_sources(
        tenant_home.to_str().unwrap(),
        &[format!("{}:/secrets:ro", mount_source.display())],
        root.path(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("AIBox internal data"),
        "a dotdot mount source that resolves into a Named Config catalog must be rejected: {error}"
    );
}

#[test]
fn aibox_mount_check_allows_unrelated_paths() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();

    validate_aibox_mount_sources(
        outside.path().to_str().unwrap(),
        &[format!("{}:/archive:ro", outside.path().display())],
        root.path(),
    )
    .unwrap();
}

#[test]
fn aibox_mount_check_rejects_an_ancestor_before_the_root_exists() {
    let parent = tempfile::tempdir().unwrap();
    let future_root = parent.path().join("future/.aibox");

    let error = validate_aibox_mount_sources(parent.path().to_str().unwrap(), &[], &future_root)
        .unwrap_err()
        .to_string();

    assert!(error.contains("AIBox internal data"), "{error}");
    assert!(!future_root.exists());
}

#[test]
fn tenant_home_children_are_valid_extra_mount_sources() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    crate::tenant::ManagedTenant::resolve(root.path(), "work")
        .unwrap()
        .ensure_initialized()
        .unwrap();
    let home_child = root.path().join("tenants/work/projects/demo");
    fs::create_dir_all(&home_child).unwrap();

    validate_aibox_mount_sources(
        outside.path().to_str().unwrap(),
        &[format!("{}:/demo:ro", home_child.display())],
        root.path(),
    )
    .unwrap();
}

#[test]
fn reject_bind_sources_that_short_syntax_cannot_represent() {
    let parent = tempfile::tempdir().unwrap();
    let colon_dir = parent.path().join("a:b");
    fs::create_dir(&colon_dir).unwrap();
    let error = resolve_workspace(Some(colon_dir.to_str().unwrap()))
        .unwrap_err()
        .to_string();
    assert!(error.contains("contains ':'"));

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;

        let opaque = PathBuf::from(std::ffi::OsString::from_vec(vec![
            b'w', b'o', b'r', b'k', 0xff,
        ]));
        let error = reject_colon_in_bind_source("mount host", &opaque)
            .unwrap_err()
            .to_string();
        assert!(error.contains("not valid UTF-8"), "{error}");
        assert!(
            error.contains("cannot be represented safely for docker"),
            "{error}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn resolve_workspace_rejects_a_symlink_to_a_non_utf8_bind_source() {
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;

    let parent = tempfile::tempdir().unwrap();
    let opaque_dir = parent.path().join(std::ffi::OsString::from_vec(vec![
        b'w', b'o', b'r', b'k', 0xff,
    ]));
    let link = parent.path().join("work-link");
    fs::create_dir(&opaque_dir).unwrap();
    symlink(&opaque_dir, &link).unwrap();

    let error = resolve_workspace(Some(link.to_str().unwrap()))
        .unwrap_err()
        .to_string();

    assert!(error.contains("not valid UTF-8"), "{error}");
    assert!(
        error.contains("cannot be represented safely for docker"),
        "{error}"
    );
}
