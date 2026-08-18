use super::*;
use crate::agent::AgentKind;
use crate::tenant::ManagedTenant;
use serde_json::Value;
use std::path::Path;

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
    assert_eq!(entries[0].state, "incomplete");
    assert_eq!(entries[1].state, "ready");
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
        br#"{"env":{"ANTHROPIC_AUTH_TOKEN":"token"}}"#,
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
    );
    assert!(stale.is_err());
}

#[test]
fn apply_named_config_updates_current_files_and_records_status() {
    let root = tempfile::tempdir().unwrap();
    let selected = selected(root.path(), AgentKind::Codex);
    create_named_config(&selected, "custom").unwrap();
    fs::write(
        selected.named_config_file("custom", "config.toml"),
        b"model = \"gpt\"\n",
    )
    .unwrap();
    fs::write(
        selected.named_config_file("custom", "auth.json"),
        br#"{"token":"secret"}"#,
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
    )
    .unwrap();
    assert_eq!(after.content, b"not json\0bytes");
    let inspection = inspect_current_config(&selected).unwrap();
    assert_eq!(inspection.present_files, 1);
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
