use super::*;
use crate::agent::AgentKind;
use crate::tenant::{ManagedTenant, Tenant};
use std::fs;

#[test]
fn component_specs_validate_versions() {
    assert_eq!(
        "rust@1.2.3".parse::<ComponentSpec>().unwrap().to_string(),
        "rust@1.2.3"
    );
    assert!("claude-statusline@1.2.3".parse::<ComponentSpec>().is_err());
    assert!("rust@01.2.3".parse::<ComponentSpec>().is_err());
    for component in ["node", "codex", "claude", "python"] {
        assert_eq!(
            format!("{component}@1.2.3")
                .parse::<ComponentSpec>()
                .unwrap()
                .to_string(),
            format!("{component}@1.2.3")
        );
    }
}

#[test]
fn missing_managed_catalog_is_read_only_and_reports_components_uninstalled() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    let inspection = inspect_catalog(&Tenant::Managed(tenant)).unwrap();
    assert_eq!(inspection.len(), 8);
    assert!(
        inspection
            .iter()
            .all(|item| item.status == Some(ComponentStatus::NotInstalled))
    );
    assert!(!root.path().join("tenants/work").exists());
}

#[cfg(unix)]
#[test]
fn tenant_environment_snapshot_accepts_only_installed_and_collects_inspection_errors() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();

    fs::create_dir(tenant.home_dir.join(".node")).unwrap();
    fs::create_dir(tenant.home_dir.join(".rustup")).unwrap();
    fs::write(tenant.home_dir.join(".rustup/settings.toml"), [0xff, 0xfe]).unwrap();
    fs::create_dir(tenant.home_dir.join(".goroot")).unwrap();
    fs::create_dir(tenant.home_dir.join(".goroot/bin")).unwrap();
    fs::write(tenant.home_dir.join(".goroot/VERSION"), b"go1.2.3\n").unwrap();
    make_executable(&tenant.home_dir.join(".goroot/bin/go"));

    let (components, warnings) = inspect_tenant_environment_components(&tenant.home_dir);

    assert!(!components.node());
    assert!(!components.claude());
    assert!(!components.python());
    assert!(!components.rust());
    assert!(components.go());
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("rust Component"), "{warnings:?}");
    assert!(warnings[0].contains("not UTF-8"), "{warnings:?}");
}

#[test]
fn statusline_install_and_remove_manage_only_owned_state() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    let selected = Tenant::Managed(tenant);
    install_component(
        &selected,
        &ComponentSpec {
            kind: ComponentKind::ClaudeStatusline,
            version: None,
        },
    )
    .unwrap();
    let status = inspect_catalog(&selected)
        .unwrap()
        .into_iter()
        .find(|item| item.kind == ComponentKind::ClaudeStatusline)
        .unwrap();
    assert_eq!(
        status.status,
        Some(ComponentStatus::Installed { version: None })
    );
    remove_component(&selected, ComponentKind::ClaudeStatusline).unwrap();
    assert_eq!(
        inspect_catalog(&selected)
            .unwrap()
            .into_iter()
            .find(|item| item.kind == ComponentKind::ClaudeStatusline)
            .unwrap()
            .status,
        Some(ComponentStatus::NotInstalled)
    );
}

#[test]
fn host_catalog_contains_only_statuslines_and_rejects_toolchains() {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let selected = Tenant::Host {
        home_dir: home.path().to_path_buf(),
        root_dir: root.path().to_path_buf(),
    };
    let catalog = inspect_catalog(&selected).unwrap();
    assert_eq!(catalog.len(), 2);
    let error = install_component(
        &selected,
        &ComponentSpec {
            kind: ComponentKind::Rust,
            version: None,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("Host Tenant"), "{error}");
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, b"fixture\n").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn install_python_fixture(home: &std::path::Path, version: &str, uv_version: &str) {
    use std::os::unix::fs::symlink;

    let platform = expected_python_platform().unwrap();
    let architecture = match platform {
        "x86_64-unknown-linux-gnu" => "x86_64",
        "aarch64-unknown-linux-gnu" => "aarch64",
        _ => unreachable!(),
    };
    let root = home.join(".python");
    let uv_release = root.join(format!("uv/releases/v{uv_version}"));
    fs::create_dir_all(&uv_release).unwrap();
    make_executable(&uv_release.join("uv"));
    make_executable(&uv_release.join("uvx"));

    let python_release = root.join(format!(
        "cpython/releases/cpython-{version}-linux-{architecture}-gnu"
    ));
    fs::create_dir_all(python_release.join("bin")).unwrap();
    let minor = version.rsplit_once('.').unwrap().0;
    let python = python_release.join(format!("bin/python{minor}"));
    make_executable(&python);
    fs::create_dir_all(root.join("bin")).unwrap();

    let generation_name = format!("python-{version}__uv-{uv_version}__{platform}__123-456");
    let generation_bin = root.join("generations").join(&generation_name).join("bin");
    fs::create_dir_all(&generation_bin).unwrap();
    symlink(&python, generation_bin.join("python")).unwrap();
    symlink(&python, generation_bin.join("python3")).unwrap();
    symlink(&python, generation_bin.join(format!("python{minor}"))).unwrap();
    symlink(uv_release.join("uv"), generation_bin.join("uv")).unwrap();
    symlink(uv_release.join("uvx"), generation_bin.join("uvx")).unwrap();
    make_executable(&generation_bin.join("pip"));
    make_executable(&generation_bin.join("pip3"));
    fs::write(
        generation_bin.parent().unwrap().join("pyvenv.cfg"),
        format!("home = {}\n", python_release.join("bin").display()),
    )
    .unwrap();
    fs::create_dir_all(
        generation_bin
            .parent()
            .unwrap()
            .join(format!("lib/python{minor}/site-packages/pip")),
    )
    .unwrap();
    symlink(
        format!("generations/{generation_name}"),
        root.join("current"),
    )
    .unwrap();

    let local_bin = home.join(".local/bin");
    fs::create_dir_all(&local_bin).unwrap();
    for name in [
        "uv",
        "uvx",
        "python",
        "python3",
        &format!("python{minor}"),
        "pip",
        "pip3",
    ] {
        let launcher = local_bin.join(name);
        if let Some(content) = python_launcher_wrapper(name) {
            make_executable(&launcher);
            fs::write(launcher, content).unwrap();
        } else {
            symlink(format!("/home/aibox/.python/current/bin/{name}"), launcher).unwrap();
        }
    }
}

#[cfg(unix)]
#[test]
fn native_runtime_layouts_derive_installed_versions_without_executing_binaries() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();

    let node_release = tenant.home_dir.join(".node/releases/v24.19.0");
    fs::create_dir_all(node_release.join("bin")).unwrap();
    fs::create_dir_all(node_release.join("lib/node_modules/npm/bin")).unwrap();
    make_executable(&node_release.join("bin/node"));
    fs::write(
        node_release.join("lib/node_modules/npm/bin/npm-cli.js"),
        b"fixture\n",
    )
    .unwrap();
    symlink(
        "../lib/node_modules/npm/bin/npm-cli.js",
        node_release.join("bin/npm"),
    )
    .unwrap();
    symlink("releases/v24.19.0", tenant.home_dir.join(".node/current")).unwrap();

    let codex_release = tenant
        .home_dir
        .join(".codex/packages/standalone/releases/0.149.0-x86_64-unknown-linux-musl");
    fs::create_dir_all(codex_release.join("bin")).unwrap();
    make_executable(&codex_release.join("bin/codex"));
    fs::create_dir_all(tenant.home_dir.join(".local/bin")).unwrap();
    symlink(
        "/home/aibox/.codex/packages/standalone/releases/0.149.0-x86_64-unknown-linux-musl",
        tenant.home_dir.join(".codex/packages/standalone/current"),
    )
    .unwrap();
    symlink(
        "/home/aibox/.codex/packages/standalone/current/bin/codex",
        tenant.home_dir.join(".local/bin/codex"),
    )
    .unwrap();

    let claude_versions = tenant.home_dir.join(".local/share/claude/versions");
    fs::create_dir_all(&claude_versions).unwrap();
    make_executable(&claude_versions.join("2.1.238"));
    symlink(
        "/home/aibox/.local/share/claude/versions/2.1.238",
        tenant.home_dir.join(".local/bin/claude"),
    )
    .unwrap();

    install_python_fixture(&tenant.home_dir, "3.13.7", "0.8.12");

    for (kind, expected) in [
        (ComponentKind::Node, "24.19.0"),
        (ComponentKind::Codex, "0.149.0"),
        (ComponentKind::Claude, "2.1.238"),
        (ComponentKind::Python, "3.13.7"),
    ] {
        assert_eq!(
            inspect(kind, &tenant.home_dir).unwrap(),
            ComponentStatus::Installed {
                version: Some(expected.to_string())
            }
        );
    }
}

#[cfg(unix)]
#[test]
fn python_inspection_detects_partial_foreign_and_wrong_platform_state() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    install_python_fixture(&tenant.home_dir, "3.13.7", "0.8.12");

    let python_launcher = tenant.home_dir.join(".local/bin/python");
    fs::remove_file(&python_launcher).unwrap();
    symlink("/home/aibox/.python/current/bin/python", &python_launcher).unwrap();
    assert_eq!(
        inspect(ComponentKind::Python, &tenant.home_dir).unwrap(),
        ComponentStatus::Incomplete
    );
    fs::remove_file(&python_launcher).unwrap();
    make_executable(&python_launcher);
    fs::write(&python_launcher, python_launcher_wrapper("python").unwrap()).unwrap();

    fs::remove_file(tenant.home_dir.join(".local/bin/pip")).unwrap();
    assert_eq!(
        inspect(ComponentKind::Python, &tenant.home_dir).unwrap(),
        ComponentStatus::Incomplete
    );
    symlink(
        "/home/aibox/.python/current/bin/pip",
        tenant.home_dir.join(".local/bin/pip"),
    )
    .unwrap();

    fs::remove_file(tenant.home_dir.join(".local/bin/uv")).unwrap();
    symlink("/tmp/foreign-uv", tenant.home_dir.join(".local/bin/uv")).unwrap();
    assert_eq!(
        inspect(ComponentKind::Python, &tenant.home_dir).unwrap(),
        ComponentStatus::Unmanaged
    );
    fs::remove_file(tenant.home_dir.join(".local/bin/uv")).unwrap();
    symlink(
        "/home/aibox/.python/current/bin/uv",
        tenant.home_dir.join(".local/bin/uv"),
    )
    .unwrap();

    let current = tenant.home_dir.join(".python/current");
    let active = fs::read_link(&current).unwrap();
    let active_name = active.file_name().unwrap().to_string_lossy();
    let wrong_platform = if expected_python_platform() == Some("x86_64-unknown-linux-gnu") {
        "aarch64-unknown-linux-gnu"
    } else {
        "x86_64-unknown-linux-gnu"
    };
    let wrong_name = active_name.replace(expected_python_platform().unwrap(), wrong_platform);
    fs::remove_file(&current).unwrap();
    symlink(format!("generations/{wrong_name}"), &current).unwrap();
    assert_eq!(
        inspect(ComponentKind::Python, &tenant.home_dir).unwrap(),
        ComponentStatus::Unmanaged
    );
}

#[cfg(unix)]
#[test]
fn python_inspection_keeps_historical_minor_launchers_safe_and_usable() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    install_python_fixture(&tenant.home_dir, "3.13.7", "0.8.12");

    let platform = expected_python_platform().unwrap();
    let architecture = if platform.starts_with("x86_64") {
        "x86_64"
    } else {
        "aarch64"
    };
    let historical = tenant.home_dir.join(format!(
        ".python/cpython/releases/cpython-3.12.11-linux-{architecture}-gnu/bin/python3.12"
    ));
    fs::create_dir_all(historical.parent().unwrap()).unwrap();
    make_executable(&historical);
    symlink(&historical, tenant.home_dir.join(".python/bin/python3.12")).unwrap();

    assert_eq!(
        inspect(ComponentKind::Python, &tenant.home_dir).unwrap(),
        ComponentStatus::Installed {
            version: Some("3.13.7".to_string())
        }
    );
    assert_eq!(
        fs::canonicalize(tenant.home_dir.join(".python/bin/python3.12")).unwrap(),
        fs::canonicalize(historical).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn exact_active_python_install_is_idempotent_without_contacting_docker() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    install_python_fixture(&tenant.home_dir, "3.13.7", "0.8.12");
    let docker = crate::docker::DockerCli::isolated(
        root.path().join("missing-docker"),
        Vec::<(std::ffi::OsString, std::ffi::OsString)>::new(),
    );

    install_runtime_component_with(
        &tenant,
        &ComponentSpec {
            kind: ComponentKind::Python,
            version: Some("3.13.7".to_string()),
        },
        &docker,
    )
    .unwrap();
}

#[cfg(unix)]
#[test]
fn python_remove_preserves_uv_pip_and_workspace_user_state() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    install_python_fixture(&tenant.home_dir, "3.13.7", "0.8.12");

    for path in [
        ".config/uv/settings.toml",
        ".cache/uv/archive/keep",
        ".local/share/uv/tools/keep",
        ".local/lib/python3.13/site-packages/keep",
        "workspace/.venv/keep",
    ] {
        let path = tenant.home_dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"keep\n").unwrap();
    }

    remove_component(&Tenant::Managed(tenant.clone()), ComponentKind::Python).unwrap();

    assert!(!tenant.home_dir.join(".python").exists());
    for launcher in [
        "uv",
        "uvx",
        "python",
        "python3",
        "python3.13",
        "pip",
        "pip3",
    ] {
        assert!(fs::symlink_metadata(tenant.home_dir.join(".local/bin").join(launcher)).is_err());
    }
    for path in [
        ".config/uv/settings.toml",
        ".cache/uv/archive/keep",
        ".local/share/uv/tools/keep",
        ".local/lib/python3.13/site-packages/keep",
        "workspace/.venv/keep",
    ] {
        assert_eq!(fs::read(tenant.home_dir.join(path)).unwrap(), b"keep\n");
    }
}

#[cfg(unix)]
#[test]
fn unmanaged_python_launcher_cannot_be_installed_or_removed() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    fs::create_dir_all(tenant.home_dir.join(".local/bin")).unwrap();
    let launcher = tenant.home_dir.join(".local/bin/python");
    symlink("/opt/foreign/python", &launcher).unwrap();
    let selected = Tenant::Managed(tenant.clone());

    let docker = crate::docker::DockerCli::isolated(
        root.path().join("missing-docker"),
        Vec::<(std::ffi::OsString, std::ffi::OsString)>::new(),
    );
    let install_error = install_runtime_component_with(
        &tenant,
        &ComponentSpec {
            kind: ComponentKind::Python,
            version: None,
        },
        &docker,
    )
    .unwrap_err()
    .to_string();
    assert!(
        install_error.contains("unmanaged Component state"),
        "{install_error}"
    );

    let remove_error = remove_component(&selected, ComponentKind::Python)
        .unwrap_err()
        .to_string();
    assert!(
        remove_error.contains("refusing to remove foreign"),
        "{remove_error}"
    );
    assert_eq!(
        fs::read_link(&launcher).unwrap(),
        std::path::Path::new("/opt/foreign/python")
    );
}

#[cfg(unix)]
#[test]
fn runtime_inspection_distinguishes_incomplete_and_unmanaged_launchers() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    fs::create_dir_all(tenant.home_dir.join(".local/bin")).unwrap();
    fs::create_dir_all(tenant.home_dir.join(".local/share/claude/versions")).unwrap();

    assert_eq!(
        inspect(ComponentKind::Claude, &tenant.home_dir).unwrap(),
        ComponentStatus::Incomplete
    );
    symlink("/tmp/not-owned", tenant.home_dir.join(".local/bin/claude")).unwrap();
    assert_eq!(
        inspect(ComponentKind::Claude, &tenant.home_dir).unwrap(),
        ComponentStatus::Unmanaged
    );
}

#[cfg(unix)]
#[test]
fn removing_agent_components_preserves_native_config_and_user_data() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let selected = Tenant::Managed(tenant.clone());
    fs::write(tenant.home_dir.join(".codex/config.toml"), b"keep\n").unwrap();
    fs::write(tenant.home_dir.join(".claude/settings.json"), b"{}\n").unwrap();
    fs::create_dir_all(tenant.home_dir.join(".local/bin")).unwrap();

    let codex_release = tenant
        .home_dir
        .join(".codex/packages/standalone/releases/1.2.3-x86_64-unknown-linux-musl");
    fs::create_dir_all(codex_release.join("bin")).unwrap();
    make_executable(&codex_release.join("bin/codex"));
    symlink(
        "/home/aibox/.codex/packages/standalone/releases/1.2.3-x86_64-unknown-linux-musl",
        tenant.home_dir.join(".codex/packages/standalone/current"),
    )
    .unwrap();
    symlink(
        "/home/aibox/.codex/packages/standalone/current/bin/codex",
        tenant.home_dir.join(".local/bin/codex"),
    )
    .unwrap();

    let claude_versions = tenant.home_dir.join(".local/share/claude/versions");
    fs::create_dir_all(&claude_versions).unwrap();
    make_executable(&claude_versions.join("4.5.6"));
    symlink(
        "/home/aibox/.local/share/claude/versions/4.5.6",
        tenant.home_dir.join(".local/bin/claude"),
    )
    .unwrap();

    remove_component(&selected, ComponentKind::Codex).unwrap();
    remove_component(&selected, ComponentKind::Claude).unwrap();

    assert_eq!(
        fs::read(tenant.home_dir.join(".codex/config.toml")).unwrap(),
        b"keep\n"
    );
    assert_eq!(
        fs::read(tenant.home_dir.join(".claude/settings.json")).unwrap(),
        b"{}\n"
    );
    assert!(!tenant.home_dir.join(".local/bin/codex").exists());
    assert!(!tenant.home_dir.join(".local/bin/claude").exists());
}

#[test]
fn restoring_user_shell_profiles_reverses_installer_edits_and_creations() {
    let home = tempfile::tempdir().unwrap();
    let profile = home.path().join(".bash_profile");
    fs::write(&profile, b"export KEEP=yes\n").unwrap();
    let snapshots = capture_user_shell_profiles(home.path()).unwrap();

    fs::write(&profile, b"changed\n").unwrap();
    fs::write(home.path().join(".bashrc"), b"created\n").unwrap();
    restore_user_shell_profiles(&snapshots).unwrap();

    assert_eq!(fs::read(profile).unwrap(), b"export KEEP=yes\n");
    assert!(!home.path().join(".bashrc").exists());
}

#[cfg(unix)]
#[test]
fn restoring_user_shell_profiles_removes_installer_symlinks_without_touching_targets() {
    use std::os::unix::fs::symlink;

    let home = tempfile::tempdir().unwrap();
    let profile = home.path().join(".bash_profile");
    let target = home.path().join("installer-target");
    fs::write(&profile, b"export KEEP=yes\n").unwrap();
    fs::write(&target, b"outside\n").unwrap();
    let snapshots = capture_user_shell_profiles(home.path()).unwrap();

    fs::remove_file(&profile).unwrap();
    symlink(&target, &profile).unwrap();
    symlink(&target, home.path().join(".bashrc")).unwrap();
    restore_user_shell_profiles(&snapshots).unwrap();

    assert_eq!(fs::read(profile).unwrap(), b"export KEEP=yes\n");
    assert!(fs::symlink_metadata(home.path().join(".bashrc")).is_err());
    assert_eq!(fs::read(target).unwrap(), b"outside\n");
}

#[test]
fn node_installer_contains_architecture_and_checksum_guards() {
    assert!(NODE_INSTALLER.contains("x86_64 | amd64"));
    assert!(NODE_INSTALLER.contains("aarch64 | arm64"));
    assert!(NODE_INSTALLER.contains("SHASUMS256.txt"));
    assert!(NODE_INSTALLER.contains("sha256sum"));
    assert!(NODE_INSTALLER.contains("mv -Tf"));
}

#[test]
fn embedded_component_installers_are_valid_bash() {
    for (name, installer) in [
        ("node", NODE_INSTALLER),
        ("codex", CODEX_INSTALLER),
        ("claude", CLAUDE_INSTALLER),
        ("python", PYTHON_INSTALLER),
        ("rust", RUST_INSTALLER),
        ("go", GO_INSTALLER),
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(format!("install-{name}.sh"));
        fs::write(&path, installer).unwrap();
        let output = std::process::Command::new("bash")
            .arg("-n")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name} installer syntax failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn nested_component_installers_keep_command_tracing_enabled() {
    assert!(CODEX_INSTALLER.contains("sh -x \"$installer\""));
    assert!(CLAUDE_INSTALLER.contains("bash -x \"$installer\""));
    assert!(PYTHON_INSTALLER.contains("sh -x \"$uv_installer\""));
    assert!(RUST_INSTALLER.contains("sh -x \"$bootstrap\""));
}

#[test]
fn python_installer_is_self_bootstrapping_transactional_and_managed_only() {
    assert!(PYTHON_INSTALLER.contains("UV_UNMANAGED_INSTALL"));
    assert!(PYTHON_INSTALLER.contains("UV_NO_MODIFY_PATH=1"));
    assert!(PYTHON_INSTALLER.contains("UV_PYTHON_DOWNLOADS=manual"));
    assert!(PYTHON_INSTALLER.contains("cpython@3"));
    assert!(PYTHON_INSTALLER.contains("-m venv \"$generation_stage\""));
    assert!(PYTHON_INSTALLER.contains("-m venv"));
    assert!(
        PYTHON_INSTALLER.contains("import bz2, ctypes, lzma, multiprocessing, sqlite3, ssl, venv")
    );
    assert!(PYTHON_INSTALLER.contains("mv -Tf"));
    assert!(!PYTHON_INSTALLER.contains("#!/usr/bin/env python"));
}

#[test]
fn rust_and_go_installers_do_not_require_python() {
    assert!(!RUST_INSTALLER.contains("python3"));
    assert!(!RUST_INSTALLER.contains("tomllib"));
    assert!(RUST_INSTALLER.contains("rustup\" run stable rustc --version"));
    assert!(!GO_INSTALLER.contains("python3"));
    assert!(GO_INSTALLER.contains("jq -er"));
}

#[cfg(unix)]
#[test]
fn rust_removal_accepts_owned_symlink_and_hardlink_proxies_and_preserves_cargo_state() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let rustup_home = tenant.home_dir.join(".rustup");
    let toolchain = rustup_home.join("toolchains/1.82.0-x86_64-unknown-linux-gnu");
    fs::create_dir_all(toolchain.join("bin")).unwrap();
    fs::write(
        rustup_home.join("settings.toml"),
        b"default_toolchain = \"1.82.0-x86_64-unknown-linux-gnu\"\n",
    )
    .unwrap();
    make_executable(&toolchain.join("bin/rustc"));

    let cargo_home = tenant.home_dir.join(".cargo");
    let bin = cargo_home.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let rustup = bin.join("rustup");
    make_executable(&rustup);
    symlink("rustup", bin.join("cargo")).unwrap();
    symlink("rustup", bin.join("cargo-miri")).unwrap();
    fs::hard_link(&rustup, bin.join("rustc")).unwrap();
    fs::hard_link(&rustup, bin.join("zz-rustup-proxy")).unwrap();
    fs::create_dir_all(cargo_home.join("registry/cache")).unwrap();
    fs::write(cargo_home.join("registry/cache/user-state"), b"keep\n").unwrap();
    make_executable(&bin.join("user-tool"));

    let selected = Tenant::Managed(tenant.clone());
    assert_eq!(
        inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
        ComponentStatus::Installed {
            version: Some("1.82.0".to_string())
        }
    );
    assert_eq!(
        rustup_proxy_paths(&bin)
            .unwrap()
            .last()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str()),
        Some("rustup")
    );
    remove_component(&selected, ComponentKind::Rust).unwrap();

    assert!(!rustup_home.exists());
    for proxy in ["rustup", "cargo", "cargo-miri", "rustc", "zz-rustup-proxy"] {
        assert!(fs::symlink_metadata(bin.join(proxy)).is_err(), "{proxy}");
    }
    assert_eq!(
        fs::read(cargo_home.join("registry/cache/user-state")).unwrap(),
        b"keep\n"
    );
    assert_eq!(fs::read(bin.join("user-tool")).unwrap(), b"fixture\n");
}

#[cfg(unix)]
#[test]
fn rust_removal_preserves_a_foreign_binary_at_a_proxy_name() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    let rustup_home = tenant.home_dir.join(".rustup");
    let toolchain = rustup_home.join("toolchains/1.82.0-x86_64-unknown-linux-gnu");
    fs::create_dir_all(toolchain.join("bin")).unwrap();
    fs::write(
        rustup_home.join("settings.toml"),
        b"default_toolchain = \"1.82.0-x86_64-unknown-linux-gnu\"\n",
    )
    .unwrap();
    make_executable(&toolchain.join("bin/rustc"));
    let bin = tenant.home_dir.join(".cargo/bin");
    fs::create_dir_all(&bin).unwrap();
    make_executable(&bin.join("rustup"));
    fs::write(bin.join("cargo"), b"foreign cargo\n").unwrap();

    remove_component(&Tenant::Managed(tenant), ComponentKind::Rust).unwrap();

    assert_eq!(fs::read(bin.join("cargo")).unwrap(), b"foreign cargo\n");
}

#[cfg(unix)]
#[test]
fn runtime_install_uses_the_shared_image_converges_state_and_restores_profiles() {
    let _run_lock = crate::docker::run_registry_test_lock();
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    fs::write(
        tenant.home_dir.join(".bash_profile"),
        b"export USER_OWNED=yes\n",
    )
    .unwrap();

    let docker_dir = tempfile::tempdir().unwrap();
    let log = docker_dir.path().join("docker.log");
    crate::testutil::write_stub_script(
        docker_dir.path(),
        "docker",
        r#"#!/bin/sh
if [ "$1" = image ] && [ "$2" = inspect ]; then
    printf 'sha256:fixture'
    exit 0
fi
if [ "$1" = container ] && [ "$2" = ls ]; then
    exit 0
fi
if [ "$1" != run ]; then
    exit 99
fi
printf '%s' "$*" >> "$AIBOX_TEST_LOG"
cid=
while [ "$#" -gt 0 ]; do
    if [ "$1" = --cidfile ]; then
        cid="$2"
        shift 2
    else
        shift
    fi
done
printf 'fixture-container' > "$cid"
printf 'installer edit' > "$AIBOX_TEST_HOME/.bash_profile"
printf 'installer creation' > "$AIBOX_TEST_HOME/.bashrc"
release="$AIBOX_TEST_HOME/.node/releases/v24.19.0"
mkdir -p "$release/bin"
printf 'node' > "$release/bin/node"
printf 'npm' > "$release/bin/npm"
chmod 755 "$release/bin/node" "$release/bin/npm"
ln -s releases/v24.19.0 "$AIBOX_TEST_HOME/.node/.current"
mv -f "$AIBOX_TEST_HOME/.node/.current" "$AIBOX_TEST_HOME/.node/current"
"#,
    );
    let docker = crate::docker::DockerCli::isolated(
        docker_dir.path().join("docker"),
        [
            (
                std::ffi::OsString::from("PATH"),
                std::ffi::OsString::from("/usr/bin:/bin"),
            ),
            (
                std::ffi::OsString::from("AIBOX_TEST_LOG"),
                log.clone().into_os_string(),
            ),
            (
                std::ffi::OsString::from("AIBOX_TEST_HOME"),
                tenant.home_dir.clone().into_os_string(),
            ),
        ],
    );
    let component = ComponentSpec {
        kind: ComponentKind::Node,
        version: Some("24.19.0".to_string()),
    };

    install_runtime_component_with(&tenant, &component, &docker).unwrap();
    let first_log = fs::read_to_string(&log).unwrap();
    assert!(
        first_log.contains(" bash -ceux "),
        "Component installers must expose Bash xtrace commands: {first_log}"
    );
    install_runtime_component_with(&tenant, &component, &docker).unwrap();

    assert_eq!(fs::read_to_string(&log).unwrap(), first_log);
    assert_eq!(
        fs::read(tenant.home_dir.join(".bash_profile")).unwrap(),
        b"export USER_OWNED=yes\n"
    );
    assert!(!tenant.home_dir.join(".bashrc").exists());
    assert_eq!(
        inspect(ComponentKind::Node, &tenant.home_dir).unwrap(),
        ComponentStatus::Installed {
            version: Some("24.19.0".to_string())
        }
    );
}

#[test]
fn codex_statusline_preserves_unrelated_configuration() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    let selected = Tenant::Managed(tenant);
    let agent = selected.for_agent(AgentKind::Codex);
    agent.ensure_agent_state_dir().unwrap();
    fs::write(agent.state_file("config.toml"), b"model = \"custom\"\n").unwrap();
    install_component(
        &selected,
        &ComponentSpec {
            kind: ComponentKind::CodexStatusline,
            version: None,
        },
    )
    .unwrap();
    let content = fs::read_to_string(agent.state_file("config.toml")).unwrap();
    assert!(content.contains("model = \"custom\""));
    assert!(content.contains("status_line"));
}
