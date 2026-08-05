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

fn replace_profile_files(selected: &TenantAgent, profile: &str, main: &str, auth: &str) {
    fs::write(
        selected.profile_file(profile, selected.agent.main_config_file()),
        main,
    )
    .unwrap();
    fs::write(
        selected.profile_file(profile, selected.agent.profile_auth_file()),
        auth,
    )
    .unwrap();
}

#[test]
fn profiles_are_tenant_and_agent_local() {
    let root = tempfile::tempdir().unwrap();
    let codex = selected(root.path(), AgentKind::Codex);
    let claude = selected(root.path(), AgentKind::Claude);
    create_profile(&codex, "custom").unwrap();
    assert_eq!(list_profiles(&codex).unwrap(), ["custom"]);
    assert!(list_profiles(&claude).unwrap().is_empty());
}

#[test]
fn create_uses_templates_and_rejects_a_complete_existing_profile() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "custom").unwrap();

    let main = fs::read_to_string(selected.profile_file("custom", "config.toml")).unwrap();
    assert!(main.contains("name = \"custom\""), "{main}");
    assert_eq!(
        fs::read_to_string(selected.profile_file("custom", "auth.json")).unwrap(),
        "{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n"
    );
    assert!(!selected.state_file("config.toml").exists());
    assert!(!selected.state_file("auth.json").exists());
    let error = create_profile(&selected, "custom").unwrap_err().to_string();
    assert!(error.contains("already exists"), "{error}");

    fs::write(
        selected.profile_file("custom", "config.toml"),
        "invalid = true\n",
    )
    .unwrap();
    let error = create_profile(&selected, "custom").unwrap_err().to_string();
    assert!(error.contains("already exists"), "{error}");
}

#[cfg(unix)]
#[test]
fn create_repairs_only_safe_valid_incomplete_profiles() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    selected.ensure_profile_catalog().unwrap();
    let directory = selected.profile_dir("partial");
    tenant::ensure_real_dir(&directory, "Profile directory").unwrap();
    let main = selected.profile_file("partial", "config.toml");
    fs::write(&main, "model = \"kept\"\n").unwrap();
    tenant::set_600(&main).unwrap();

    create_profile(&selected, "partial").unwrap();

    assert_eq!(fs::read_to_string(&main).unwrap(), "model = \"kept\"\n");
    assert!(selected.profile_file("partial", "auth.json").is_file());

    let broken = selected.profile_dir("broken");
    tenant::ensure_real_dir(&broken, "Profile directory").unwrap();
    let broken_main = selected.profile_file("broken", "config.toml");
    fs::write(&broken_main, "unknown = true\n").unwrap();
    tenant::set_600(&broken_main).unwrap();
    let error = format!("{:#}", create_profile(&selected, "broken").unwrap_err());
    assert!(error.contains("unsupported Agent Profile Field"), "{error}");
    assert!(!selected.profile_file("broken", "auth.json").exists());

    let unknown = selected.profile_dir("unknown");
    tenant::ensure_real_dir(&unknown, "Profile directory").unwrap();
    fs::write(unknown.join("extra"), "unexpected").unwrap();
    let error = create_profile(&selected, "unknown")
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown entry"), "{error}");

    use std::os::unix::fs::symlink;
    let linked = selected.profile_dir("linked");
    tenant::ensure_real_dir(&linked, "Profile directory").unwrap();
    symlink(&main, linked.join("config.toml")).unwrap();
    let error = create_profile(&selected, "linked").unwrap_err().to_string();
    assert!(error.contains("non-regular file"), "{error}");

    assert_eq!(list_profiles(&selected).unwrap(), ["partial"]);
}

#[test]
fn invalid_complete_profiles_are_visible_readable_and_rejected_by_apply() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    create_profile(&selected, "broken").unwrap();
    fs::write(
        selected.profile_file("broken", "settings.json"),
        "{\"theme\":\"dark\"}\n",
    )
    .unwrap();

    assert_eq!(list_profiles(&selected).unwrap(), ["broken"]);
    assert!(get_profile(&selected, "broken", false)
        .unwrap()
        .contains("theme"));
    let error = format!("{:#}", apply_profile(&selected, "broken").unwrap_err());
    assert!(error.contains("/config/theme"), "{error}");
    assert!(!selected.state_file("settings.json").exists());
}

#[cfg(unix)]
#[test]
fn edit_can_repair_an_invalid_complete_profile() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "broken").unwrap();
    fs::write(
        selected.profile_file("broken", "config.toml"),
        "unknown = true\n",
    )
    .unwrap();
    let editor_dir = tempfile::tempdir().unwrap();
    let editor = crate::testutil::write_stub_script(
        editor_dir.path(),
        "editor",
        "#!/bin/sh\nprintf 'model = \"repaired\"\\n' > \"$1\"\n",
    );
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());

    edit_profile(&selected, "broken", false).unwrap();
    apply_profile(&selected, "broken").unwrap();

    assert!(fs::read_to_string(selected.state_file("config.toml"))
        .unwrap()
        .contains("model = \"repaired\""));
}

#[test]
fn claude_apply_sets_and_removes_fixed_fields_without_touching_statusline() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    create_profile(&selected, "partial").unwrap();
    replace_profile_files(
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

    apply_profile(&selected, "partial").unwrap();

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
    create_profile(&selected, "partial").unwrap();
    replace_profile_files(
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

    apply_profile(&selected, "partial").unwrap();

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
fn empty_profile_keeps_missing_agent_files_absent() {
    let root = tempfile::tempdir().unwrap();
    for agent in AgentKind::ALL {
        let selected = selected(root.path(), agent);
        create_profile(&selected, agent.tag()).unwrap();
        replace_profile_files(
            &selected,
            agent.tag(),
            if agent == AgentKind::Claude {
                "{}\n"
            } else {
                ""
            },
            "{}\n",
        );

        apply_profile(&selected, agent.tag()).unwrap();

        assert!(!selected
            .state_file(selected.agent.main_config_file())
            .exists());
        if let Some(auth) = selected.agent.native_auth_file() {
            assert!(!selected.state_file(auth).exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn apply_preserves_existing_modes_and_uses_0600_for_new_files() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let codex = selected(root.path(), AgentKind::Codex);
    create_profile(&codex, "empty").unwrap();
    replace_profile_files(&codex, "empty", "", "{}\n");
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

    apply_profile(&codex, "empty").unwrap();

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
    create_profile(&claude, "new").unwrap();
    apply_profile(&claude, "new").unwrap();
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
    create_profile(&selected, "custom").unwrap();
    let original = "# untouched\nmodel = \"old\"\n";
    fs::write(selected.state_file("config.toml"), original).unwrap();
    fs::write(selected.state_file("auth.json"), "not-json\n").unwrap();

    let error = apply_profile(&selected, "custom").unwrap_err().to_string();

    assert!(error.contains("Agent Configuration auth.json"), "{error}");
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
    create_profile(&selected, "custom").unwrap();
    apply_profile(&selected, "custom").unwrap();
    let main = fs::read(selected.state_file("config.toml")).unwrap();
    let auth = fs::read(selected.state_file("auth.json")).unwrap();

    apply_profile(&selected, "custom").unwrap();

    assert_eq!(fs::read(selected.state_file("config.toml")).unwrap(), main);
    assert_eq!(fs::read(selected.state_file("auth.json")).unwrap(), auth);
}

#[test]
fn delete_all_includes_invalid_and_incomplete_profiles() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "valid").unwrap();
    create_profile(&selected, "invalid").unwrap();
    fs::write(
        selected.profile_file("invalid", "config.toml"),
        "unknown = true\n",
    )
    .unwrap();
    tenant::ensure_real_dir(&selected.profile_dir("partial"), "Profile directory").unwrap();

    delete_profiles(
        &selected,
        &["invalid".to_string(), "partial".to_string()],
        false,
        true,
    )
    .unwrap();
    assert!(!selected.profile_dir("invalid").exists());
    assert!(!selected.profile_dir("partial").exists());

    create_profile(&selected, "invalid").unwrap();
    fs::write(
        selected.profile_file("invalid", "config.toml"),
        "unknown = true\n",
    )
    .unwrap();
    tenant::ensure_real_dir(&selected.profile_dir("partial"), "Profile directory").unwrap();

    delete_profiles(&selected, &[], true, true).unwrap();

    assert!(list_profiles(&selected).unwrap().is_empty());
    assert!(!selected.profile_dir("valid").exists());
    assert!(!selected.profile_dir("invalid").exists());
    assert!(!selected.profile_dir("partial").exists());
}

#[cfg(unix)]
#[test]
fn deletion_prevalidates_unsafe_targets() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "valid").unwrap();
    fs::write(outside.path().join("keep"), "outside").unwrap();
    symlink(outside.path(), selected.profile_dir("linked")).unwrap();

    let error = delete_profiles(
        &selected,
        &["valid".to_string(), "linked".to_string()],
        false,
        true,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("not a real directory"), "{error}");
    assert!(selected.profile_dir("valid").is_dir());
    assert_eq!(
        fs::read_to_string(outside.path().join("keep")).unwrap(),
        "outside"
    );
}

#[cfg(unix)]
#[test]
fn profile_catalog_permissions_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "custom").unwrap();

    for directory in [
        selected.profile_catalog_dir(),
        selected.profile_dir("custom").as_path(),
    ] {
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700,
            "{}",
            directory.display()
        );
    }
    for file in selected.agent.profile_files() {
        assert_eq!(
            fs::metadata(selected.profile_file("custom", file))
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

    create_profile(&selected, "custom").unwrap();
    apply_profile(&selected, "custom").unwrap();

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
fn profile_deletion_requires_explicit_selection() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    let error = delete_profiles(&selected, &[], false, true)
        .unwrap_err()
        .to_string();
    assert!(error.contains("at least one"), "{error}");

    create_profile(&selected, "custom").unwrap();
    if !io::stdin().is_terminal() {
        let error = delete_profiles(&selected, &["custom".to_string()], false, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("without --yes"), "{error}");
    }
}
