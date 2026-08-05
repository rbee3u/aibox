use super::*;
use crate::agent::AgentKind;
use crate::profile::{create_profile, list_profiles};
use crate::tenant::{self, ManagedTenant};
use std::fs;
use std::path::Path;

fn selected(root: &Path, agent: AgentKind) -> TenantAgent {
    let tenant = ManagedTenant::resolve(root, "work").unwrap();
    tenant.ensure_initialized().unwrap();
    tenant.for_agent(agent)
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
