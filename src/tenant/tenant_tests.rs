use super::host::{aibox_root_from, host_home_from};
use super::identity::ManagedTenantName;
use super::layout::HOST_STORAGE_KEY;
use super::*;
use crate::agent::AgentKind;
use std::{ffi::OsStr, fs, path::Path};

#[test]
fn root_and_home_resolution_use_only_explicit_inputs() {
    let cwd = Path::new("/workspace/project");
    assert_eq!(
        aibox_root_from(Some(OsStr::new("../state")), None, cwd).unwrap(),
        Path::new("/workspace/state")
    );
    assert_eq!(
        aibox_root_from(None, Some(OsStr::new("/host/home")), cwd).unwrap(),
        Path::new("/host/home/.aibox")
    );
    assert!(
        aibox_root_from(Some(OsStr::new("")), Some(OsStr::new("/host/home")), cwd)
            .unwrap_err()
            .to_string()
            .contains("AIBOX_ROOT is set but empty")
    );
    assert!(
        host_home_from(None, cwd)
            .unwrap_err()
            .to_string()
            .contains("HOME is not set")
    );
}

#[test]
fn names_are_lowercase_dns_labels() {
    for valid in ["a", "work-1", &"a".repeat(63)] {
        assert!(is_safe_name(valid), "{valid}");
    }
    for invalid in [
        "",
        "Work",
        "work_1",
        "-work",
        "work-",
        HOST_STORAGE_KEY,
        &"a".repeat(64),
    ] {
        assert!(!is_safe_name(invalid), "{invalid}");
    }

    assert_eq!(
        validate_name("tenant", "Work").unwrap_err().to_string(),
        "invalid tenant name 'Work': expected a 1-63 character lowercase DNS label"
    );
}

#[test]
fn tenant_selection_decodes_only_the_canonical_wire_keys() {
    assert_eq!(
        TenantSelection::parse("host").unwrap(),
        TenantSelection::Host
    );
    assert_eq!(
        TenantSelection::parse("managed:work-1").unwrap(),
        TenantSelection::Managed(ManagedTenantName::parse("work-1").unwrap())
    );
    assert_eq!(
        TenantSelection::parse("work-1").unwrap_err().to_string(),
        "unknown Tenant selection: work-1"
    );
    assert!(TenantSelection::parse("managed:Work").is_err());
}

#[test]
fn initialization_publishes_direct_home_layout() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    assert_eq!(tenant.home_dir, root.path().join("tenants/work"));
    assert!(tenant.home_dir.join(".gitconfig").is_file());
    assert!(!tenant.home_dir.join(".config").exists());
    assert!(tenant.home_dir.join(".codex").is_dir());
    assert!(!tenant.home_dir.join(".bash_profile").exists());
    assert!(!tenant.home_dir.join(".bashrc").exists());
    assert!(!tenant.home_dir.join(".claude/statusline.sh").exists());
    assert_eq!(list_tenants(root.path()).unwrap(), ["work"]);
}

#[test]
fn initialization_repairs_baseline_without_overwriting_user_files() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let gitconfig = tenant.home_dir.join(".gitconfig");
    let bash_profile = tenant.home_dir.join(".bash_profile");
    let bashrc = tenant.home_dir.join(".bashrc");
    fs::write(&gitconfig, b"[user]\nname = Keep Me\n").unwrap();
    fs::write(&bash_profile, b"export PROFILE_OWNER=user\n").unwrap();
    fs::write(&bashrc, b"export BASHRC_OWNER=user\n").unwrap();
    fs::remove_dir(tenant.home_dir.join(".claude")).unwrap();

    tenant.ensure_initialized().unwrap();

    assert_eq!(fs::read(&gitconfig).unwrap(), b"[user]\nname = Keep Me\n");
    assert_eq!(
        fs::read(&bash_profile).unwrap(),
        b"export PROFILE_OWNER=user\n"
    );
    assert_eq!(fs::read(&bashrc).unwrap(), b"export BASHRC_OWNER=user\n");
    assert!(tenant.home_dir.join(".claude").is_dir());
    assert!(tenant.home_dir.join(".codex").is_dir());
}

#[test]
fn initialization_rolls_stale_tenant_transitions_forward() {
    let root = tempfile::tempdir().unwrap();
    let tenants = root.path().join(TENANTS_DIR);
    fs::create_dir(&tenants).unwrap();
    let creating = tenants.join("$creating-work");
    let deleting = tenants.join("$deleting-work");
    fs::create_dir(&creating).unwrap();
    fs::create_dir(&deleting).unwrap();
    fs::write(creating.join("preserved"), b"staged").unwrap();
    fs::write(deleting.join("discarded"), b"old").unwrap();

    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();

    assert!(tenant.home_dir.join("preserved").is_file());
    assert!(!creating.exists());
    assert!(!deleting.exists());
    assert_eq!(list_tenants(root.path()).unwrap(), ["work"]);
}

#[test]
fn delete_all_converges_interrupted_tenant_transitions() {
    let root = tempfile::tempdir().unwrap();
    let tenants = root.path().join(TENANTS_DIR);
    fs::create_dir(&tenants).unwrap();
    fs::create_dir(tenants.join("$deleting-gone")).unwrap();
    fs::write(tenants.join("$deleting-gone/auth.json"), b"secret").unwrap();
    fs::create_dir(tenants.join("$creating-half")).unwrap();

    assert!(list_tenants(root.path()).unwrap().is_empty());
    delete_tenants(root.path(), &[], true).unwrap();

    assert!(!tenants.join("$deleting-gone").exists());
    assert!(!tenants.join("$creating-half").exists());
}

#[cfg(unix)]
#[test]
fn newly_created_boundary_directories_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("new-aibox-root");
    let tenant = ManagedTenant::resolve(&root, "work").unwrap();
    tenant.ensure_initialized().unwrap();

    for path in [
        root.clone(),
        root.join(TENANTS_DIR),
        tenant.home_dir.clone(),
        tenant.home_dir.join(".claude"),
        tenant.home_dir.join(".codex"),
    ] {
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700,
            "{}",
            path.display()
        );
    }
}

#[cfg(unix)]
#[test]
fn initialization_ignores_a_legacy_managed_environment() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    fs::create_dir_all(tenant.home_dir.join(".config/aibox")).unwrap();
    let environment = tenant.home_dir.join(".config/aibox/env.sh");
    symlink(outside.path().join("env.sh"), &environment).unwrap();

    tenant.ensure_initialized().unwrap();

    assert!(fs::symlink_metadata(&environment).unwrap().is_symlink());
    assert!(!outside.path().join("env.sh").exists());
}

#[test]
fn initialization_preserves_a_legacy_managed_environment_file() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let environment = tenant.home_dir.join(".config/aibox/env.sh");
    fs::create_dir_all(environment.parent().unwrap()).unwrap();
    fs::write(&environment, b"legacy bytes\n").unwrap();

    tenant.ensure_initialized().unwrap();

    assert_eq!(fs::read(environment).unwrap(), b"legacy bytes\n");
}

#[test]
fn initialization_ignores_an_abnormal_legacy_configuration_entry() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let configuration = tenant.home_dir.join(".config");
    fs::write(&configuration, b"not a directory\n").unwrap();

    tenant.ensure_initialized().unwrap();

    assert_eq!(fs::read(configuration).unwrap(), b"not a directory\n");
}

#[test]
fn host_and_managed_storage_keys_do_not_collide() {
    let root = tempfile::tempdir().unwrap();
    let managed = ManagedTenant::resolve(root.path(), "host").unwrap();
    assert_eq!(
        managed
            .for_agent(AgentKind::Codex)
            .named_config_catalog_dir(),
        root.path().join("codex/host")
    );
    let host = Tenant::Host {
        home_dir: root.path().to_path_buf(),
        root_dir: root.path().to_path_buf(),
    };
    assert_eq!(
        host.for_agent(AgentKind::Codex).named_config_catalog_dir(),
        root.path().join("codex/__host")
    );
}

#[test]
fn listing_is_read_only_and_ignores_unrecognized_entries() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("unrelated")).unwrap();
    let missing_root = root.path().join("missing");
    assert!(list_tenants(&missing_root).unwrap().is_empty());
    assert!(
        !missing_root.exists(),
        "listing a missing root must not initialize it"
    );
    fs::create_dir(root.path().join(TENANTS_DIR)).unwrap();
    fs::write(root.path().join("tenants/not-a-dir"), b"x").unwrap();
    fs::create_dir(root.path().join("tenants/bad_name")).unwrap();
    assert!(list_tenants(root.path()).unwrap().is_empty());
}

#[test]
fn create_and_delete_are_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    tenant.ensure_initialized().unwrap();
    for agent in AgentKind::ALL {
        let catalog = root.path().join(agent.tag()).join("work");
        fs::create_dir_all(&catalog).unwrap();
        fs::write(catalog.join("metadata.json"), b"config metadata").unwrap();
    }
    delete_tenants(root.path(), &["work".to_string()], false).unwrap();
    delete_tenants(root.path(), &["work".to_string()], false).unwrap();
    assert!(!tenant.home_dir.exists());
    assert!(!root.path().join("claude/work").exists());
    assert!(!root.path().join("codex/work").exists());
}

#[test]
fn tenant_deletion_requires_explicit_selection() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let empty = delete_tenants(root.path(), &[], false)
        .unwrap_err()
        .to_string();
    assert!(empty.contains("at least one Tenant"), "{empty}");
    let mixed = delete_tenants(root.path(), &["work".to_string()], true)
        .unwrap_err()
        .to_string();
    assert!(mixed.contains("--all cannot be combined"), "{mixed}");
    assert!(
        list_tenants(root.path())
            .unwrap()
            .contains(&"work".to_string())
    );
}

#[test]
fn default_managed_tenant_is_protected_from_explicit_and_all_deletion() {
    let root = tempfile::tempdir().unwrap();
    for name in [DEFAULT_TENANT_NAME, "host", "work"] {
        ManagedTenant::resolve(root.path(), name)
            .unwrap()
            .ensure_initialized()
            .unwrap();
    }

    let error = delete_tenants(root.path(), &[DEFAULT_TENANT_NAME.to_string()], false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("protected"), "{error}");

    delete_tenants(root.path(), &["host".to_string()], false).unwrap();
    assert!(!root.path().join("tenants/host").exists());

    delete_tenants(root.path(), &[], true).unwrap();
    assert!(root.path().join("tenants/default").is_dir());
    assert!(!root.path().join("tenants/work").exists());
}

#[test]
fn deleting_from_an_absent_root_is_idempotent_and_read_only() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("missing");

    delete_tenants(&root, &["work".to_string()], false).unwrap();
    delete_tenants(&root, &["work".to_string()], false).unwrap();

    assert!(!root.exists());
}

#[cfg(unix)]
#[test]
fn tenant_collection_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.path().join(TENANTS_DIR)).unwrap();

    let list_error = list_tenants(root.path()).unwrap_err().to_string();
    assert!(list_error.contains("not a real directory"), "{list_error}");
    let delete_error = delete_tenants(root.path(), &["work".to_string()], false)
        .unwrap_err()
        .to_string();
    assert!(
        delete_error.contains("not a real directory"),
        "{delete_error}"
    );
    assert!(!outside.path().join("work").exists());
}

#[cfg(unix)]
#[test]
fn orphaned_config_catalog_symlinks_block_tenant_publication() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("keep"), b"outside").unwrap();
    fs::create_dir(root.path().join("claude")).unwrap();
    symlink(outside.path(), root.path().join("claude/work")).unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();

    let error = tenant.ensure_initialized().unwrap_err().to_string();

    assert!(error.contains("not a real directory"), "{error}");
    assert!(
        !tenant.home_dir.exists(),
        "an unsafe orphan must be rejected before publishing a new identity"
    );
    assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
}

#[cfg(unix)]
#[test]
fn linked_config_collection_is_rejected_before_orphan_cleanup() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(outside.path().join("work")).unwrap();
    fs::write(outside.path().join("work/keep"), b"outside").unwrap();
    symlink(outside.path(), root.path().join("claude")).unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();

    let error = tenant.ensure_initialized().unwrap_err().to_string();

    assert!(error.contains("not a real directory"), "{error}");
    assert!(!tenant.home_dir.exists());
    assert_eq!(
        fs::read(outside.path().join("work/keep")).unwrap(),
        b"outside"
    );
}

#[cfg(unix)]
#[test]
fn interrupted_delete_rejects_linked_config_catalog_and_rolls_forward_safely() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("keep"), b"outside").unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    fs::create_dir(root.path().join("claude")).unwrap();
    let linked_catalog = root.path().join("claude/work");
    symlink(outside.path(), &linked_catalog).unwrap();

    let error = delete_tenants(root.path(), &["work".to_string()], false)
        .unwrap_err()
        .to_string();

    assert!(error.contains("not a real directory"), "{error}");
    assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
    assert!(!tenant.home_dir.exists());
    assert!(root.path().join("tenants/$deleting-work").is_dir());

    fs::remove_file(linked_catalog).unwrap();
    delete_tenants(root.path(), &["work".to_string()], false).unwrap();

    assert!(!root.path().join("tenants/$deleting-work").exists());
    assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
}

#[cfg(unix)]
#[test]
fn symlinked_tenant_home_is_rejected_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(TENANTS_DIR)).unwrap();
    fs::write(outside.path().join("keep"), b"outside").unwrap();
    symlink(outside.path(), root.path().join("tenants/work")).unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();

    let init_error = tenant.ensure_initialized().unwrap_err().to_string();
    assert!(init_error.contains("not a real directory"), "{init_error}");
    let delete_error = delete_tenants(root.path(), &["work".to_string()], false)
        .unwrap_err()
        .to_string();
    assert!(
        delete_error.contains("not a real directory"),
        "{delete_error}"
    );
    assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
}

#[test]
fn ensure_agent_state_creates_only_the_selected_agent_state_directory() {
    let home = tempfile::tempdir().unwrap();
    ensure_agent_state(AgentKind::Codex, home.path()).unwrap();
    assert!(home.path().join(".codex").is_dir());
    assert!(!home.path().join(".codex/statusline.sh").exists());

    let home = tempfile::tempdir().unwrap();
    ensure_agent_state(AgentKind::Claude, home.path()).unwrap();
    assert!(home.path().join(".claude").is_dir());
    assert!(!home.path().join(".claude/statusline.sh").exists());
}
