use super::*;
use std::time::Duration;

async fn wait_until_finished(manager: &OperationManager) -> OperationSnapshot {
    let mut changed = manager.subscribe();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let snapshot = manager.snapshot().expect("operation exists");
            if snapshot.state != OperationState::Running {
                return snapshot;
            }
            match changed.recv().await {
                Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => panic!("operation observation closed"),
            }
        }
    })
    .await
    .expect("operation finishes")
}

#[tokio::test]
async fn only_one_operation_runs_and_cancellation_is_observable() {
    let manager = OperationManager::new();
    let (release, released) = std::sync::mpsc::channel();
    let started = manager
        .start("wait", move |context| {
            released.recv().expect("test releases operation");
            assert!(context.is_cancelled());
            Ok("stopped".to_string())
        })
        .unwrap();
    let error = manager
        .start("second", |_| Ok("impossible".to_string()))
        .unwrap_err()
        .to_string();
    assert!(error.contains("already running"), "{error}");

    manager.cancel(&started.id).unwrap();
    release.send(()).unwrap();
    let finished = wait_until_finished(&manager).await;
    assert_eq!(finished.state, OperationState::Cancelled);
    assert!(
        finished
            .logs
            .iter()
            .any(|entry| entry.message == "Cancellation requested")
    );
}

#[tokio::test]
async fn failed_operation_records_its_error_and_allows_the_next_operation() {
    let manager = OperationManager::new();
    let started = manager
        .start("fail", |_| anyhow::bail!("image build failed"))
        .unwrap();

    let failed = wait_until_finished(&manager).await;
    assert_eq!(failed.id, started.id);
    assert_eq!(failed.state, OperationState::Failed);
    assert!(failed.ended_at.is_some());
    assert_eq!(failed.result.as_deref(), Some("image build failed"));
    assert!(!manager.is_running());

    let next = manager.start("next", |_| Ok("ready".to_string())).unwrap();
    let succeeded = wait_until_finished(&manager).await;
    assert_eq!(succeeded.id, next.id);
    assert_eq!(succeeded.state, OperationState::Succeeded);
    assert_eq!(succeeded.result.as_deref(), Some("ready"));
}

#[tokio::test]
async fn log_ring_is_bounded_and_reports_the_retained_sequence_window() {
    let manager = OperationManager::new();
    manager
        .start("logs", |context| {
            context.log("a".repeat(600 * 1024));
            context.log("b".repeat(600 * 1024));
            context.log("tail");
            Ok("done".to_string())
        })
        .unwrap();
    let finished = wait_until_finished(&manager).await;
    assert_eq!(finished.state, OperationState::Succeeded);
    assert_eq!(finished.next_sequence, 3);
    assert_eq!(finished.first_sequence, 1);
    assert_eq!(finished.logs.front().unwrap().sequence, 1);
    assert!(
        finished
            .logs
            .iter()
            .map(|entry| entry.message.len())
            .sum::<usize>()
            <= LOG_LIMIT_BYTES
    );
}
