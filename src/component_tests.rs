use super::*;
use std::ffi::OsString;
use std::fs;
use std::process::{Command, Stdio};

#[cfg(unix)]
fn isolated_docker(
    bin: &Path,
    env: impl IntoIterator<Item = (&'static str, OsString)>,
) -> crate::docker::DockerCli {
    let mut isolated_env = vec![
        (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
        (OsString::from("LC_ALL"), OsString::from("C")),
    ];
    isolated_env.extend(
        env.into_iter()
            .map(|(name, value)| (OsString::from(name), value)),
    );
    crate::docker::DockerCli::isolated(bin.join("docker"), isolated_env)
}

fn initialized_tenant() -> (tempfile::TempDir, ManagedTenant) {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    tenant.ensure_initialized().unwrap();
    (root, tenant)
}

fn managed_scope(tenant: &ManagedTenant) -> Tenant {
    Tenant::Managed(tenant.clone())
}

fn host_scope(root: &Path, home: &Path) -> Tenant {
    Tenant::Host {
        home_dir: home.to_path_buf(),
        root_dir: root.to_path_buf(),
    }
}

fn remove_confirmed(tenant: &ManagedTenant, kind: ComponentKind) -> Result<i32> {
    let selected = managed_scope(tenant);
    remove_from_tenant(
        &selected,
        kind,
        RemovalOptions {
            skip_confirmation: true,
        },
    )
}

fn write_rust_state(home: &Path, toolchain: &str, complete: bool) {
    let rustup = home.join(".rustup");
    fs::create_dir_all(&rustup).unwrap();
    fs::write(
        rustup.join("settings.toml"),
        format!("version = \"12\"\ndefault_toolchain = \"{toolchain}\"\n"),
    )
    .unwrap();
    if complete {
        fs::create_dir_all(home.join(".cargo/bin")).unwrap();
        fs::write(home.join(".cargo/bin/rustup"), "rustup").unwrap();
        let rustc = rustup.join("toolchains").join(toolchain).join("bin");
        fs::create_dir_all(&rustc).unwrap();
        fs::write(rustc.join("rustc"), "rustc").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                home.join(".cargo/bin/rustup"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            fs::set_permissions(rustc.join("rustc"), fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}

fn write_go_state(home: &Path, version: &str, complete: bool) {
    let goroot = home.join(".goroot");
    fs::create_dir_all(&goroot).unwrap();
    fs::write(
        goroot.join("VERSION"),
        format!("{version}\ntime 2026-01-01T00:00:00Z\n"),
    )
    .unwrap();
    if complete {
        fs::create_dir_all(goroot.join("bin")).unwrap();
        fs::write(goroot.join("bin/go"), "go").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(goroot.join("bin/go"), fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
}

#[test]
fn component_specs_accept_supported_shapes_and_explain_rejections() {
    for (input, kind, version) in [
        ("claude-statusline", ComponentKind::ClaudeStatusline, None),
        ("rust", ComponentKind::Rust, None),
        ("go@1.25.6", ComponentKind::Go, Some("1.25.6")),
    ] {
        assert_eq!(
            input.parse::<ComponentSpec>().unwrap(),
            ComponentSpec {
                kind,
                version: version.map(str::to_string),
            },
            "{input}"
        );
    }

    for (input, expected) in [
        ("statusline", "unknown Component"),
        ("rust@stable", "expected X.Y.Z"),
        ("rust@1.90", "expected X.Y.Z"),
        ("rust@01.90.0", "expected X.Y.Z"),
        ("go@1.25.6@extra", "expected X.Y.Z"),
        ("codex-statusline@1.0.0", "does not accept a version"),
    ] {
        let error = input.parse::<ComponentSpec>().unwrap_err();
        assert!(error.contains(expected), "{input:?}: {error}");
    }
}

#[test]
fn status_format_is_stable_and_versioned_only_for_toolchains() {
    assert_eq!(
        format_status(
            ComponentKind::ClaudeStatusline,
            &ComponentStatus::Installed { version: None }
        ),
        "claude-statusline installed"
    );
    assert_eq!(
        format_status(
            ComponentKind::Rust,
            &ComponentStatus::Installed {
                version: Some("1.90.0".to_string())
            }
        ),
        "rust installed 1.90.0"
    );
    assert_eq!(
        format_status(ComponentKind::Go, &ComponentStatus::Unmanaged),
        "go unmanaged"
    );
}

#[cfg(unix)]
#[test]
fn claude_statusline_install_overwrites_owned_state_and_preserves_other_settings() {
    use std::os::unix::fs::PermissionsExt;

    let (_root, tenant) = initialized_tenant();
    let claude = tenant.home_dir.join(".claude");
    fs::write(claude.join("statusline.sh"), "#!/bin/sh\necho custom\n").unwrap();
    fs::write(
        claude.join("settings.json"),
        r#"{"keep":true,"statusLine":{"type":"command","command":"custom"}}"#,
    )
    .unwrap();
    assert_eq!(
        inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Modified
    );

    install_claude_statusline(&managed_scope(&tenant)).unwrap();

    assert_eq!(
        inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Installed { version: None }
    );
    assert_eq!(
        fs::read(claude.join("statusline.sh")).unwrap(),
        CLAUDE_STATUSLINE
    );
    assert_eq!(
        fs::metadata(claude.join("statusline.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    let settings: Value =
        serde_json::from_slice(&fs::read(claude.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["keep"], true);
    assert_eq!(
        settings["statusLine"],
        json!({
            "type": "command",
            "command": "bash ~/.claude/statusline.sh"
        })
    );

    let script_before = fs::read(claude.join("statusline.sh")).unwrap();
    let settings_before = fs::read(claude.join("settings.json")).unwrap();
    install_claude_statusline(&managed_scope(&tenant)).unwrap();
    assert_eq!(
        fs::read(claude.join("statusline.sh")).unwrap(),
        script_before
    );
    assert_eq!(
        fs::read(claude.join("settings.json")).unwrap(),
        settings_before
    );
}

#[cfg(unix)]
#[test]
fn statusline_install_creates_missing_current_configs_private() {
    use std::os::unix::fs::PermissionsExt;

    let (_root, tenant) = initialized_tenant();
    install_claude_statusline(&managed_scope(&tenant)).unwrap();
    install_codex_statusline(&managed_scope(&tenant)).unwrap();

    for path in [
        tenant.home_dir.join(".claude/settings.json"),
        tenant.home_dir.join(".codex/config.toml"),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[test]
fn host_statusline_install_initializes_agent_state_without_changing_home_mode() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o751)).unwrap();
    let selected = host_scope(root.path(), home.path());

    install_claude_statusline(&selected).unwrap();
    install_codex_statusline(&selected).unwrap();

    assert_eq!(
        fs::metadata(home.path()).unwrap().permissions().mode() & 0o777,
        0o751
    );
    for path in [home.path().join(".claude"), home.path().join(".codex")] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    for path in [
        home.path().join(".claude/settings.json"),
        home.path().join(".codex/config.toml"),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert_eq!(
        fs::metadata(home.path().join(".claude/statusline.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        inspect(ComponentKind::ClaudeStatusline, home.path()).unwrap(),
        ComponentStatus::Installed { version: None }
    );
    assert_eq!(
        inspect(ComponentKind::CodexStatusline, home.path()).unwrap(),
        ComponentStatus::Installed { version: None }
    );
}

#[test]
fn host_statusline_remove_is_idempotent_and_toolchains_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let selected = host_scope(root.path(), home.path());

    assert_eq!(
        remove(&selected, ComponentKind::CodexStatusline, true).unwrap(),
        0
    );
    let error = install(&selected, &"rust@1.90.0".parse().unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("unavailable to the Host Tenant"), "{error}");
    let error = remove(&selected, ComponentKind::Rust, true)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unavailable to the Host Tenant"), "{error}");
    assert!(!home.path().join(".codex").exists());
    assert!(!home.path().join(".rustup").exists());

    install_claude_statusline(&selected).unwrap();
    remove(&selected, ComponentKind::ClaudeStatusline, true).unwrap();
    assert!(!home.path().join(".claude/statusline.sh").exists());
    let settings: Value =
        serde_json::from_slice(&fs::read(home.path().join(".claude/settings.json")).unwrap())
            .unwrap();
    assert!(settings.get("statusLine").is_none());
}

#[test]
fn host_component_list_stays_read_only_when_home_is_missing() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("missing-home");
    let selected = host_scope(root.path(), &home);

    assert!(!tenant_home_exists(&selected).unwrap());
    assert_eq!(component_catalog(&selected), &ComponentKind::STATUSLINES);
    list(&selected).unwrap();
    assert!(!home.exists());
    let error = install_claude_statusline(&selected)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Host Home does not exist"), "{error}");
    let error = remove(&selected, ComponentKind::ClaudeStatusline, true)
        .unwrap_err()
        .to_string();
    assert!(error.contains("Host Home does not exist"), "{error}");
}

#[test]
fn codex_statusline_requires_an_explicit_false_color_setting() {
    let (_root, tenant) = initialized_tenant();
    let config = tenant.home_dir.join(".codex/config.toml");
    let items = CODEX_STATUSLINE_ITEMS
        .iter()
        .map(|item| format!("\"{item}\""))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(&config, format!("[tui]\nstatus_line = [{items}]\n")).unwrap();
    assert_eq!(
        inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Incomplete
    );

    fs::write(
        &config,
        format!("[tui]\nstatus_line = [{items}]\nstatus_line_use_colors = true\n"),
    )
    .unwrap();
    assert_eq!(
        inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Modified
    );

    fs::write(
        &config,
        format!("[tui]\nstatus_line = [{items}]\nstatus_line_use_colors = false\n"),
    )
    .unwrap();
    assert_eq!(
        inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Installed { version: None }
    );
}

#[test]
fn codex_statusline_install_preserves_unrelated_toml_and_comments() {
    let (_root, tenant) = initialized_tenant();
    let config = tenant.home_dir.join(".codex/config.toml");
    fs::write(
            &config,
            "# keep this comment\nmodel = \"custom\"\n\n[tui]\nanimations = false\nstatus_line = [\"old\"]\nstatus_line_use_colors = true\n",
        )
        .unwrap();

    assert_eq!(
        inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Modified
    );

    install_codex_statusline(&managed_scope(&tenant)).unwrap();

    let content = fs::read_to_string(&config).unwrap();
    assert!(content.contains("# keep this comment"), "{content}");
    let document = content.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(document["model"].as_str(), Some("custom"));
    assert_eq!(document["tui"]["animations"].as_bool(), Some(false));
    let status_line: Vec<_> = document["tui"]["status_line"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap())
        .collect();
    assert_eq!(
        status_line,
        vec![
            "model-with-reasoning",
            "current-dir",
            "git-branch",
            "context-window-size",
            "context-used",
        ]
    );
    assert_eq!(
        document["tui"]["status_line_use_colors"].as_bool(),
        Some(false)
    );
    assert_eq!(
        inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Installed { version: None }
    );
}

#[test]
fn claude_statusline_renders_unified_fields() {
    assert!(!CLAUDE_STATUSLINE.contains(&0x1b));
    assert!(!String::from_utf8_lossy(CLAUDE_STATUSLINE).contains("\\033"));
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let workspace = home.join("easymath3/workspace");
    fs::create_dir_all(&workspace).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&workspace)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args(["symbolic-ref", "HEAD", "refs/heads/main"])
            .current_dir(&workspace)
            .status()
            .unwrap()
            .success()
    );

    let script = root.path().join("statusline.sh");
    fs::write(&script, CLAUDE_STATUSLINE).unwrap();
    let input = serde_json::json!({
        "model": {"display_name": "gpt-5.6-sol"},
        "effort": {"level": "xhigh"},
        "workspace": {"current_dir": workspace},
        "context_window": {
            "context_window_size": 258000,
            "used_percentage": 54.8
        }
    });
    let mut child = Command::new("bash")
        .arg(&script)
        .env("HOME", &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "gpt-5.6-sol xhigh · ~/easymath3/workspace · main · 258K window · Context 54% used\n"
    );
}

#[test]
fn claude_statusline_omits_missing_fields_and_branch() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let script = root.path().join("statusline.sh");
    fs::write(&script, CLAUDE_STATUSLINE).unwrap();
    let input = serde_json::json!({
        "workspace": {"current_dir": workspace},
        "context_window": {}
    });
    let mut child = Command::new("bash")
        .arg(&script)
        .env("HOME", root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "~/workspace\n");
}

#[test]
fn claude_statusline_clamps_context_percentage() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap();
    let script = root.path().join("statusline.sh");
    fs::write(&script, CLAUDE_STATUSLINE).unwrap();

    for (percentage, expected) in [
        (-2.0, "~/workspace · Context 0% used\n"),
        (125.0, "~/workspace · Context 100% used\n"),
    ] {
        let input = serde_json::json!({
            "workspace": {"current_dir": workspace},
            "context_window": {"used_percentage": percentage}
        });
        let mut child = Command::new("bash")
            .arg(&script)
            .env("HOME", root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }

    let input = serde_json::json!({
        "workspace": {"current_dir": workspace},
        "context_window": {"used_percentage": "unknown"}
    });
    let mut child = Command::new("bash")
        .arg(&script)
        .env("HOME", root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "~/workspace\n");
}

#[test]
fn codex_statusline_install_rejects_an_unowned_non_table_tui() {
    let (_root, tenant) = initialized_tenant();
    let config = tenant.home_dir.join(".codex/config.toml");
    let original = "model = \"custom\"\ntui = \"keep\"\n";
    fs::write(&config, original).unwrap();

    assert_eq!(
        inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::NotInstalled
    );
    let error = install_codex_statusline(&managed_scope(&tenant))
        .unwrap_err()
        .to_string();
    assert!(error.contains("refusing to replace unowned"), "{error}");
    assert_eq!(fs::read_to_string(config).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn partial_statusline_installations_are_incomplete() {
    use std::os::unix::fs::PermissionsExt;

    let (_root, tenant) = initialized_tenant();
    let claude_script = tenant.home_dir.join(".claude/statusline.sh");
    fs::write(&claude_script, CLAUDE_STATUSLINE).unwrap();
    fs::set_permissions(&claude_script, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Incomplete
    );

    fs::write(
        tenant.home_dir.join(".codex/config.toml"),
        "[tui]\nstatus_line_use_colors = false\n",
    )
    .unwrap();
    assert_eq!(
        inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Incomplete
    );
}

#[test]
fn statusline_survives_repeated_config_applications() {
    let (_root, tenant) = initialized_tenant();
    let selected = tenant.for_agent(AgentKind::Codex);
    for (name, model) in [("one", "one"), ("two", "two")] {
        crate::config::create_named_config(&selected, name).unwrap();
        fs::write(
            selected.named_config_file(name, "config.toml"),
            format!("model = \"{model}\"\n"),
        )
        .unwrap();
    }

    install_codex_statusline(&managed_scope(&tenant)).unwrap();
    assert_eq!(
        inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Installed { version: None }
    );
    crate::config::apply_named_config(&selected, "one").unwrap();
    assert_eq!(
        inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Installed { version: None }
    );
    crate::config::apply_named_config(&selected, "two").unwrap();
    assert_eq!(
        inspect(ComponentKind::CodexStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Installed { version: None }
    );
    let config = fs::read_to_string(selected.state_file("config.toml")).unwrap();
    let document = config.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(document["model"].as_str(), Some("two"));
    assert!(document["tui"]["status_line"].is_array());
}

#[test]
fn statusline_install_after_config_apply_needs_no_config_coordination() {
    let (_root, tenant) = initialized_tenant();
    let selected = tenant.for_agent(AgentKind::Claude);
    crate::config::create_named_config(&selected, "custom").unwrap();
    crate::config::apply_named_config(&selected, "custom").unwrap();

    install_claude_statusline(&managed_scope(&tenant)).unwrap();
    assert_eq!(
        inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Installed { version: None }
    );
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(selected.state_file("settings.json")).unwrap()).unwrap();
    assert!(settings["env"]["ANTHROPIC_BASE_URL"].is_string());
    assert_eq!(settings["statusLine"]["type"], "command");
}

#[test]
fn component_remove_requires_confirmation_but_not_discard_and_is_idempotent() {
    let (_root, tenant) = initialized_tenant();
    let claude = tenant.home_dir.join(".claude");
    fs::write(claude.join("settings.json"), "{\"keep\":true}\n").unwrap();
    install_claude_statusline(&managed_scope(&tenant)).unwrap();

    if !io::stdin().is_terminal() {
        let selected = managed_scope(&tenant);
        let error = remove_from_tenant(
            &selected,
            ComponentKind::ClaudeStatusline,
            RemovalOptions {
                skip_confirmation: false,
            },
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("without --yes"), "{error}");
        assert_eq!(
            inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
            ComponentStatus::Installed { version: None },
            "refusing non-interactive removal must preserve the Component"
        );
    }

    remove_confirmed(&tenant, ComponentKind::ClaudeStatusline).unwrap();
    remove_confirmed(&tenant, ComponentKind::ClaudeStatusline).unwrap();
    assert!(!claude.join("statusline.sh").exists());
    let settings: Value =
        serde_json::from_slice(&fs::read(claude.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["keep"], true);
    assert!(settings.get("statusLine").is_none());

    fs::write(claude.join("statusline.sh"), "custom\n").unwrap();
    assert_eq!(
        inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap(),
        ComponentStatus::Modified
    );
    remove_confirmed(&tenant, ComponentKind::ClaudeStatusline).unwrap();
}

#[test]
fn codex_statusline_remove_modified_state_removes_only_component_owned_keys() {
    let (_root, tenant) = initialized_tenant();
    let config = tenant.home_dir.join(".codex/config.toml");
    fs::write(
        &config,
        "# keep this comment\nmodel = \"custom\"\n\n[tui]\nanimations = false\n",
    )
    .unwrap();
    install_codex_statusline(&managed_scope(&tenant)).unwrap();
    let mut document = fs::read_to_string(&config)
        .unwrap()
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
    let mut customized = toml_edit::Array::new();
    customized.push("user-customized");
    document["tui"]["status_line"] = toml_edit::value(customized);
    fs::write(&config, document.to_string()).unwrap();

    remove_confirmed(&tenant, ComponentKind::CodexStatusline).unwrap();

    let content = fs::read_to_string(&config).unwrap();
    assert!(content.contains("# keep this comment"), "{content}");
    let document = content.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(document["model"].as_str(), Some("custom"));
    let tui = document["tui"].as_table_like().unwrap();
    assert_eq!(
        tui.get("animations").and_then(toml_edit::Item::as_bool),
        Some(false)
    );
    assert!(tui.get("status_line").is_none());
    assert!(tui.get("status_line_use_colors").is_none());
}

#[test]
fn unowned_toolchain_paths_are_not_claimed_or_removed() {
    let (_root, tenant) = initialized_tenant();
    let manual_rust = tenant.home_dir.join(".cargo/bin/rustc");
    let manual_go = tenant.home_dir.join(".gopath/bin/custom-go-tool");
    fs::create_dir_all(manual_rust.parent().unwrap()).unwrap();
    fs::create_dir_all(manual_go.parent().unwrap()).unwrap();
    fs::write(&manual_rust, b"manual rust").unwrap();
    fs::write(&manual_go, b"manual go").unwrap();

    assert_eq!(
        inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
        ComponentStatus::NotInstalled
    );
    assert_eq!(
        inspect(ComponentKind::Go, &tenant.home_dir).unwrap(),
        ComponentStatus::NotInstalled
    );

    remove_confirmed(&tenant, ComponentKind::Rust).unwrap();
    remove_confirmed(&tenant, ComponentKind::Go).unwrap();
    assert_eq!(fs::read(&manual_rust).unwrap(), b"manual rust");
    assert_eq!(fs::read(&manual_go).unwrap(), b"manual go");
}

#[test]
fn toolchain_remove_preserves_user_caches_and_unrelated_commands() {
    let (_root, tenant) = initialized_tenant();
    let cargo_bin = tenant.home_dir.join(".cargo/bin");
    fs::create_dir_all(&cargo_bin).unwrap();
    fs::write(cargo_bin.join("custom-command"), "keep").unwrap();
    fs::write(cargo_bin.join("cargo"), "proxy").unwrap();
    fs::create_dir_all(tenant.home_dir.join(".rustup")).unwrap();
    fs::write(
        tenant.home_dir.join(".rustup/settings.toml"),
        "default_toolchain = \"nightly-x86_64-unknown-linux-gnu\"\n",
    )
    .unwrap();
    remove_confirmed(&tenant, ComponentKind::Rust).unwrap();
    assert!(!tenant.home_dir.join(".rustup").exists());
    assert!(!cargo_bin.join("cargo").exists());
    assert_eq!(
        fs::read_to_string(cargo_bin.join("custom-command")).unwrap(),
        "keep"
    );

    fs::create_dir_all(tenant.home_dir.join(".goroot")).unwrap();
    fs::create_dir_all(tenant.home_dir.join(".gopath")).unwrap();
    fs::write(tenant.home_dir.join(".gopath/keep"), "keep").unwrap();
    remove_confirmed(&tenant, ComponentKind::Go).unwrap();
    assert!(!tenant.home_dir.join(".goroot").exists());
    assert_eq!(
        fs::read_to_string(tenant.home_dir.join(".gopath/keep")).unwrap(),
        "keep"
    );
}

#[cfg(unix)]
#[test]
fn rust_remove_rejects_a_symlinked_cargo_ancestor_before_deleting_anything() {
    use std::os::unix::fs::symlink;

    let (_root, tenant) = initialized_tenant();
    fs::create_dir(tenant.home_dir.join(".rustup")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::create_dir(outside.path().join("bin")).unwrap();
    let outside_proxy = outside.path().join("bin/rustup");
    fs::write(&outside_proxy, "keep").unwrap();
    symlink(outside.path(), tenant.home_dir.join(".cargo")).unwrap();

    let error = remove_confirmed(&tenant, ComponentKind::Rust)
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("Cargo Home is not a real directory"),
        "{error}"
    );
    assert!(tenant.home_dir.join(".rustup").is_dir());
    assert_eq!(fs::read_to_string(outside_proxy).unwrap(), "keep");
}

#[cfg(unix)]
#[test]
fn rust_remove_prevalidates_every_proxy_before_removing_anything() {
    use std::os::unix::fs::symlink;

    let (_root, tenant) = initialized_tenant();
    let rustup_home = tenant.home_dir.join(".rustup");
    let cargo_bin = tenant.home_dir.join(".cargo/bin");
    fs::create_dir(&rustup_home).unwrap();
    fs::create_dir_all(&cargo_bin).unwrap();
    let rustup_proxy = cargo_bin.join("rustup");
    fs::write(&rustup_proxy, "keep proxy").unwrap();
    let outside = tempfile::tempdir().unwrap();
    let outside_rustc = outside.path().join("rustc");
    fs::write(&outside_rustc, "outside").unwrap();
    symlink(&outside_rustc, cargo_bin.join("rustc")).unwrap();

    let error = remove_confirmed(&tenant, ComponentKind::Rust)
        .unwrap_err()
        .to_string();

    assert!(
        error.contains("rustup proxy is not a regular file"),
        "{error}"
    );
    assert!(rustup_home.is_dir());
    assert_eq!(fs::read_to_string(rustup_proxy).unwrap(), "keep proxy");
    assert_eq!(fs::read_to_string(outside_rustc).unwrap(), "outside");
}

#[cfg(unix)]
#[test]
fn go_remove_rejects_a_symlinked_sdk_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let (_root, tenant) = initialized_tenant();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("VERSION"), b"go1.25.6\n").unwrap();
    fs::write(outside.path().join("keep"), b"outside sdk").unwrap();
    symlink(outside.path(), tenant.home_dir.join(".goroot")).unwrap();

    let error = remove_confirmed(&tenant, ComponentKind::Go)
        .unwrap_err()
        .to_string();

    assert!(error.contains("Go root is not a real directory"), "{error}");
    assert_eq!(
        fs::read(outside.path().join("keep")).unwrap(),
        b"outside sdk"
    );
}

#[test]
fn toolchain_statuses_are_derived_from_native_files() {
    let (_root, tenant) = initialized_tenant();
    assert_eq!(
        inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
        ComponentStatus::NotInstalled
    );
    assert_eq!(
        inspect(ComponentKind::Go, &tenant.home_dir).unwrap(),
        ComponentStatus::NotInstalled
    );

    let toolchain = "1.90.0-x86_64-unknown-linux-gnu";
    write_rust_state(&tenant.home_dir, toolchain, true);
    write_go_state(&tenant.home_dir, "go1.25.6", true);
    assert_eq!(
        inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
        ComponentStatus::Installed {
            version: Some("1.90.0".to_string())
        }
    );
    assert_eq!(
        inspect(ComponentKind::Go, &tenant.home_dir).unwrap(),
        ComponentStatus::Installed {
            version: Some("1.25.6".to_string())
        }
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            tenant
                .home_dir
                .join(".rustup/toolchains")
                .join(toolchain)
                .join("bin/rustc"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        fs::set_permissions(
            tenant.home_dir.join(".goroot/bin/go"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert_eq!(
            inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
            ComponentStatus::Incomplete
        );
        assert_eq!(
            inspect(ComponentKind::Go, &tenant.home_dir).unwrap(),
            ComponentStatus::Incomplete
        );
    }

    fs::write(
        tenant.home_dir.join(".rustup/settings.toml"),
        "default_toolchain = \"nightly-x86_64-unknown-linux-gnu\"\n",
    )
    .unwrap();
    fs::write(tenant.home_dir.join(".goroot/VERSION"), "go1.26rc1\n").unwrap();
    assert_eq!(
        inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
        ComponentStatus::Unmanaged
    );
    assert_eq!(
        inspect(ComponentKind::Go, &tenant.home_dir).unwrap(),
        ComponentStatus::Unmanaged
    );

    write_rust_state(&tenant.home_dir, "1.90.0-custom", true);
    assert_eq!(
        inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
        ComponentStatus::Unmanaged
    );
}

#[test]
fn incomplete_stable_toolchains_are_reported_as_incomplete() {
    let (_root, tenant) = initialized_tenant();
    write_rust_state(&tenant.home_dir, "1.90.0-x86_64-unknown-linux-gnu", false);
    write_go_state(&tenant.home_dir, "go1.25.6", false);
    assert_eq!(
        inspect(ComponentKind::Rust, &tenant.home_dir).unwrap(),
        ComponentStatus::Incomplete
    );
    assert_eq!(
        inspect(ComponentKind::Go, &tenant.home_dir).unwrap(),
        ComponentStatus::Incomplete
    );
}

#[test]
fn explicit_healthy_toolchain_version_skips_before_docker_lookup() {
    let (_root, tenant) = initialized_tenant();
    write_rust_state(&tenant.home_dir, "1.90.0-x86_64-unknown-linux-gnu", true);
    let component = "rust@1.90.0".parse::<ComponentSpec>().unwrap();

    assert_eq!(install_toolchain(&tenant, &component).unwrap(), 0);
}

#[cfg(unix)]
#[test]
fn status_inspection_rejects_symlinked_owned_paths() {
    use std::os::unix::fs::symlink;

    let (_root, tenant) = initialized_tenant();
    let outside = tempfile::NamedTempFile::new().unwrap();
    symlink(
        outside.path(),
        tenant.home_dir.join(".claude/statusline.sh"),
    )
    .unwrap();
    let error = format!(
        "{:#}",
        inspect(ComponentKind::ClaudeStatusline, &tenant.home_dir).unwrap_err()
    );
    assert!(error.contains("not a regular file"), "{error}");
}

#[cfg(unix)]
fn write_fake_docker(dir: &Path) {
    crate::testutil::write_stub_script(
        dir,
        "docker",
        r#"#!/bin/sh
if [ -n "$AIBOX_FAKE_DOCKER_LOG" ]; then
    printf '%s\n' "$*" >> "$AIBOX_FAKE_DOCKER_LOG"
fi
if [ "$1" = image ] && [ "$2" = inspect ]; then
    [ "$AIBOX_FAKE_DOCKER_MODE" = missing ] && exit 1
    printf 'sha256:fake\n'
    exit 0
fi
if [ "$1" = image ] && [ "$2" = ls ]; then
    exit 0
fi
if [ "$1" = container ] && [ "$2" = ls ]; then
    exit 0
fi
if [ "$1" = run ]; then
    shift
    while [ "$#" -gt 0 ]; do
        if [ "$1" = --cidfile ]; then
            printf 'fake-container\n' > "$2"
            exit 0
        fi
        shift
    done
fi
exit 99
"#,
    );
}

#[cfg(unix)]
fn run_installer(
    script: &str,
    home: &Path,
    bin: &Path,
    version: &str,
    env: impl IntoIterator<Item = (&'static str, OsString)>,
) -> std::process::Output {
    let path = std::env::join_paths([bin, Path::new("/usr/bin"), Path::new("/bin")]).unwrap();
    let mut command = Command::new("bash");
    command
        .arg(script)
        .arg(version)
        .env_clear()
        .env("HOME", home)
        .env("PATH", path)
        .env("LC_ALL", "C")
        .envs(env);
    command.output().unwrap()
}

#[cfg(unix)]
fn expose_host_command(bin: &Path, name: &str) {
    use std::os::unix::fs::symlink;

    let output = Command::new("sh")
        .args(["-c", &format!("command -v {name}")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "required test command {name} is missing"
    );
    let target = String::from_utf8(output.stdout).unwrap();
    symlink(target.trim(), bin.join(name)).unwrap();
}

#[cfg(unix)]
#[test]
fn rust_installer_skips_same_version_and_uninstalls_before_switching() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let bin = scratch.path().join("bin");
    let log = scratch.path().join("rustup.log");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&bin).unwrap();
    expose_host_command(&bin, "python3");
    crate::testutil::write_stub_script(
        &bin,
        "curl",
        r#"#!/bin/sh
out=
while [ "$#" -gt 0 ]; do
    if [ "$1" = -o ]; then out=$2; shift 2; else shift; fi
done
cat > "$out" <<'BOOTSTRAP'
#!/bin/sh
mkdir -p "$CARGO_HOME/bin" "$RUSTUP_HOME"
cp "$AIBOX_FAKE_RUSTUP" "$CARGO_HOME/bin/rustup"
chmod +x "$CARGO_HOME/bin/rustup"
BOOTSTRAP
"#,
    );
    crate::testutil::write_stub_script(
        &bin,
        "fake-rustup",
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$AIBOX_FAKE_RUSTUP_LOG"
case "$1 $2" in
    "toolchain list")
        old=$(sed -n 's/^default_toolchain = "\(.*\)"/\1/p' "$RUSTUP_HOME/settings.toml" 2>/dev/null)
        [ -n "$old" ] && [ -d "$RUSTUP_HOME/toolchains/$old" ] && printf '%s (default)\n' "$old"
        ;;
    "toolchain uninstall")
        rm -rf "$RUSTUP_HOME/toolchains/$3"
        ;;
    "toolchain install")
        mkdir -p "$RUSTUP_HOME/toolchains/$3-x86_64-unknown-linux-gnu/bin"
        cat > "$RUSTUP_HOME/toolchains/$3-x86_64-unknown-linux-gnu/bin/rustc" <<EOF
#!/bin/sh
printf 'rustc $3\n'
EOF
        chmod +x "$RUSTUP_HOME/toolchains/$3-x86_64-unknown-linux-gnu/bin/rustc"
        cp "$RUSTUP_HOME/toolchains/$3-x86_64-unknown-linux-gnu/bin/rustc" "$CARGO_HOME/bin/rustc"
        ;;
    "default "*)
        printf 'version = "12"\ndefault_toolchain = "%s-x86_64-unknown-linux-gnu"\n' "$2" > "$RUSTUP_HOME/settings.toml"
        ;;
esac
"#,
    );
    let installer_env = || {
        [
            ("AIBOX_FAKE_RUSTUP", bin.join("fake-rustup").into()),
            ("AIBOX_FAKE_RUSTUP_LOG", log.as_os_str().into()),
        ]
    };

    let first = run_installer(
        &format!("{}/assets/install-rust.sh", env!("CARGO_MANIFEST_DIR")),
        &home,
        &bin,
        "1.90.0",
        installer_env(),
    );
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first_log = fs::read_to_string(&log).unwrap();

    let same = run_installer(
        &format!("{}/assets/install-rust.sh", env!("CARGO_MANIFEST_DIR")),
        &home,
        &bin,
        "1.90.0",
        installer_env(),
    );
    assert!(
        same.status.success(),
        "{}",
        String::from_utf8_lossy(&same.stderr)
    );
    assert!(String::from_utf8_lossy(&same.stdout).contains("already installed"));
    let same_log = fs::read_to_string(&log).unwrap();
    assert_eq!(
        same_log,
        format!("{first_log}run 1.90.0-x86_64-unknown-linux-gnu rustc --version\n")
    );

    let switch = run_installer(
        &format!("{}/assets/install-rust.sh", env!("CARGO_MANIFEST_DIR")),
        &home,
        &bin,
        "1.89.0",
        installer_env(),
    );
    assert!(
        switch.status.success(),
        "{}",
        String::from_utf8_lossy(&switch.stderr)
    );
    let switched = fs::read_to_string(&log).unwrap();
    let uninstall = switched.find("toolchain uninstall 1.90.0-").unwrap();
    let install = switched.rfind("toolchain install 1.89.0").unwrap();
    assert!(uninstall < install, "{switched}");
    assert!(home.join(".cargo").is_dir());
}

#[cfg(unix)]
#[test]
fn go_installer_verifies_and_replaces_only_goroot() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let bin = scratch.path().join("bin");
    let fixture = scratch.path().join("fixture");
    let archive = scratch.path().join("go.tar.gz");
    let metadata = scratch.path().join("releases.json");
    fs::create_dir_all(home.join(".goroot")).unwrap();
    fs::create_dir_all(home.join(".gopath")).unwrap();
    fs::write(home.join(".goroot/old"), "old").unwrap();
    fs::write(home.join(".gopath/keep"), "keep").unwrap();
    fs::create_dir_all(fixture.join("go/bin")).unwrap();
    fs::write(fixture.join("go/VERSION"), "go1.25.6\n").unwrap();
    crate::testutil::write_stub_script(
        &fixture.join("go/bin"),
        "go",
        "#!/bin/sh\nprintf 'go version go1.25.6 linux/amd64\n'\n",
    );
    let status = Command::new("tar")
        .args(["-C", fixture.to_str().unwrap(), "-czf"])
        .arg(&archive)
        .arg("go")
        .status()
        .unwrap();
    assert!(status.success());
    let checksum = Command::new("sha256sum").arg(&archive).output().unwrap();
    let checksum = String::from_utf8(checksum.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    fs::write(
            &metadata,
            format!(
                r#"[{{"version":"go1.25.6","stable":true,"files":[{{"filename":"go1.25.6.linux-amd64.tar.gz","os":"linux","arch":"amd64","kind":"archive","sha256":"{checksum}"}}]}}]"#
            ),
        )
        .unwrap();
    fs::create_dir_all(&bin).unwrap();
    expose_host_command(&bin, "python3");
    expose_host_command(&bin, "sha256sum");
    crate::testutil::write_stub_script(&bin, "dpkg", "#!/bin/sh\nprintf 'amd64\n'\n");
    crate::testutil::write_stub_script(
        &bin,
        "curl",
        r#"#!/bin/sh
url=
out=
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) out=$2; shift 2 ;;
        http*) url=$1; shift ;;
        *) shift ;;
    esac
done
case "$url" in
    *mode=json*) cp "$AIBOX_FAKE_GO_METADATA" "$out" ;;
    *) cp "$AIBOX_FAKE_GO_ARCHIVE" "$out" ;;
esac
"#,
    );
    let output = run_installer(
        &format!("{}/assets/install-go.sh", env!("CARGO_MANIFEST_DIR")),
        &home,
        &bin,
        "1.25.6",
        [
            ("AIBOX_FAKE_GO_METADATA", metadata.as_os_str().into()),
            ("AIBOX_FAKE_GO_ARCHIVE", archive.as_os_str().into()),
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(home.join(".goroot/VERSION")).unwrap(),
        "go1.25.6\n"
    );
    assert!(!home.join(".goroot/old").exists());
    assert_eq!(
        fs::read_to_string(home.join(".gopath/keep")).unwrap(),
        "keep"
    );

    let same = run_installer(
        &format!("{}/assets/install-go.sh", env!("CARGO_MANIFEST_DIR")),
        &home,
        &bin,
        "1.25.6",
        [
            ("AIBOX_FAKE_GO_METADATA", metadata.as_os_str().into()),
            ("AIBOX_FAKE_GO_ARCHIVE", archive.as_os_str().into()),
        ],
    );
    assert!(
        same.status.success(),
        "{}",
        String::from_utf8_lossy(&same.stderr)
    );
    assert!(String::from_utf8_lossy(&same.stdout).contains("already installed"));
}

#[cfg(unix)]
#[test]
fn toolchain_install_uses_the_shared_image_and_home_only_mount() {
    let root = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let log = root.path().join("docker.log");
    write_fake_docker(bin.path());
    let docker = isolated_docker(
        bin.path(),
        [("AIBOX_FAKE_DOCKER_LOG", log.as_os_str().into())],
    );
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    let component = "rust@1.90.0".parse::<ComponentSpec>().unwrap();

    assert_eq!(
        install_toolchain_with(&tenant, &component, None, &docker).unwrap(),
        0
    );

    let log = fs::read_to_string(log).unwrap();
    assert!(log.contains("image inspect"), "{log}");
    assert!(
        log.contains(&format!(
            "{}:/home/aibox",
            root.path().join("tenants/work").display()
        )),
        "{log}"
    );
    assert!(!log.contains("/workspace"), "{log}");
    assert!(log.contains("aibox-rust-installer 1.90.0"), "{log}");
}

#[cfg(unix)]
#[test]
fn missing_image_does_not_initialize_a_toolchain_tenant() {
    let root = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    write_fake_docker(bin.path());
    let docker = isolated_docker(bin.path(), [("AIBOX_FAKE_DOCKER_MODE", "missing".into())]);
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    let component = "go@1.25.6".parse::<ComponentSpec>().unwrap();

    let error = install_toolchain_with(&tenant, &component, None, &docker)
        .unwrap_err()
        .to_string();

    assert!(error.contains("build it first"), "{error}");
    assert!(!tenant.home_dir.exists());
}
