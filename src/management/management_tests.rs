use super::*;
use crate::application_error::ApplicationError;
use std::time::Duration;

fn gate() -> ManagementGate {
    ManagementGate {
        lock: Arc::new(Mutex::new(())),
    }
}

#[tokio::test]
async fn cancelled_caller_does_not_release_a_running_mutation() {
    let gate = gate();
    let mutation = gate.begin().unwrap();
    let (started, started_rx) = tokio::sync::oneshot::channel();
    let (release, release_rx) = std::sync::mpsc::channel();
    let caller = tokio::spawn(mutation.run_blocking(move || {
        started.send(()).unwrap();
        release_rx.recv().unwrap();
        Ok(())
    }));
    started_rx.await.unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    let error = gate
        .begin()
        .err()
        .expect("the worker still owns the mutation");
    assert_eq!(
        ApplicationError::kind(&error),
        Some(ApplicationErrorKind::Busy)
    );

    release.send(()).unwrap();
    // Wait for the actual lock release instead of assuming the worker has run.
    let released = tokio::time::timeout(Duration::from_secs(2), gate.lock.lock())
        .await
        .unwrap();
    drop(released);
    assert!(gate.begin().is_ok());
}

#[tokio::test]
async fn failed_and_panicking_workers_release_mutation_ownership() {
    let gate = gate();
    let failed = gate
        .begin()
        .unwrap()
        .run_blocking(|| -> Result<()> { anyhow::bail!("domain failure") })
        .await
        .unwrap_err();
    assert_eq!(failed.to_string(), "domain failure");

    let panicked = gate
        .begin()
        .unwrap()
        .run_blocking(|| -> Result<()> { panic!("worker panic") })
        .await
        .unwrap_err();
    assert_eq!(
        ApplicationError::kind(&panicked),
        Some(ApplicationErrorKind::Internal)
    );
    assert!(gate.begin().is_ok());
}
