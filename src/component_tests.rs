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
}

#[test]
fn missing_managed_catalog_is_read_only_and_reports_components_uninstalled() {
    let root = tempfile::tempdir().unwrap();
    let tenant = ManagedTenant::resolve(root.path(), "work").unwrap();
    let inspection = inspect_catalog(&Tenant::Managed(tenant)).unwrap();
    assert!(
        inspection
            .iter()
            .all(|item| item.status == Some(ComponentStatus::NotInstalled))
    );
    assert!(!root.path().join("tenants/work").exists());
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
