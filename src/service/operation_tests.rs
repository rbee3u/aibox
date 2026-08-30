use super::*;
use std::time::Duration;

async fn wait_until_finished(manager: &OperationManager) -> OperationSnapshot {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let snapshot = manager.snapshot().expect("operation exists");
            if snapshot.state != OperationState::Running {
                return snapshot;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("operation finishes")
}

#[tokio::test]
async fn only_one_operation_runs_and_cancellation_is_observable() {
    let manager = OperationManager::new();
    let started = manager
        .start("wait", |context| {
            while !context.is_cancelled() {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok("stopped".to_string())
        })
        .unwrap();
    let error = manager
        .start("second", |_| Ok("impossible".to_string()))
        .unwrap_err()
        .to_string();
    assert!(error.contains("already running"), "{error}");

    manager.cancel(&started.id).unwrap();
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
