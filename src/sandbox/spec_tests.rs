use super::*;
use std::fs;

/// Build an AIBox Root with one initialized Managed Tenant.
fn root_with_tenant(name: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    crate::tenant::ManagedTenant::resolve(root.path(), name)
        .unwrap()
        .ensure_initialized()
        .unwrap();
    root
}

#[test]
fn resolve_accepts_a_workspace_with_read_only_extra_mounts() {
    let root = root_with_tenant("work");
    let workspace = tempfile::tempdir().unwrap();
    let extra = tempfile::tempdir().unwrap();

    let spec = RunSpec::resolve(
        Some(workspace.path().to_str().unwrap()),
        &[format!("{}:/data:ro", extra.path().display())],
        root.path(),
    )
    .unwrap();

    let args = spec.assemble_run_args(Path::new("/abs/tenant"));
    let canonical_workspace = fs::canonicalize(workspace.path()).unwrap();
    let canonical_extra = fs::canonicalize(extra.path()).unwrap();
    assert!(
        args.windows(2).any(|pair| pair[0] == "-v"
            && pair[1] == format!("{}:/workspace", canonical_workspace.display())),
        "{args:?}"
    );
    assert!(
        args.windows(2)
            .any(|pair| pair[0] == "-v"
                && pair[1] == format!("{}:/data:ro", canonical_extra.display())),
        "{args:?}"
    );
}

#[test]
fn resolve_rejects_a_mount_that_shadows_a_managed_target() {
    let root = root_with_tenant("work");
    let workspace = tempfile::tempdir().unwrap();
    let extra = tempfile::tempdir().unwrap();

    let error = RunSpec::resolve(
        Some(workspace.path().to_str().unwrap()),
        &[format!("{}:/workspace:ro", extra.path().display())],
        root.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("would override or shadow"), "{error}");
}

#[test]
fn resolve_rejects_a_workspace_inside_a_named_config_catalog() {
    let root = root_with_tenant("work");
    let catalog = root.path().join("codex/work");
    fs::create_dir_all(&catalog).unwrap();

    let error = RunSpec::resolve(Some(catalog.to_str().unwrap()), &[], root.path())
        .unwrap_err()
        .to_string();

    assert!(error.contains("AIBox internal data"), "{error}");
}

#[test]
fn resolve_rejects_an_extra_mount_source_inside_the_aibox_root() {
    let root = root_with_tenant("work");
    let workspace = tempfile::tempdir().unwrap();
    let catalog = root.path().join("claude/__host");
    fs::create_dir_all(&catalog).unwrap();

    let error = RunSpec::resolve(
        Some(workspace.path().to_str().unwrap()),
        &[format!("{}:/secrets:ro", catalog.display())],
        root.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("AIBox internal data"), "{error}");
}

/// A malformed mount must be rejected during parsing, before any AIBox Root
/// comparison runs. Resolution and validation therefore cannot be reordered
/// without changing which error a caller sees.
#[test]
fn resolve_reports_a_malformed_mount_before_checking_the_aibox_root() {
    let root = root_with_tenant("work");
    let catalog = root.path().join("codex/work");
    fs::create_dir_all(&catalog).unwrap();

    let error = RunSpec::resolve(
        Some(catalog.to_str().unwrap()),
        &["not-a-mount".to_string()],
        root.path(),
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("invalid mount"),
        "mount parsing must precede the AIBox Root check: {error}"
    );
}

/// A Managed Tenant Home subtree is the one path inside the Root a Run may
/// mount, so the boundary check must accept it after full resolution.
#[test]
fn resolve_allows_a_managed_tenant_home_subtree() {
    let root = root_with_tenant("work");
    let nested = root.path().join("tenants/work/projects/demo");
    fs::create_dir_all(&nested).unwrap();

    RunSpec::resolve(Some(nested.to_str().unwrap()), &[], root.path()).unwrap();
}

#[test]
fn resolved_specs_compare_by_value() {
    let root = root_with_tenant("work");
    let workspace = tempfile::tempdir().unwrap();
    let path = workspace.path().to_str().unwrap();

    let first = RunSpec::resolve(Some(path), &[], root.path()).unwrap();
    let second = RunSpec::resolve(Some(path), &[], root.path()).unwrap();

    assert_eq!(first, second);
}
