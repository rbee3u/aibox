use super::*;

#[tokio::test]
async fn current_and_event_cursor_preserve_gap_and_incremental_log_semantics() {
    let root = tempfile::tempdir().unwrap();
    let coordinator = OperationCoordinator::new(crate::service::tests::test_state(root.path()));
    coordinator
        .start("logs", |context| {
            context.log("a".repeat(600 * 1024));
            context.log("b".repeat(600 * 1024));
            context.log("tail");
            Ok("done".to_string())
        })
        .unwrap();
    coordinator.wait_until_idle().await;

    let current = coordinator.current(Some(0));
    assert!(current.gap);
    let current = current.operation.unwrap();
    assert_eq!(current.first_sequence, 1);
    assert_eq!(current.logs.front().unwrap().sequence, 1);

    let mut cursor = coordinator.event_cursor();
    let first = cursor.next(&coordinator);
    assert!(first.gap);
    assert_eq!(first.operation.unwrap().logs.len(), 2);
    let second = cursor.next(&coordinator);
    assert!(!second.gap);
    assert!(second.operation.unwrap().logs.is_empty());
}
