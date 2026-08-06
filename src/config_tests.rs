use super::*;
use crate::agent::AgentKind;
use crate::tenant::{ManagedTenant, Tenant};
use crate::testutil::EnvGuard;
use serde_json::Value;
use std::path::Path;

fn selected(root: &Path, agent: AgentKind) -> TenantAgent {
    let tenant = ManagedTenant::resolve(root, "work").unwrap();
    tenant.ensure_initialized().unwrap();
    tenant.for_agent(agent)
}

fn replace_config_files(selected: &TenantAgent, config: &str, main: &str, auth: &str) {
    fs::write(
        selected.named_config_file(config, selected.agent.main_config_file()),
        main,
    )
    .unwrap();
    if let Some(file) = selected.agent.native_auth_file() {
        fs::write(selected.named_config_file(config, file), auth).unwrap();
    }
}

#[test]
fn configs_are_tenant_and_agent_local() {
    let root = tempfile::tempdir().unwrap();
    let codex = selected(root.path(), AgentKind::Codex);
    let claude = selected(root.path(), AgentKind::Claude);
    create_named_config(&codex, "custom").unwrap();
    assert_eq!(list_named_configs(&codex).unwrap(), ["custom"]);
    assert!(list_named_configs(&claude).unwrap().is_empty());
}

#[test]
fn create_uses_templates_and_rejects_a_complete_existing_config() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();

    let main = fs::read_to_string(selected.named_config_file("custom", "config.toml")).unwrap();
    assert!(main.contains("name = \"custom\""), "{main}");
    assert_eq!(
        fs::read_to_string(selected.named_config_file("custom", "auth.json")).unwrap(),
        "{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n"
    );
    assert!(!selected.state_file("config.toml").exists());
    assert!(!selected.state_file("auth.json").exists());
    let error = create_named_config(&selected, "custom")
        .unwrap_err()
        .to_string();
    assert!(error.contains("already exists"), "{error}");

    fs::write(
        selected.named_config_file("custom", "config.toml"),
        "invalid = true\n",
    )
    .unwrap();
    let error = create_named_config(&selected, "custom")
        .unwrap_err()
        .to_string();
    assert!(error.contains("already exists"), "{error}");
}

#[test]
fn claude_named_config_uses_one_native_file_with_the_token_in_env() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);

    create_named_config(&selected, "custom").unwrap();

    assert_eq!(selected.agent.config_files(), ["settings.json"]);
    assert!(!selected.named_config_file("custom", "auth.json").exists());
    let settings: Value = serde_json::from_str(
        &fs::read_to_string(selected.named_config_file("custom", "settings.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-example");
}

#[test]
fn get_prints_native_files_in_order_with_unredacted_content_and_missing_markers() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    replace_config_files(
        &selected,
        "custom",
        "model = \"custom\"\n",
        "{\"OPENAI_API_KEY\":\"secret\"}\n",
    );

    assert_eq!(
        get_named_config(&selected, "custom").unwrap(),
        b"==> config.toml <==\nmodel = \"custom\"\n\n==> auth.json <==\n{\"OPENAI_API_KEY\":\"secret\"}\n"
    );

    fs::write(selected.state_file("config.toml"), b"not valid toml\n").unwrap();
    assert_eq!(
        get_current_config(&selected).unwrap(),
        b"==> config.toml <==\nnot valid toml\n\n==> auth.json (missing) <==\n"
    );
}

#[test]
fn get_current_rejects_a_missing_managed_tenant_without_creating_it() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "missing").unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);

    let error = get_current_config(&selected).unwrap_err().to_string();

    assert!(error.contains("Tenant Home does not exist"), "{error}");
    assert!(!tenant.home_dir.exists());
}

#[cfg(unix)]
#[test]
fn create_repairs_only_safe_valid_incomplete_configs() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    selected.ensure_named_config_catalog().unwrap();
    let directory = selected.named_config_dir("partial");
    tenant::ensure_real_dir(&directory, "Named Config directory").unwrap();
    let main = selected.named_config_file("partial", "config.toml");
    fs::write(&main, "model = \"kept\"\n").unwrap();
    tenant::set_600(&main).unwrap();

    create_named_config(&selected, "partial").unwrap();

    assert_eq!(fs::read_to_string(&main).unwrap(), "model = \"kept\"\n");
    assert!(selected.named_config_file("partial", "auth.json").is_file());

    let broken = selected.named_config_dir("broken");
    tenant::ensure_real_dir(&broken, "Named Config directory").unwrap();
    let broken_main = selected.named_config_file("broken", "config.toml");
    fs::write(&broken_main, "unknown = true\n").unwrap();
    tenant::set_600(&broken_main).unwrap();
    let error = format!(
        "{:#}",
        create_named_config(&selected, "broken").unwrap_err()
    );
    assert!(error.contains("unsupported Config Field"), "{error}");
    assert!(!selected.named_config_file("broken", "auth.json").exists());

    let unknown = selected.named_config_dir("unknown");
    tenant::ensure_real_dir(&unknown, "Named Config directory").unwrap();
    fs::write(unknown.join("extra"), "unexpected").unwrap();
    let error = create_named_config(&selected, "unknown")
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown entry"), "{error}");

    use std::os::unix::fs::symlink;
    let linked = selected.named_config_dir("linked");
    tenant::ensure_real_dir(&linked, "Named Config directory").unwrap();
    symlink(&main, linked.join("config.toml")).unwrap();
    let error = create_named_config(&selected, "linked")
        .unwrap_err()
        .to_string();
    assert!(error.contains("non-regular file"), "{error}");

    assert_eq!(list_named_configs(&selected).unwrap(), ["partial"]);
}

#[test]
fn invalid_complete_configs_are_visible_readable_and_rejected_by_apply() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    create_named_config(&selected, "broken").unwrap();
    fs::write(
        selected.named_config_file("broken", "settings.json"),
        "{\"theme\":\"dark\"}\n",
    )
    .unwrap();

    assert_eq!(list_named_configs(&selected).unwrap(), ["broken"]);
    assert!(
        String::from_utf8(get_named_config(&selected, "broken").unwrap())
            .unwrap()
            .contains("theme")
    );
    let error = format!("{:#}", apply_named_config(&selected, "broken").unwrap_err());
    assert!(error.contains("/config/theme"), "{error}");
    assert!(!selected.state_file("settings.json").exists());
}

#[cfg(unix)]
#[test]
fn edit_can_repair_an_invalid_complete_config() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "broken").unwrap();
    fs::write(
        selected.named_config_file("broken", "config.toml"),
        "unknown = true\n",
    )
    .unwrap();
    fs::write(
        selected.named_config_file("broken", "auth.json"),
        "not-json\n",
    )
    .unwrap();
    let editor_dir = tempfile::tempdir().unwrap();
    let editor = crate::testutil::write_stub_script(
        editor_dir.path(),
        "editor",
        "#!/bin/sh\ncase \"$1\" in\n  *config.toml*) printf 'model = \"repaired\"\\n' > \"$1\" ;;\n  *auth.json*) printf '{}\\n' > \"$1\" ;;\nesac\n",
    );
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());

    edit_named_config(&selected, "broken").unwrap();
    apply_named_config(&selected, "broken").unwrap();

    assert!(fs::read_to_string(selected.state_file("config.toml"))
        .unwrap()
        .contains("model = \"repaired\""));
}

#[cfg(unix)]
#[test]
fn edit_current_initializes_missing_state_and_preserves_raw_invalid_content() {
    use std::os::unix::fs::PermissionsExt;

    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "new").unwrap();
    let selected = tenant.for_agent(AgentKind::Codex);
    let editor_dir = tempfile::tempdir().unwrap();
    let editor = crate::testutil::write_stub_script(
        editor_dir.path(),
        "editor",
        "#!/bin/sh\ncase \"$1\" in\n  *config.toml*) printf 'not toml' > \"$1\" ;;\n  *auth.json*) printf 'not json' > \"$1\" ;;\nesac\n",
    );
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());

    edit_current_config(&selected).unwrap();

    assert_eq!(
        fs::read(selected.state_file("config.toml")).unwrap(),
        b"not toml"
    );
    assert_eq!(
        fs::read(selected.state_file("auth.json")).unwrap(),
        b"not json"
    );
    for file in selected.agent.config_files() {
        assert_eq!(
            fs::metadata(selected.state_file(file))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[test]
fn edit_current_keeps_an_earlier_commit_when_the_next_editor_fails() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    fs::write(selected.state_file("config.toml"), "old-main\n").unwrap();
    fs::write(selected.state_file("auth.json"), "old-auth\n").unwrap();
    let editor_dir = tempfile::tempdir().unwrap();
    let editor = crate::testutil::write_stub_script(
        editor_dir.path(),
        "editor",
        "#!/bin/sh\ncase \"$1\" in\n  *config.toml*) printf 'new-main' > \"$1\" ;;\n  *auth.json*) exit 7 ;;\nesac\n",
    );
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());

    let error = edit_current_config(&selected).unwrap_err().to_string();

    assert!(error.contains("editor exited"), "{error}");
    assert_eq!(
        fs::read(selected.state_file("config.toml")).unwrap(),
        b"new-main"
    );
    assert_eq!(
        fs::read(selected.state_file("auth.json")).unwrap(),
        b"old-auth\n"
    );
}

#[test]
fn claude_apply_sets_and_removes_fixed_fields_without_touching_statusline() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    create_named_config(&selected, "partial").unwrap();
    replace_config_files(
        &selected,
        "partial",
        r#"{
          "env": {"ANTHROPIC_BASE_URL": "https://new"},
          "permissions": {"defaultMode": "bypassPermissions"}
        }
"#,
        "{}\n",
    );
    fs::write(
        selected.state_file("settings.json"),
        r#"{
          "env": {
            "ANTHROPIC_BASE_URL": "https://old",
            "ANTHROPIC_DEFAULT_HAIKU_MODEL": "old",
            "ANTHROPIC_AUTH_TOKEN": "old-token",
            "KEEP": "yes"
          },
          "permissions": "conflict",
          "statusLine": {"type":"command","command":"keep"}
        }
"#,
    )
    .unwrap();

    apply_named_config(&selected, "partial").unwrap();

    let settings: Value =
        serde_json::from_str(&fs::read_to_string(selected.state_file("settings.json")).unwrap())
            .unwrap();
    assert_eq!(settings["env"]["ANTHROPIC_BASE_URL"], "https://new");
    assert_eq!(settings["env"]["KEEP"], "yes");
    assert!(settings["env"].get("ANTHROPIC_AUTH_TOKEN").is_none());
    assert!(settings["env"]
        .get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
        .is_none());
    assert_eq!(settings["permissions"]["defaultMode"], "bypassPermissions");
    assert_eq!(settings["statusLine"]["command"], "keep");
}

#[test]
fn codex_apply_preserves_toml_comments_unrelated_values_and_statusline() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "partial").unwrap();
    replace_config_files(
        &selected,
        "partial",
        "model = \"new\"\n\n[model_providers.custom]\nname = \"custom\"\n",
        "{\"OPENAI_API_KEY\":\"new\"}\n",
    );
    fs::write(
        selected.state_file("config.toml"),
        "# keep comment\nmodel = \"old\"\nsandbox_mode = \"workspace-write\"\nkeep = true\n\n[tui]\nstatus_line = [\"model\"]\nstatus_line_use_colors = true\n",
    )
    .unwrap();
    fs::write(
        selected.state_file("auth.json"),
        "{\"old\":\"credential\"}\n",
    )
    .unwrap();

    apply_named_config(&selected, "partial").unwrap();

    let config = fs::read_to_string(selected.state_file("config.toml")).unwrap();
    assert!(config.contains("# keep comment"), "{config}");
    assert!(config.contains("model = \"new\""), "{config}");
    assert!(!config.contains("sandbox_mode"), "{config}");
    assert!(config.contains("keep = true"), "{config}");
    assert!(config.contains("status_line ="), "{config}");
    let auth: Value =
        serde_json::from_str(&fs::read_to_string(selected.state_file("auth.json")).unwrap())
            .unwrap();
    assert_eq!(auth, serde_json::json!({"OPENAI_API_KEY": "new"}));
}

#[test]
fn empty_config_keeps_missing_agent_files_absent() {
    let root = tempfile::tempdir().unwrap();
    for agent in AgentKind::ALL {
        let selected = selected(root.path(), agent);
        create_named_config(&selected, agent.tag()).unwrap();
        replace_config_files(
            &selected,
            agent.tag(),
            if agent == AgentKind::Claude {
                "{}\n"
            } else {
                ""
            },
            "{}\n",
        );

        apply_named_config(&selected, agent.tag()).unwrap();

        assert!(!selected
            .state_file(selected.agent.main_config_file())
            .exists());
        if let Some(auth) = selected.agent.native_auth_file() {
            assert!(!selected.state_file(auth).exists());
        }
    }
}

#[test]
fn apply_materializes_missing_codex_auth_file() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();

    apply_named_config(&selected, "custom").unwrap();

    assert!(selected.state_file("config.toml").is_file());
    assert_eq!(
        fs::read_to_string(selected.state_file("auth.json")).unwrap(),
        "{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n"
    );
}

#[cfg(unix)]
#[test]
fn apply_preserves_existing_modes_and_uses_0600_for_new_files() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let codex = selected(root.path(), AgentKind::Codex);
    create_named_config(&codex, "empty").unwrap();
    replace_config_files(&codex, "empty", "", "{}\n");
    fs::write(codex.state_file("config.toml"), "model = \"old\"\n").unwrap();
    fs::write(codex.state_file("auth.json"), "{\"old\":true}\n").unwrap();
    fs::set_permissions(
        codex.state_file("config.toml"),
        fs::Permissions::from_mode(0o640),
    )
    .unwrap();
    fs::set_permissions(
        codex.state_file("auth.json"),
        fs::Permissions::from_mode(0o400),
    )
    .unwrap();

    apply_named_config(&codex, "empty").unwrap();

    assert!(codex.state_file("config.toml").is_file());
    assert!(fs::read_to_string(codex.state_file("config.toml"))
        .unwrap()
        .trim()
        .is_empty());
    assert_eq!(
        fs::read_to_string(codex.state_file("auth.json")).unwrap(),
        "{}\n"
    );
    assert_eq!(
        fs::metadata(codex.state_file("config.toml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    assert_eq!(
        fs::metadata(codex.state_file("auth.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o400
    );

    let claude = selected(root.path(), AgentKind::Claude);
    create_named_config(&claude, "new").unwrap();
    apply_named_config(&claude, "new").unwrap();
    assert_eq!(
        fs::metadata(claude.state_file("settings.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn malformed_current_auth_prevents_all_codex_writes() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    let original = "# untouched\nmodel = \"old\"\n";
    fs::write(selected.state_file("config.toml"), original).unwrap();
    fs::write(selected.state_file("auth.json"), "not-json\n").unwrap();

    let error = apply_named_config(&selected, "custom")
        .unwrap_err()
        .to_string();

    assert!(error.contains("Current Config auth.json"), "{error}");
    assert_eq!(
        fs::read_to_string(selected.state_file("config.toml")).unwrap(),
        original
    );
    assert_eq!(
        fs::read_to_string(selected.state_file("auth.json")).unwrap(),
        "not-json\n"
    );
}

#[test]
fn repeated_apply_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    apply_named_config(&selected, "custom").unwrap();
    let main = fs::read(selected.state_file("config.toml")).unwrap();
    let auth = fs::read(selected.state_file("auth.json")).unwrap();

    apply_named_config(&selected, "custom").unwrap();

    assert_eq!(fs::read(selected.state_file("config.toml")).unwrap(), main);
    assert_eq!(fs::read(selected.state_file("auth.json")).unwrap(), auth);
}

#[test]
fn delete_all_includes_invalid_and_incomplete_configs() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "valid").unwrap();
    create_named_config(&selected, "invalid").unwrap();
    fs::write(
        selected.named_config_file("invalid", "config.toml"),
        "unknown = true\n",
    )
    .unwrap();
    tenant::ensure_real_dir(
        &selected.named_config_dir("partial"),
        "Named Config directory",
    )
    .unwrap();

    delete_named_configs(
        &selected,
        &["invalid".to_string(), "partial".to_string()],
        false,
        true,
    )
    .unwrap();
    assert!(!selected.named_config_dir("invalid").exists());
    assert!(!selected.named_config_dir("partial").exists());

    create_named_config(&selected, "invalid").unwrap();
    fs::write(
        selected.named_config_file("invalid", "config.toml"),
        "unknown = true\n",
    )
    .unwrap();
    tenant::ensure_real_dir(
        &selected.named_config_dir("partial"),
        "Named Config directory",
    )
    .unwrap();

    delete_named_configs(&selected, &[], true, true).unwrap();

    assert!(list_named_configs(&selected).unwrap().is_empty());
    assert!(!selected.named_config_dir("valid").exists());
    assert!(!selected.named_config_dir("invalid").exists());
    assert!(!selected.named_config_dir("partial").exists());
}

#[cfg(unix)]
#[test]
fn deletion_prevalidates_unsafe_targets() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "valid").unwrap();
    fs::write(outside.path().join("keep"), "outside").unwrap();
    symlink(outside.path(), selected.named_config_dir("linked")).unwrap();

    let error = delete_named_configs(
        &selected,
        &["valid".to_string(), "linked".to_string()],
        false,
        true,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("not a real directory"), "{error}");
    assert!(selected.named_config_dir("valid").is_dir());
    assert_eq!(
        fs::read_to_string(outside.path().join("keep")).unwrap(),
        "outside"
    );
}

#[cfg(unix)]
#[test]
fn config_catalog_permissions_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();

    for directory in [
        selected.named_config_catalog_dir(),
        selected.named_config_dir("custom").as_path(),
    ] {
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700,
            "{}",
            directory.display()
        );
    }
    for file in selected.agent.config_files() {
        assert_eq!(
            fs::metadata(selected.named_config_file("custom", file))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[test]
fn host_apply_preserves_existing_home_and_file_modes() {
    use std::os::unix::fs::PermissionsExt;

    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let agent_dir = home.path().join(".claude");
    fs::create_dir(&agent_dir).unwrap();
    fs::set_permissions(&agent_dir, fs::Permissions::from_mode(0o711)).unwrap();
    let settings = agent_dir.join("settings.json");
    fs::write(&settings, "{\"theme\":\"dark\"}\n").unwrap();
    fs::set_permissions(&settings, fs::Permissions::from_mode(0o640)).unwrap();
    let _home = EnvGuard::set("HOME", home.path());
    let selected = Tenant::resolve(root.path(), true, "default")
        .unwrap()
        .for_agent(AgentKind::Claude);

    create_named_config(&selected, "custom").unwrap();
    apply_named_config(&selected, "custom").unwrap();

    assert_eq!(
        fs::metadata(agent_dir).unwrap().permissions().mode() & 0o777,
        0o711
    );
    assert_eq!(
        fs::metadata(settings).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert!(!home.path().join(".gitconfig").exists());
}

#[cfg(unix)]
#[test]
fn host_current_edit_preserves_existing_home_and_file_modes() {
    use std::os::unix::fs::PermissionsExt;

    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let agent_dir = home.path().join(".claude");
    fs::create_dir(&agent_dir).unwrap();
    fs::set_permissions(&agent_dir, fs::Permissions::from_mode(0o711)).unwrap();
    let settings = agent_dir.join("settings.json");
    fs::write(&settings, "old\n").unwrap();
    fs::set_permissions(&settings, fs::Permissions::from_mode(0o640)).unwrap();
    let editor_dir = tempfile::tempdir().unwrap();
    let editor = crate::testutil::write_stub_script(
        editor_dir.path(),
        "editor",
        "#!/bin/sh\nprintf 'new raw content' > \"$1\"\n",
    );
    let _home = EnvGuard::set("HOME", home.path());
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());
    let selected = Tenant::resolve(root.path(), true, "default")
        .unwrap()
        .for_agent(AgentKind::Claude);

    edit_current_config(&selected).unwrap();

    assert_eq!(fs::read(&settings).unwrap(), b"new raw content");
    assert_eq!(
        fs::metadata(agent_dir).unwrap().permissions().mode() & 0o777,
        0o711
    );
    assert_eq!(
        fs::metadata(settings).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert!(!home.path().join(".gitconfig").exists());
}

#[test]
fn editor_command_parsing_preserves_quoted_escaped_and_empty_arguments() {
    let parts = split_editor_command(OsStr::new(
        r#"code --wait "two words" 'literal $value' escaped\ space """#,
    ))
    .unwrap();
    assert_eq!(
        parts,
        [
            "code",
            "--wait",
            "two words",
            "literal $value",
            "escaped space",
            "",
        ]
        .map(OsString::from)
    );

    for invalid in ["   ", "code 'unterminated", "code trailing\\"] {
        assert!(split_editor_command(OsStr::new(invalid)).is_err());
    }
}

#[test]
fn config_deletion_requires_explicit_selection() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    let error = delete_named_configs(&selected, &[], false, true)
        .unwrap_err()
        .to_string();
    assert!(error.contains("at least one"), "{error}");

    create_named_config(&selected, "custom").unwrap();
    if !io::stdin().is_terminal() {
        let error = delete_named_configs(&selected, &["custom".to_string()], false, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("without --yes"), "{error}");
    }
}
