use super::{ComponentStatusWire, component_counts_as_installed, topology_tenant};
use crate::component::{ComponentInspection, ComponentKind, ComponentStatus};
use crate::service::coordination::TopologyTenantSnapshot;

fn inspection(kind: ComponentKind, status: ComponentStatus) -> ComponentInspection {
    ComponentInspection {
        kind,
        status: Some(status),
        error: None,
    }
}

#[test]
fn modified_counts_as_installed_like_the_tenants_catalog() {
    assert!(component_counts_as_installed(Some(
        ComponentStatusWire::Installed
    )));
    assert!(component_counts_as_installed(Some(
        ComponentStatusWire::Modified
    )));
    assert!(!component_counts_as_installed(Some(
        ComponentStatusWire::Incomplete
    )));
    assert!(!component_counts_as_installed(Some(
        ComponentStatusWire::Unmanaged
    )));
    assert!(!component_counts_as_installed(Some(
        ComponentStatusWire::NotInstalled
    )));
    assert!(!component_counts_as_installed(None));
}

#[test]
fn topology_installed_count_includes_modified_and_keeps_it_in_attention() {
    let snapshot = TopologyTenantSnapshot {
        name: Some("shadow1".to_string()),
        display_name: "shadow1".to_string(),
        home: "/tmp/shadow1".to_string(),
        exists: true,
        agents: Vec::new(),
        components: Ok(vec![
            inspection(
                ComponentKind::Codex,
                ComponentStatus::Installed {
                    version: Some("1".to_string()),
                },
            ),
            inspection(ComponentKind::Claude, ComponentStatus::NotInstalled),
            inspection(
                ComponentKind::CodexStatusline,
                ComponentStatus::Installed { version: None },
            ),
            inspection(ComponentKind::ClaudeStatusline, ComponentStatus::Modified),
            inspection(
                ComponentKind::Node,
                ComponentStatus::Installed {
                    version: Some("24".to_string()),
                },
            ),
            inspection(
                ComponentKind::Python,
                ComponentStatus::Installed {
                    version: Some("3".to_string()),
                },
            ),
            inspection(
                ComponentKind::Rust,
                ComponentStatus::Installed {
                    version: Some("1".to_string()),
                },
            ),
            inspection(ComponentKind::Go, ComponentStatus::NotInstalled),
        ]),
    };
    let value = serde_json::to_value(topology_tenant(snapshot)).unwrap();
    assert_eq!(value["components"]["total"], 8);
    assert_eq!(value["components"]["installed"], 6);
    let attention = value["components"]["attention"].as_array().unwrap();
    assert_eq!(attention.len(), 1);
    assert_eq!(attention[0]["kind"], "claude-statusline");
    assert_eq!(attention[0]["status"], "modified");
}
