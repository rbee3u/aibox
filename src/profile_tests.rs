use super::*;
use crate::tenant::{ManagedTenant, Tenant};
use crate::testutil::EnvGuard;

fn selected(root: &Path, agent: AgentKind) -> TenantAgent {
    let tenant = ManagedTenant::resolve(root, "work").unwrap();
    tenant.ensure_initialized().unwrap();
    tenant.for_agent(agent)
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
fn creating_an_existing_valid_profile_preserves_edited_source() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "custom").unwrap();
    fs::write(
        selected.profile_file("custom", "config.toml"),
        b"model = \"edited\"\n",
    )
    .unwrap();
    fs::write(
        selected.profile_file("custom", "auth.json"),
        b"{\"token\":\"keep-secret\"}\n",
    )
    .unwrap();
    let before: Vec<_> = selected
        .agent
        .profile_files()
        .iter()
        .map(|file| fs::read(selected.profile_file("custom", file)).unwrap())
        .collect();

    create_profile(&selected, "custom").unwrap();

    let after: Vec<_> = selected
        .agent
        .profile_files()
        .iter()
        .map(|file| fs::read(selected.profile_file("custom", file)).unwrap())
        .collect();
    assert_eq!(
        after, before,
        "idempotent create must not reset source files"
    );
}

#[test]
fn codex_profile_uses_default_native_configuration() {
    let root = tempfile::tempdir().unwrap();
    let codex = selected(root.path(), AgentKind::Codex);

    create_profile(&codex, "custom").unwrap();

    assert_eq!(
        fs::read_to_string(codex.profile_file("custom", "config.toml")).unwrap(),
        r#"approval_policy = "never"
sandbox_mode = "danger-full-access"
model_reasoning_effort = "xhigh"
plan_mode_reasoning_effort = "xhigh"
model = "gpt-5.6-sol"
model_provider = "custom"

[model_providers.custom]
base_url = "https://example.com/v1"
requires_openai_auth = true
"#
    );
    assert_eq!(
        fs::read_to_string(codex.profile_file("custom", "auth.json")).unwrap(),
        "{\n  \"OPENAI_API_KEY\": \"sk-example\"\n}\n"
    );
}

#[test]
fn claude_profile_uses_default_native_configuration() {
    let root = tempfile::tempdir().unwrap();
    let claude = selected(root.path(), AgentKind::Claude);

    create_profile(&claude, "custom").unwrap();

    assert_eq!(
        fs::read_to_string(claude.profile_file("custom", "settings.json")).unwrap(),
        r#"{
  "env": {
    "ANTHROPIC_BASE_URL": "https://example.com",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4-5",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-5[1m]",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-5[1m]",
    "ANTHROPIC_DEFAULT_FABLE_MODEL": "claude-fable-5[1m]"
  },
  "permissions": {
    "defaultMode": "bypassPermissions"
  },
  "skipDangerousModePermissionPrompt": true
}
"#
    );
    assert_eq!(
        fs::read_to_string(claude.profile_file("custom", "auth.json")).unwrap(),
        "{\n  \"ANTHROPIC_AUTH_TOKEN\": \"sk-example\"\n}\n"
    );

    activate_profile(&claude, "custom", false).unwrap();
    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(claude.state_file("settings.json")).unwrap())
            .unwrap();
    assert_eq!(settings["env"]["ANTHROPIC_AUTH_TOKEN"], "sk-example");
    assert_eq!(settings["env"]["ANTHROPIC_BASE_URL"], "https://example.com");
    assert!(!claude.state_file("auth.json").exists());
}

#[test]
fn editor_configuration_errors_do_not_leave_profile_temporary_files() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "custom").unwrap();
    let auth = selected.profile_file("custom", "auth.json");
    fs::write(&auth, "{\"token\":\"secret\"}\n").unwrap();
    let original = fs::read(&auth).unwrap();
    let _visual = EnvGuard::set("VISUAL", "'");

    let error = edit_profile(&selected, "custom", true)
        .unwrap_err()
        .to_string();

    assert!(error.contains("unterminated quote"), "{error}");
    assert_eq!(fs::read(auth).unwrap(), original);
    let leftovers: Vec<_> = fs::read_dir(selected.profile_dir("custom"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .filter(|name| name.to_string_lossy().contains(".aibox-edit-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[cfg(unix)]
#[test]
fn successful_edit_validates_and_commits_the_temporary_profile_file() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "custom").unwrap();
    let editor_dir = tempfile::tempdir().unwrap();
    let editor = crate::testutil::write_stub_script(
        editor_dir.path(),
        "editor",
        r#"#!/bin/sh
printf 'model = "edited"\n' > "$1"
"#,
    );
    let _visual = EnvGuard::set("VISUAL", editor.as_os_str());

    edit_profile(&selected, "custom", false).unwrap();

    assert_eq!(
        get_profile(&selected, "custom", false).unwrap(),
        "model = \"edited\"\n"
    );
    let leftovers: Vec<_> = fs::read_dir(selected.profile_dir("custom"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .filter(|name| name.to_string_lossy().contains(".aibox-edit-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
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
        assert!(
            split_editor_command(OsStr::new(invalid)).is_err(),
            "{invalid:?} should be rejected"
        );
    }
}

#[test]
fn atomic_write_rejects_a_non_file_destination() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("occupied");
    fs::create_dir(&path).unwrap();

    let error = write_atomic(&path, b"replace", Some(0o600))
        .unwrap_err()
        .to_string();

    assert!(error.contains("not a regular file"), "{error}");
    assert!(path.is_dir());
}

#[test]
fn host_profile_creation_does_not_install_managed_tenant_baseline_files() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("HOME", home.path());
    let selected = Tenant::resolve(root.path(), true, "default")
        .unwrap()
        .for_agent(AgentKind::Claude);

    create_profile(&selected, "custom").unwrap();

    assert!(selected.profile_dir("custom").is_dir());
    assert!(!home.path().join(".gitconfig").exists());
    assert!(!home.path().join(".claude/statusline.sh").exists());
}

#[test]
fn host_profile_activation_does_not_install_managed_tenant_statusline() {
    let _env_lock = crate::test_env_lock();
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let _home = EnvGuard::set("HOME", home.path());
    let selected = Tenant::resolve(root.path(), true, "default")
        .unwrap()
        .for_agent(AgentKind::Claude);
    create_profile(&selected, "custom").unwrap();

    activate_profile(&selected, "custom", false).unwrap();

    assert!(home.path().join(".claude/settings.json").is_file());
    assert!(!home.path().join(".claude/statusline.sh").exists());
}

#[test]
fn missing_managed_tenant_ignores_orphaned_profile_metadata() {
    let root = tempfile::tempdir().unwrap();
    let managed = ManagedTenant::resolve(root.path(), "work").unwrap();
    let selected = managed.for_agent(AgentKind::Codex);
    let orphan = root.path().join("codex/work/custom");
    fs::create_dir_all(&orphan).unwrap();
    fs::write(
        orphan.join("config.toml"),
        AgentKind::Codex.profile_template(),
    )
    .unwrap();
    fs::write(orphan.join("auth.json"), "{}\n").unwrap();
    tenant::set_600(&orphan.join("auth.json")).unwrap();
    fs::write(orphan.join(PROFILE_METADATA_FILE), "{\"tombstones\":[]}\n").unwrap();

    assert!(list_profiles(&selected).unwrap().is_empty());
    assert!(read_active_state(&selected).unwrap().is_none());
    delete_profiles(&selected, &["custom".to_string()], false, true).unwrap();
    assert!(!managed.home_dir.exists());
    assert!(orphan.exists());
}

#[test]
fn activation_materializes_and_deactivation_restores_exact_base() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    fs::write(
        selected.state_file("settings.json"),
        b"{\"theme\":\"dark\"}\n",
    )
    .unwrap();
    let base = fs::read(selected.state_file("settings.json")).unwrap();
    create_profile(&selected, "custom").unwrap();
    activate_profile(&selected, "custom", false).unwrap();
    let active = fs::read_to_string(selected.state_file("settings.json")).unwrap();
    assert!(active.contains("ANTHROPIC_BASE_URL"));
    assert!(active.contains("theme"));
    deactivate_profile(&selected, false).unwrap();
    assert_eq!(
        fs::read(selected.state_file("settings.json")).unwrap(),
        base
    );
    assert!(read_active_state(&selected).unwrap().is_none());
}

#[test]
fn activating_the_same_profile_again_still_restores_the_original_base() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    let config = selected.state_file("settings.json");
    let base = b"{\"theme\":\"dark\"}\n";
    fs::write(&config, base).unwrap();
    create_profile(&selected, "custom").unwrap();

    activate_profile(&selected, "custom", false).unwrap();
    activate_profile(&selected, "custom", false).unwrap();
    deactivate_profile(&selected, false).unwrap();

    assert_eq!(fs::read(config).unwrap(), base);
}

#[test]
fn empty_codex_profile_auth_does_not_create_native_auth_file() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "custom").unwrap();
    fs::write(selected.profile_file("custom", "auth.json"), b"{}\n").unwrap();
    assert_eq!(
        fs::read_to_string(selected.profile_file("custom", "auth.json")).unwrap(),
        "{}\n"
    );

    activate_profile(&selected, "custom", false).unwrap();

    assert!(selected.state_file("config.toml").is_file());
    assert!(!selected.state_file("auth.json").exists());
}

#[cfg(unix)]
#[test]
fn empty_codex_profile_auth_preserves_existing_native_auth() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    let auth = selected.state_file("auth.json");
    let original = b"{\"token\":\"native\"}\n";
    fs::write(&auth, original).unwrap();
    fs::set_permissions(&auth, fs::Permissions::from_mode(0o400)).unwrap();
    create_profile(&selected, "custom").unwrap();
    fs::write(selected.profile_file("custom", "auth.json"), b"{}\n").unwrap();

    activate_profile(&selected, "custom", false).unwrap();

    assert_eq!(fs::read(&auth).unwrap(), original);
    assert_eq!(
        fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
        0o400
    );
}

#[test]
fn profile_that_owns_no_config_preserves_native_file_bytes() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    let config = selected.state_file("config.toml");
    let original = b"# keep this formatting\nmodel='native'\n";
    fs::write(&config, original).unwrap();
    create_profile(&selected, "empty").unwrap();
    fs::write(selected.profile_file("empty", "config.toml"), b"\n").unwrap();

    activate_profile(&selected, "empty", false).unwrap();

    assert_eq!(fs::read(&config).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn profile_auth_requires_exact_owner_read_write_mode() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "custom").unwrap();
    let auth = selected.profile_file("custom", "auth.json");
    fs::set_permissions(&auth, fs::Permissions::from_mode(0o400)).unwrap();

    let error = activate_profile(&selected, "custom", false)
        .unwrap_err()
        .to_string();

    assert!(error.contains("mode 0600"), "{error}");
    assert!(!selected.state_file("config.toml").exists());
}

#[cfg(unix)]
#[test]
fn codex_auth_permissions_round_trip_through_deactivation() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    let auth = selected.state_file("auth.json");
    fs::write(&auth, b"{\"token\":\"base\"}\n").unwrap();
    fs::set_permissions(&auth, fs::Permissions::from_mode(0o400)).unwrap();
    create_profile(&selected, "custom").unwrap();
    fs::write(
        selected.profile_file("custom", "auth.json"),
        b"{\"token\":\"profile\"}\n",
    )
    .unwrap();

    activate_profile(&selected, "custom", false).unwrap();
    assert_eq!(
        fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
        0o600
    );
    deactivate_profile(&selected, false).unwrap();
    assert_eq!(
        fs::metadata(&auth).unwrap().permissions().mode() & 0o777,
        0o400
    );
}

#[test]
fn working_drift_blocks_switch_without_explicit_discard() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    create_profile(&selected, "one").unwrap();
    create_profile(&selected, "two").unwrap();
    activate_profile(&selected, "one", false).unwrap();
    fs::write(
        selected.state_file("settings.json"),
        b"{\"changed\":true}\n",
    )
    .unwrap();
    let error = activate_profile(&selected, "two", false)
        .unwrap_err()
        .to_string();
    assert!(error.contains("working changes"));
    activate_profile(&selected, "two", true).unwrap();
}

#[test]
fn explicit_discard_recovers_from_malformed_working_configuration() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "one").unwrap();
    create_profile(&selected, "two").unwrap();
    for profile in ["one", "two"] {
        fs::write(
            selected.profile_file(profile, "auth.json"),
            format!("{{\"token\":\"{profile}\"}}\n"),
        )
        .unwrap();
    }
    activate_profile(&selected, "one", false).unwrap();

    fs::write(selected.state_file("auth.json"), b"not-json\n").unwrap();
    assert!(activate_profile(&selected, "two", false).is_err());
    activate_profile(&selected, "two", true).unwrap();
    assert_eq!(
        read_active_state(&selected).unwrap().unwrap().profile,
        "two"
    );

    fs::write(selected.state_file("auth.json"), b"not-json\n").unwrap();
    assert!(deactivate_profile(&selected, false).is_err());
    deactivate_profile(&selected, true).unwrap();
    assert!(read_active_state(&selected).unwrap().is_none());
    assert!(!selected.state_file("auth.json").exists());
}

#[test]
fn reconcile_moves_non_overlapping_changes_both_directions() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    create_profile(&selected, "custom").unwrap();
    fs::write(
        selected.profile_file("custom", "settings.json"),
        b"{\"model\":\"a\",\"source\":1}\n",
    )
    .unwrap();
    activate_profile(&selected, "custom", false).unwrap();
    fs::write(
        selected.profile_file("custom", "settings.json"),
        b"{\"model\":\"a\",\"source\":2}\n",
    )
    .unwrap();
    fs::write(
        selected.state_file("settings.json"),
        b"{\"model\":\"working\",\"source\":1}\n",
    )
    .unwrap();
    reconcile_profile(
        &selected,
        &ReconcileArgs {
            take_profile: Vec::new(),
            take_config: Vec::new(),
            take_profile_all: false,
            take_config_all: false,
        },
    )
    .unwrap();
    let source = fs::read_to_string(selected.profile_file("custom", "settings.json")).unwrap();
    let working = fs::read_to_string(selected.state_file("settings.json")).unwrap();
    assert!(source.contains("working"));
    assert!(source.contains('2'));
    assert!(working.contains("working"));
    assert!(working.contains('2'));
}

#[test]
fn unresolved_conflicts_are_atomic_and_explicit_choices_converge_both_sides() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    create_profile(&selected, "custom").unwrap();
    let source_path = selected.profile_file("custom", "settings.json");
    let working_path = selected.state_file("settings.json");
    fs::write(&source_path, b"{\"model\":\"base\",\"theme\":\"base\"}\n").unwrap();
    activate_profile(&selected, "custom", false).unwrap();

    fs::write(
        &source_path,
        b"{\"model\":\"profile\",\"theme\":\"profile\"}\n",
    )
    .unwrap();
    fs::write(
        &working_path,
        b"{\"model\":\"config\",\"theme\":\"config\"}\n",
    )
    .unwrap();
    let source_before = fs::read(&source_path).unwrap();
    let working_before = fs::read(&working_path).unwrap();
    let metadata_before = fs::read(selected.metadata_file()).unwrap();

    let error = reconcile_profile(
        &selected,
        &ReconcileArgs {
            take_profile: Vec::new(),
            take_config: Vec::new(),
            take_profile_all: false,
            take_config_all: false,
        },
    )
    .unwrap_err()
    .to_string();

    assert!(
        error.contains("unresolved Agent Profile conflicts"),
        "{error}"
    );
    assert_eq!(fs::read(&source_path).unwrap(), source_before);
    assert_eq!(fs::read(&working_path).unwrap(), working_before);
    assert_eq!(fs::read(selected.metadata_file()).unwrap(), metadata_before);

    reconcile_profile(
        &selected,
        &ReconcileArgs {
            take_profile: vec!["/config/model".to_string()],
            take_config: vec!["/config/theme".to_string()],
            take_profile_all: false,
            take_config_all: false,
        },
    )
    .unwrap();

    let source: serde_json::Value =
        serde_json::from_slice(&fs::read(&source_path).unwrap()).unwrap();
    let working: serde_json::Value =
        serde_json::from_slice(&fs::read(&working_path).unwrap()).unwrap();
    let expected = serde_json::json!({"model": "profile", "theme": "config"});
    assert_eq!(source, expected);
    assert_eq!(working, expected);
    assert!(!has_divergence(&selected).unwrap());
}

#[test]
fn opposite_explicit_resolutions_for_one_path_are_rejected() {
    let error = explicit_resolutions(&ReconcileArgs {
        take_profile: vec!["/config/model".to_string()],
        take_config: vec!["/config/model".to_string()],
        take_profile_all: false,
        take_config_all: false,
    })
    .unwrap_err()
    .to_string();

    assert!(error.contains("conflicting resolutions"), "{error}");
    assert!(error.contains("/config/model"), "{error}");
}

#[test]
fn clean_reconcile_rejects_a_stale_explicit_resolution() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "custom").unwrap();
    activate_profile(&selected, "custom", false).unwrap();

    let error = reconcile_profile(
        &selected,
        &ReconcileArgs {
            take_profile: vec!["/config/model".to_string()],
            take_config: Vec::new(),
            take_profile_all: false,
            take_config_all: false,
        },
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("not a current conflict"), "{error}");
    assert!(!has_divergence(&selected).unwrap());
}

#[test]
fn reconcile_adopts_codex_auth_refresh_and_deletion() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "custom").unwrap();
    fs::write(
        selected.profile_file("custom", "auth.json"),
        b"{\"token\":\"profile\"}\n",
    )
    .unwrap();
    activate_profile(&selected, "custom", false).unwrap();

    fs::write(
        selected.state_file("auth.json"),
        b"{\"token\":\"refreshed\",\"account\":\"new\"}\n",
    )
    .unwrap();
    assert!(has_divergence(&selected).unwrap());
    reconcile_profile(
        &selected,
        &ReconcileArgs {
            take_profile: Vec::new(),
            take_config: Vec::new(),
            take_profile_all: false,
            take_config_all: false,
        },
    )
    .unwrap();
    let source: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(selected.profile_file("custom", "auth.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        source,
        serde_json::json!({"account": "new", "token": "refreshed"})
    );
    assert!(!has_divergence(&selected).unwrap());

    fs::remove_file(selected.state_file("auth.json")).unwrap();
    reconcile_profile(
        &selected,
        &ReconcileArgs {
            take_profile: Vec::new(),
            take_config: Vec::new(),
            take_profile_all: false,
            take_config_all: false,
        },
    )
    .unwrap();
    assert!(!selected.state_file("auth.json").exists());
    assert!(
        fs::read_to_string(selected.profile_file("custom", PROFILE_METADATA_FILE))
            .unwrap()
            .contains("/auth")
    );
    assert!(!has_divergence(&selected).unwrap());
}

#[test]
fn diff_values_are_opt_in_and_auth_is_always_redacted() {
    let old = ProfileDefinition::parse(
        AgentKind::Codex,
        "model = \"old\"\n",
        "{\"token\":\"old-secret\"}",
        None,
    )
    .unwrap();
    let new = ProfileDefinition::parse(
        AgentKind::Codex,
        "model = \"new\"\n",
        "{\"token\":\"new-secret\"}",
        None,
    )
    .unwrap();
    let entries = agent_config::diff(&old, &new);
    let config = entries
        .iter()
        .find(|entry| entry.path.to_string() == "/config/model")
        .unwrap();
    let auth = entries
        .iter()
        .find(|entry| entry.path.to_string() == "/auth")
        .unwrap();

    assert_eq!(
        format_diff_entry("working", config, false),
        "working modified /config/model"
    );
    let visible = format_diff_entry("source", config, true);
    assert!(visible.contains("\"old\" -> \"new\""), "{visible}");
    let redacted = format_diff_entry("source", auth, true);
    assert!(redacted.contains("<redacted> -> <redacted>"), "{redacted}");
    assert!(!redacted.contains("secret"), "{redacted}");

    let safe_old = ProfileDefinition::parse(AgentKind::Claude, "{}", "{}", None).unwrap();
    let safe_new =
        ProfileDefinition::parse(AgentKind::Claude, r#"{"\u001b[31m":true}"#, "{}", None).unwrap();
    let control = agent_config::diff(&safe_old, &safe_new);
    let rendered = format_diff_entry("working", &control[0], false);
    assert!(!rendered.contains('\u{1b}'), "{rendered:?}");
    assert!(rendered.contains(r"\u{1b}[31m"), "{rendered:?}");
}

#[test]
fn pending_profile_creation_resumes_after_partial_application() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    selected.ensure_for_management().unwrap();
    let pending = PendingTransaction {
        changes: vec![
            PendingChange::ProfileDirectory {
                profile: "custom".to_string(),
                present: true,
            },
            profile_file_change(
                "custom",
                selected.agent.main_config_file(),
                AgentKind::Codex.profile_template(),
                0o600,
            ),
            profile_file_change(
                "custom",
                "auth.json",
                AgentKind::Codex.profile_auth_template(),
                0o600,
            ),
            profile_file_change(
                "custom",
                PROFILE_METADATA_FILE,
                "{\n  \"tombstones\": []\n}\n",
                0o600,
            ),
        ],
        active_profile: None,
    };
    write_scope_metadata(
        &selected,
        &ScopeMetadata {
            active_profile: None,
            pending: Some(pending.clone()),
        },
    )
    .unwrap();
    apply_change(&selected, &pending.changes[0]).unwrap();
    apply_change(&selected, &pending.changes[1]).unwrap();

    recover_pending(&selected).unwrap();

    assert_eq!(list_profiles(&selected).unwrap(), ["custom"]);
    assert!(read_scope_metadata(&selected).unwrap().pending.is_none());
}

#[test]
fn pending_agent_file_removal_is_idempotently_replayed() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    let config = selected.state_file("config.toml");
    fs::write(&config, b"model = \"native\"\n").unwrap();
    let pending = PendingTransaction {
        changes: vec![PendingChange::AgentFile {
            file: "config.toml".to_string(),
            snapshot: FileSnapshot {
                present: false,
                content: Vec::new(),
                mode: None,
            },
        }],
        active_profile: None,
    };
    write_scope_metadata(
        &selected,
        &ScopeMetadata {
            active_profile: None,
            pending: Some(pending.clone()),
        },
    )
    .unwrap();
    apply_pending(&selected, &pending).unwrap();

    recover_pending(&selected).unwrap();

    assert!(!config.exists());
    assert!(read_scope_metadata(&selected).unwrap().pending.is_none());
}

#[test]
fn pending_transaction_rejects_untyped_paths() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    selected.ensure_for_management().unwrap();
    fs::write(
        selected.metadata_file(),
        r#"{
  "active_profile": null,
  "pending": {
    "changes": [{
      "kind": "agent-file",
      "file": "../outside",
      "snapshot": {"present": false, "content": "", "mode": null}
    }],
    "active_profile": null
  }
}
"#,
    )
    .unwrap();
    tenant::set_600(&selected.metadata_file()).unwrap();

    let error = recover_pending(&selected).unwrap_err().to_string();

    assert!(error.contains("unsupported Agent file"), "{error}");
    assert!(!root.path().join("outside").exists());
}

#[test]
fn profile_deletion_requires_explicit_selection_and_confirmation() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    let error = delete_profiles(&selected, &[], false, true)
        .unwrap_err()
        .to_string();
    assert!(error.contains("at least one"));

    create_profile(&selected, "custom").unwrap();
    if !io::stdin().is_terminal() {
        let error = delete_profiles(&selected, &["custom".to_string()], false, false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("without --yes"), "{error}");
    }
    assert_eq!(list_profiles(&selected).unwrap(), ["custom"]);
}

#[test]
fn delete_all_keeps_the_active_profile() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "active").unwrap();
    create_profile(&selected, "inactive").unwrap();
    activate_profile(&selected, "active", false).unwrap();

    delete_profiles(&selected, &[], true, true).unwrap();

    assert_eq!(list_profiles(&selected).unwrap(), ["active"]);
    assert_eq!(
        read_active_state(&selected).unwrap().unwrap().profile,
        "active"
    );
}

#[test]
fn explicit_delete_with_an_active_profile_is_all_or_nothing() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "active").unwrap();
    create_profile(&selected, "inactive").unwrap();
    activate_profile(&selected, "active", false).unwrap();

    let error = delete_profiles(
        &selected,
        &["inactive".to_string(), "active".to_string()],
        false,
        true,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("is active"), "{error}");
    assert_eq!(list_profiles(&selected).unwrap(), ["active", "inactive"]);
    assert_eq!(
        read_active_state(&selected).unwrap().unwrap().profile,
        "active"
    );
}

#[cfg(unix)]
#[test]
fn profile_listing_ignores_unknown_incomplete_and_symlinked_entries() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "valid").unwrap();
    fs::create_dir(selected.profile_dir("incomplete")).unwrap();
    fs::create_dir(selected.metadata_dir().join("bad_name")).unwrap();
    symlink(outside.path(), selected.profile_dir("linked")).unwrap();

    assert_eq!(list_profiles(&selected).unwrap(), ["valid"]);
    assert!(!outside.path().join("config.toml").exists());
}

#[cfg(unix)]
#[test]
fn explicit_profile_reads_reject_symlinked_files() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "custom").unwrap();
    let outside_config = outside.path().join("config.toml");
    fs::write(&outside_config, b"model = \"outside\"\n").unwrap();
    let profile_config = selected.profile_file("custom", "config.toml");
    fs::remove_file(&profile_config).unwrap();
    symlink(&outside_config, &profile_config).unwrap();

    let error = get_profile(&selected, "custom", false)
        .unwrap_err()
        .to_string();

    assert!(error.contains("not a regular file"), "{error}");
    assert_eq!(fs::read(&outside_config).unwrap(), b"model = \"outside\"\n");
}

#[cfg(unix)]
#[test]
fn explicit_profile_deletion_prevalidates_unsafe_targets() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "valid").unwrap();
    fs::write(outside.path().join("keep"), b"outside").unwrap();
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
    assert_eq!(fs::read(outside.path().join("keep")).unwrap(), b"outside");
}

#[cfg(unix)]
#[test]
fn scope_metadata_is_private_and_omits_an_empty_pending_field() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "custom").unwrap();

    let metadata = fs::read_to_string(selected.metadata_file()).unwrap();
    let mode = fs::metadata(selected.metadata_file())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    assert!(!metadata.contains("\"pending\""), "{metadata}");
}

#[cfg(unix)]
#[test]
fn profile_storage_and_materialized_configuration_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_profile(&selected, "custom").unwrap();

    let profile_dir = selected.profile_dir("custom");
    for directory in [selected.metadata_dir(), profile_dir.as_path()] {
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700,
            "{}",
            directory.display()
        );
    }
    for file in selected.agent.profile_files() {
        let path = selected.profile_file("custom", file);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600,
            "{}",
            path.display()
        );
    }

    activate_profile(&selected, "custom", false).unwrap();
    assert_eq!(
        fs::metadata(selected.state_file("config.toml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[cfg(unix)]
#[test]
fn host_directory_modes_stay_unchanged_and_deactivate_restores_file_mode() {
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
    activate_profile(&selected, "custom", false).unwrap();
    assert_eq!(
        fs::metadata(&agent_dir).unwrap().permissions().mode() & 0o777,
        0o711
    );
    assert_eq!(
        fs::metadata(&settings).unwrap().permissions().mode() & 0o777,
        0o600
    );

    deactivate_profile(&selected, false).unwrap();
    assert_eq!(
        fs::metadata(&agent_dir).unwrap().permissions().mode() & 0o777,
        0o711
    );
    assert_eq!(
        fs::metadata(&settings).unwrap().permissions().mode() & 0o777,
        0o640
    );
}
