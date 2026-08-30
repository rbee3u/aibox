use super::*;
use crate::config::{NamedConfigName, create_named_config, ensure_named_config_directory};
use serde_json::json;

fn named_config_file(selected: &TenantAgent, name: &str, file: &str) -> PathBuf {
    let name = NamedConfigName::parse(name).unwrap();
    let file = ConfigFile::parse(selected.agent(), file).unwrap();
    crate::config::layout::named_config_file(selected, &name, file)
}

fn credential(account: &str, refreshed: &str, marker: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "auth_mode": "chatgpt",
        "tokens": { "account_id": account },
        "last_refresh": refreshed,
        "marker": marker,
    }))
    .unwrap()
}

fn host_agent(root: &Path, host_home: &Path) -> TenantAgent {
    let selected = Tenant::Host {
        home_dir: host_home.to_path_buf(),
        root_dir: root.to_path_buf(),
    }
    .for_agent(AgentKind::Codex);
    selected.ensure_agent_state_dir().unwrap();
    selected
}

fn set_named_auth(selected: &TenantAgent, name: &str, content: &[u8]) {
    create_named_config(selected, &NamedConfigName::parse(name).unwrap()).unwrap();
    fs::write(named_config_file(selected, name, "auth.json"), content).unwrap();
}

#[test]
fn planning_classifies_candidates_and_orders_existing_complete_targets() {
    let root = tempfile::tempdir().unwrap();
    let host_home = tempfile::tempdir().unwrap();
    let host = host_agent(root.path(), host_home.path());
    let source = credential("same", "2026-08-08T00:00:00Z", "source");
    fs::write(host.state_file("auth.json"), &source).unwrap();

    set_named_auth(
        &host,
        "older",
        &credential("same", "2026-08-01T00:00:00Z", "old"),
    );
    set_named_auth(&host, "unchanged", &source);
    set_named_auth(
        &host,
        "conflict",
        &credential("same", "2026-08-08T00:00:00Z", "different"),
    );
    set_named_auth(
        &host,
        "newer",
        &credential("same", "2026-08-09T00:00:00Z", "newer"),
    );
    set_named_auth(&host, "invalid", br#"{"auth_mode":"chatgpt"}"#);
    set_named_auth(
        &host,
        "other-account",
        &credential("other", "2026-08-01T00:00:00Z", "other"),
    );
    set_named_auth(&host, "api-key", br#"{"auth_mode":"apikey"}"#);
    ensure_named_config_directory(&host, &NamedConfigName::parse("incomplete").unwrap()).unwrap();

    let managed = ManagedTenant::resolve(root.path(), "work").unwrap();
    managed.ensure_initialized().unwrap();
    let managed = managed.for_agent(AgentKind::Codex);
    fs::write(
        managed.state_file("auth.json"),
        credential("same", "2026-08-02T00:00:00Z", "managed"),
    )
    .unwrap();

    let preview = preview_auth_propagation(
        &plan_auth_propagation_from(root.path(), host_home.path()).unwrap(),
    );
    let labels = preview
        .entries
        .iter()
        .map(|entry| entry.label.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        [
            "host/config/conflict",
            "host/config/invalid",
            "host/config/newer",
            "host/config/older",
            "host/config/unchanged",
            "tenant/work/current",
        ]
    );
    assert_eq!(preview.updates, 2);
    assert!(matches!(
        preview.entries[0].outcome,
        PropagationOutcome::Conflict { .. }
    ));
    assert!(matches!(
        preview.entries[1].outcome,
        PropagationOutcome::Invalid { .. }
    ));
    assert!(matches!(
        preview.entries[2].outcome,
        PropagationOutcome::Newer { .. }
    ));
    assert_eq!(preview.entries[3].outcome, PropagationOutcome::Updated);
    assert_eq!(preview.entries[4].outcome, PropagationOutcome::Unchanged);
    assert_eq!(preview.entries[5].outcome, PropagationOutcome::Updated);
    assert!(!named_config_file(&host, "incomplete", "auth.json").exists());
}

#[test]
fn unsafe_structural_preflight_aborts_before_any_target_write() {
    let root = tempfile::tempdir().unwrap();
    let host_home = tempfile::tempdir().unwrap();
    let host = host_agent(root.path(), host_home.path());
    fs::write(
        host.state_file("auth.json"),
        credential("same", "2026-08-08T00:00:00Z", "source"),
    )
    .unwrap();
    let managed = ManagedTenant::resolve(root.path(), "work").unwrap();
    managed.ensure_initialized().unwrap();
    let target = managed.for_agent(AgentKind::Codex).state_file("auth.json");
    let original = credential("same", "2026-08-01T00:00:00Z", "target");
    fs::write(&target, &original).unwrap();
    fs::write(
        root.path().join("tenants/unsafe"),
        b"not a Tenant directory",
    )
    .unwrap();

    let error = plan_auth_propagation_from(root.path(), host_home.path()).unwrap_err();
    assert!(format!("{error:#}").contains("Managed Tenant entry is not a real directory"));
    assert_eq!(fs::read(target).unwrap(), original);
}

#[test]
fn execution_uses_the_snapshot_continues_after_failure_and_preserves_modes() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing/auth.json");
    let target = root.path().join("target.json");
    fs::write(&target, b"old").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    }
    let snapshot = credential("same", "2026-08-08T00:00:00Z", "snapshot");
    let plan = AuthPropagationPlan {
        source_content: snapshot.clone(),
        targets: vec![
            PlannedTarget {
                label: "first".to_string(),
                action: PlannedAction::Write {
                    path: missing,
                    mode: 0o600,
                },
            },
            PlannedTarget {
                label: "second".to_string(),
                action: PlannedAction::Write {
                    path: target.clone(),
                    mode: 0o640,
                },
            },
        ],
    };

    let report = execute_auth_propagation(plan);
    assert!(matches!(
        report.entries[0].outcome,
        PropagationOutcome::Failed { .. }
    ));
    assert_eq!(report.entries[1].outcome, PropagationOutcome::Updated);
    assert_eq!(fs::read(&target).unwrap(), snapshot);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(target).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
}
