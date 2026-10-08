use super::*;
use crate::testutil::FixtureLatestProvider;
const VERSIONED_COMPONENT_COUNT: usize =
    crate::component::ComponentKind::ALL.len() - crate::component::ComponentKind::STATUSLINES.len();
fn test_components(root: &std::path::Path) -> ComponentCoordinator {
    ComponentCoordinator::new(
        super::super::ManagementPaths {
            root: Arc::new(root.to_path_buf()),
            host_home: Arc::new(root.join("host-home")),
        },
        super::super::ManagementGate {
            lock: Arc::new(Mutex::new(())),
        },
        OperationCoordinator::new(
            Arc::new("aibox:test".to_string()),
            super::super::OperationManager::new(),
        ),
        Arc::new(FixtureLatestProvider::empty()),
    )
}

#[tokio::test]
async fn component_update_prefetch_publishes_partial_but_hides_all_unavailable() {
    use crate::component::LatestResult;
    use crate::testutil::FixtureLatestProvider;
    use std::collections::BTreeMap;

    let unavailable_root = tempfile::tempdir().unwrap();
    let unavailable = test_components(unavailable_root.path());
    unavailable.prefetch().await;
    assert_eq!(unavailable.latest().await, None);

    let partial_root = tempfile::tempdir().unwrap();
    let mut partial = test_components(partial_root.path());
    partial.set_latest_provider(Arc::new(FixtureLatestProvider {
        results: BTreeMap::from([(
            "node".to_string(),
            LatestResult::Available {
                version: "24.19.0".to_string(),
                newest: Some("26.8.2".to_string()),
                source: "nodejs.org",
            },
        )]),
    }));
    partial.prefetch().await;

    let snapshot = partial.latest().await.unwrap();
    assert_eq!(snapshot.entries.len(), VERSIONED_COMPONENT_COUNT);
    assert!(snapshot.entries.iter().any(|entry| {
        entry.kind == crate::component::ComponentKind::Node
            && entry.state == crate::component::LatestEntryState::Available
    }));
    assert!(snapshot.entries.iter().any(|entry| {
        entry.kind == crate::component::ComponentKind::Codex
            && entry.state == crate::component::LatestEntryState::Unavailable
    }));
}
