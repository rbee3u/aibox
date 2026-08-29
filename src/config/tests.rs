use super::*;
use crate::agent::AgentKind;
use crate::tenant::ManagedTenant;
use serde_json::{Value, json};
use std::path::Path;

fn write_private(path: &Path, content: &[u8]) {
    fs::write(path, content).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn selected(root: &Path, agent: AgentKind) -> TenantAgent {
    let tenant = ManagedTenant::resolve(root, "work").unwrap();
    tenant.ensure_initialized().unwrap();
    tenant.for_agent(agent)
}

#[test]
fn named_config_catalog_reports_ready_and_incomplete_entries() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "ready").unwrap();
    selected.ensure_named_config_catalog().unwrap();
    fs::create_dir(selected.named_config_dir("partial")).unwrap();
    fs::write(
        selected.named_config_file("partial", "config.toml"),
        b"model = \"x\"\n",
    )
    .unwrap();
    let entries = inspect_named_configs(&selected).unwrap();
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["partial", "ready"]
    );
    assert_eq!(entries[0].state, ConfigCatalogState::Incomplete);
    assert_eq!(entries[1].state, ConfigCatalogState::Ready);
}

#[test]
fn named_config_catalog_rejects_unsupported_provider_shapes() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    fs::write(
        selected.named_config_file("custom", "config.toml"),
        b"approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"gpt\"\nmodel_provider = \"openai\"\n",
    )
    .unwrap();
    let entries = inspect_named_configs(&selected).unwrap();
    assert_eq!(entries[0].state, ConfigCatalogState::Invalid);
}

#[test]
fn config_file_reads_and_saves_use_revisions_and_native_validation() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    create_named_config(&selected, "custom").unwrap();
    let before = read_config_file(&selected, Some("custom"), false, "settings.json").unwrap();
    assert!(before.exists);
    let after = save_config_file(
        &selected,
        Some("custom"),
        false,
        "settings.json",
        &before.revision,
        br#"{"env":{"ANTHROPIC_BASE_URL":"https://example.com","ANTHROPIC_AUTH_TOKEN":"token"},"permissions":{"defaultMode":"bypassPermissions"}}"#,
        None,
        None,
    )
    .unwrap();
    assert!(
        after
            .content
            .windows(b"token".len())
            .any(|window| window == b"token")
    );
    let stale = save_config_file(
        &selected,
        Some("custom"),
        false,
        "settings.json",
        &before.revision,
        b"{}",
        None,
        None,
    );
    assert!(stale.is_err());
}

#[test]
fn named_config_main_save_rejects_missing_required_fields_before_write() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    let before = read_config_file(&selected, Some("custom"), false, "config.toml").unwrap();
    let error = save_config_file(
        &selected,
        Some("custom"),
        false,
        "config.toml",
        &before.revision,
        b"model = \"gpt-test\"\napproval_policy = \"never\"\n",
        None,
        None,
    )
    .unwrap_err();
    let error_text = format!("{error:#}");
    assert!(
        error_text.contains("required Config Field sandbox_mode is missing"),
        "{error_text}"
    );
    let after = read_config_file(&selected, Some("custom"), false, "config.toml").unwrap();
    assert_eq!(after.content, before.content);
    assert_eq!(after.revision, before.revision);
}

#[test]
fn visual_custom_provider_save_materializes_missing_auth_placeholder() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    fs::remove_file(selected.named_config_file("custom", "auth.json")).unwrap();
    let before = read_config_file(&selected, Some("custom"), false, "config.toml").unwrap();
    let fields = inspect_visual_config(
        AgentKind::Codex,
        &String::from_utf8(before.content.clone()).unwrap(),
    )
    .unwrap();
    let saved = save_config_file_with_linked(
        &selected,
        Some("custom"),
        false,
        "config.toml",
        &before.revision,
        &before.content,
        Some(&CustomProviderInput {
            included: true,
            name: "custom".to_string(),
            base_url: "https://example.com/v1".to_string(),
            proxy_routed: false,
        }),
        Some(
            &fields
                .options
                .iter()
                .map(|field| VisualConfigOptionInput {
                    path: field.path.clone(),
                    included: field.included,
                    value: field.value.clone(),
                })
                .collect::<Vec<_>>(),
        ),
        None,
    )
    .unwrap();
    assert_eq!(
        saved.linked.as_ref().map(|file| file.file.as_str()),
        Some("auth.json")
    );
    assert!(
        fs::read_to_string(selected.named_config_file("custom", "auth.json"))
            .unwrap()
            .contains("sk-example")
    );
}

#[test]
fn apply_named_config_updates_current_files_and_records_status() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    fs::write(
        selected.named_config_file("custom", "config.toml"),
        b"approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"gpt\"\n",
    )
    .unwrap();
    fs::write(
        selected.named_config_file("custom", "auth.json"),
        br#"{"OPENAI_API_KEY":"secret"}"#,
    )
    .unwrap();
    apply_named_config(&selected, "custom").unwrap();
    assert!(
        fs::read_to_string(selected.state_file("config.toml"))
            .unwrap()
            .contains("gpt")
    );
    let status = application_status(&selected);
    assert_eq!(
        status
            .last_application
            .as_ref()
            .map(|record| record.applied.as_str()),
        Some("custom")
    );
    assert_eq!(status.drift, ConfigDrift::Clean);
}

#[test]
fn application_status_reports_all_five_drift_states() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    assert_eq!(application_status(&selected).drift, ConfigDrift::Untracked);

    create_named_config(&selected, "tracked").unwrap();
    apply_named_config(&selected, "tracked").unwrap();
    assert_eq!(application_status(&selected).drift, ConfigDrift::Clean);

    let current_path = selected.state_file("config.toml");
    let current = fs::read_to_string(&current_path).unwrap();
    write_private(
        &current_path,
        current.replace("gpt-5.6-sol", "different-model").as_bytes(),
    );
    assert_eq!(application_status(&selected).drift, ConfigDrift::Dirty);

    delete_named_configs(&selected, &["tracked".to_string()], false).unwrap();
    assert_eq!(
        application_status(&selected).drift,
        ConfigDrift::SourceMissing
    );

    create_named_config(&selected, "tracked").unwrap();
    apply_named_config(&selected, "tracked").unwrap();
    write_private(&current_path, b"\xff");
    let comparison_error = application_status(&selected);
    assert_eq!(comparison_error.drift, ConfigDrift::ComparisonError);
    assert!(
        comparison_error
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("not valid UTF-8"))
    );
}

#[test]
fn application_preserves_unknown_metadata_sections_and_reruns_to_the_same_current_config() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "tracked").unwrap();
    let metadata_path = crate::metadata::metadata_path(&selected);
    write_private(
        &metadata_path,
        br#"{"future_feature":{"enabled":true,"value":7}}"#,
    );

    apply_named_config(&selected, "tracked").unwrap();
    let first_main = fs::read(selected.state_file("config.toml")).unwrap();
    let first_auth = fs::read(selected.state_file("auth.json")).unwrap();
    apply_named_config(&selected, "tracked").unwrap();

    assert_eq!(
        fs::read(selected.state_file("config.toml")).unwrap(),
        first_main
    );
    assert_eq!(
        fs::read(selected.state_file("auth.json")).unwrap(),
        first_auth
    );
    assert_eq!(application_status(&selected).drift, ConfigDrift::Clean);
    let metadata: Value = serde_json::from_slice(&fs::read(metadata_path).unwrap()).unwrap();
    assert_eq!(
        metadata["future_feature"],
        json!({"enabled": true, "value": 7})
    );
    assert!(metadata.get("last_application").is_some());
}

#[cfg(unix)]
#[test]
fn metadata_commit_failure_keeps_committed_files_without_recording_application() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "tracked").unwrap();
    let catalog = selected.named_config_catalog_dir();
    fs::set_permissions(catalog, fs::Permissions::from_mode(0o500)).unwrap();

    let result = apply_named_config(&selected, "tracked");
    fs::set_permissions(catalog, fs::Permissions::from_mode(0o700)).unwrap();
    let error = result.unwrap_err();
    assert!(format!("{error:#}").contains("metadata"));
    assert!(selected.state_file("config.toml").is_file());
    assert!(selected.state_file("auth.json").is_file());
    assert!(!crate::metadata::metadata_path(&selected).exists());
    assert_eq!(application_status(&selected).drift, ConfigDrift::Untracked);
}

#[test]
fn metadata_rejects_malformed_and_oversized_documents() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "tracked").unwrap();
    let path = crate::metadata::metadata_path(&selected);

    write_private(&path, b"not json");
    assert!(format!("{:#}", crate::metadata::read(&selected).unwrap_err()).contains("parse"));

    write_private(&path, &vec![b'x'; 16 * 1024 + 1]);
    assert!(
        format!("{:#}", crate::metadata::read(&selected).unwrap_err()).contains("exceeds 16384")
    );
}

#[cfg(unix)]
#[test]
fn metadata_rejects_wrong_modes_and_symlinks() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "tracked").unwrap();
    let path = crate::metadata::metadata_path(&selected);
    write_private(&path, b"{}");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(format!("{:#}", crate::metadata::read(&selected).unwrap_err()).contains("mode 0600"));

    fs::remove_file(&path).unwrap();
    let target = root.path().join("foreign-metadata.json");
    write_private(&target, b"{}");
    symlink(&target, &path).unwrap();
    assert!(crate::metadata::read(&selected).is_err());
}

#[test]
fn named_config_deletion_requires_explicit_selection() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    create_named_config(&selected, "custom").unwrap();
    assert!(delete_named_configs(&selected, &[], false).is_err());
    delete_named_configs(&selected, &["custom".into()], false).unwrap();
    assert!(!selected.named_config_dir("custom").exists());
}

#[test]
fn current_config_can_be_initialized_and_preserves_arbitrary_bytes() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Claude);
    let before = read_config_file(&selected, None, true, "settings.json").unwrap();
    assert!(!before.exists);
    let after = save_config_file(
        &selected,
        None,
        true,
        "settings.json",
        &before.revision,
        b"not json\0bytes",
        None,
        None,
    )
    .unwrap();
    assert_eq!(after.content, b"not json\0bytes");
    let inspection = inspect_current_config(&selected).unwrap();
    assert_eq!(inspection.present_files, 1);
}

#[test]
fn missing_managed_current_config_is_a_read_only_empty_view() {
    let root = tempfile::tempdir().unwrap();
    let selected = ManagedTenant::resolve(root.path(), "missing")
        .unwrap()
        .for_agent(AgentKind::Codex);

    for file in selected.agent.config_files() {
        let snapshot = read_config_file(&selected, None, true, file).unwrap();
        assert!(!snapshot.exists);
        assert_eq!(
            snapshot.content,
            selected.agent.empty_config_file(file).unwrap().as_bytes()
        );
    }
    assert!(!root.path().join("tenants/missing").exists());
}

#[test]
fn raw_edit_can_create_a_missing_main_file_in_a_safe_incomplete_named_config() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    selected.ensure_named_config_catalog().unwrap();
    ensure_named_config_directory(&selected, "partial").unwrap();

    let before = read_config_file(&selected, Some("partial"), false, "config.toml").unwrap();
    assert!(!before.exists);
    assert!(before.content.is_empty());
    assert!(visual_config_state(&selected, "partial", "").is_err());

    let after = save_config_file(
        &selected,
        Some("partial"),
        false,
        "config.toml",
        &before.revision,
        b"approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"custom-model\"\n",
        None,
        None,
    )
    .unwrap();
    assert!(after.exists);
    assert_eq!(
        fs::read_to_string(selected.named_config_file("partial", "config.toml")).unwrap(),
        "approval_policy = \"never\"\nsandbox_mode = \"danger-full-access\"\nmodel = \"custom-model\"\n"
    );
}

#[test]
fn revealing_an_incomplete_named_config_still_rejects_unknown_entries() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    selected.ensure_named_config_catalog().unwrap();
    ensure_named_config_directory(&selected, "partial").unwrap();
    fs::write(
        selected.named_config_dir("partial").join("unexpected"),
        b"unsafe",
    )
    .unwrap();

    let error = read_config_file(&selected, Some("partial"), false, "config.toml")
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown entry"), "{error}");
}

#[test]
fn auth_propagation_preview_is_structured() {
    let root = tempfile::tempdir().unwrap();
    let host_home = tempfile::tempdir().unwrap();
    let host = Tenant::Host {
        home_dir: host_home.path().to_path_buf(),
        root_dir: root.path().to_path_buf(),
    };
    let selected = host.for_agent(AgentKind::Codex);
    selected.ensure_agent_state_dir().unwrap();
    fs::write(
        selected.state_file("auth.json"),
        br#"{"auth_mode":"chatgpt","OPENAI_API_KEY":null,"tokens":{"id_token":"id-x","access_token":"access-x","refresh_token":"refresh-x","account_id":"account-x"},"last_refresh":"2026-08-08T04:22:23.476121Z"}"#,
    )
    .unwrap();
    let preview = preview_auth_propagation(
        &plan_auth_propagation_from(root.path(), host_home.path()).unwrap(),
    );
    assert!(preview.entries.is_empty() || preview.updates == 0);
    let _: Value = serde_json::to_value(preview).unwrap();
}
