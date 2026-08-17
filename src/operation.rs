//! Service-lifetime management operations and bounded reconnectable logs.

use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::sync::broadcast;
use uuid::Uuid;

const LOG_LIMIT_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum OperationState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OperationLog {
    pub(crate) sequence: u64,
    pub(crate) message: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OperationSnapshot {
    pub(crate) id: String,
    pub(crate) kind: String,
    pub(crate) state: OperationState,
    pub(crate) started_at: String,
    pub(crate) ended_at: Option<String>,
    pub(crate) result: Option<String>,
    pub(crate) first_sequence: u64,
    pub(crate) next_sequence: u64,
    pub(crate) logs: VecDeque<OperationLog>,
}

struct OperationStore {
    snapshot: Option<OperationSnapshot>,
    log_bytes: usize,
    cancellation: Option<Arc<AtomicBool>>,
}

#[derive(Clone)]
pub(crate) struct OperationManager {
    store: Arc<Mutex<OperationStore>>,
    changed: broadcast::Sender<()>,
}

#[derive(Clone)]
pub(crate) struct OperationContext {
    manager: OperationManager,
    id: String,
    cancelled: Arc<AtomicBool>,
}

impl OperationManager {
    pub(crate) fn new() -> Self {
        let (changed, _) = broadcast::channel(32);
        Self {
            store: Arc::new(Mutex::new(OperationStore {
                snapshot: None,
                log_bytes: 0,
                cancellation: None,
            })),
            changed,
        }
    }

    pub(crate) fn snapshot(&self) -> Option<OperationSnapshot> {
        self.store.lock().ok()?.snapshot.clone()
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<()> {
        self.changed.subscribe()
    }

    pub(crate) fn is_running(&self) -> bool {
        self.snapshot()
            .is_some_and(|operation| operation.state == OperationState::Running)
    }

    pub(crate) fn start<F>(&self, kind: impl Into<String>, job: F) -> Result<OperationSnapshot>
    where
        F: FnOnce(OperationContext) -> Result<String> + Send + 'static,
    {
        let kind = kind.into();
        let id = Uuid::now_v7().to_string();
        let cancellation = Arc::new(AtomicBool::new(false));
        let snapshot = OperationSnapshot {
            id: id.clone(),
            kind,
            state: OperationState::Running,
            started_at: now(),
            ended_at: None,
            result: None,
            first_sequence: 0,
            next_sequence: 0,
            logs: VecDeque::new(),
        };
        {
            let mut store = self.store.lock().expect("Operation store poisoned");
            if store
                .snapshot
                .as_ref()
                .is_some_and(|operation| operation.state == OperationState::Running)
            {
                bail!("another Management Operation is already running");
            }
            store.snapshot = Some(snapshot.clone());
            store.log_bytes = 0;
            store.cancellation = Some(cancellation.clone());
        }
        let _ = self.changed.send(());
        let manager = self.clone();
        let context = OperationContext {
            manager: manager.clone(),
            id: id.clone(),
            cancelled: cancellation,
        };
        tokio::task::spawn_blocking(move || {
            let result = job(context.clone());
            manager.finish(&id, result, context.is_cancelled());
        });
        Ok(snapshot)
    }

    pub(crate) fn cancel(&self, id: &str) -> Result<()> {
        let cancellation = {
            let store = self.store.lock().expect("Operation store poisoned");
            let operation = store
                .snapshot
                .as_ref()
                .filter(|operation| operation.id == id)
                .ok_or_else(|| anyhow::anyhow!("Management Operation not found"))?;
            if operation.state != OperationState::Running {
                bail!("Management Operation is no longer running");
            }
            store.cancellation.clone()
        };
        if let Some(cancellation) = cancellation {
            cancellation.store(true, Ordering::SeqCst);
        }
        self.append_log(id, "Cancellation requested".to_string());
        crate::docker::cancel_active_container_operation();
        Ok(())
    }

    pub(crate) fn cancel_current(&self) {
        if let Some(snapshot) = self.snapshot()
            && snapshot.state == OperationState::Running
        {
            let _ = self.cancel(&snapshot.id);
        }
    }

    fn append_log(&self, id: &str, message: String) {
        let mut store = self.store.lock().expect("Operation store poisoned");
        let mut log_bytes = store.log_bytes;
        let Some(operation) = store
            .snapshot
            .as_mut()
            .filter(|operation| operation.id == id)
        else {
            return;
        };
        let bytes = message.len();
        let sequence = operation.next_sequence;
        operation.next_sequence += 1;
        operation.logs.push_back(OperationLog { sequence, message });
        log_bytes += bytes;
        while log_bytes > LOG_LIMIT_BYTES {
            let Some(removed) = operation.logs.pop_front() else {
                break;
            };
            log_bytes = log_bytes.saturating_sub(removed.message.len());
        }
        operation.first_sequence = operation
            .logs
            .front()
            .map_or(operation.next_sequence, |entry| entry.sequence);
        store.log_bytes = log_bytes;
        drop(store);
        let _ = self.changed.send(());
    }

    fn finish(&self, id: &str, result: Result<String>, cancelled: bool) {
        let mut store = self.store.lock().expect("Operation store poisoned");
        let Some(operation) = store
            .snapshot
            .as_mut()
            .filter(|operation| operation.id == id)
        else {
            return;
        };
        operation.ended_at = Some(now());
        match result {
            Ok(summary) if !cancelled => {
                operation.state = OperationState::Succeeded;
                operation.result = Some(summary);
            }
            Ok(_) => {
                operation.state = OperationState::Cancelled;
                operation.result = Some("Cancelled".to_string());
            }
            Err(error) if cancelled => {
                operation.state = OperationState::Cancelled;
                operation.result = Some(format!("{error:#}"));
            }
            Err(error) => {
                operation.state = OperationState::Failed;
                operation.result = Some(format!("{error:#}"));
            }
        }
        store.cancellation = None;
        drop(store);
        let _ = self.changed.send(());
    }
}

impl OperationContext {
    pub(crate) fn log(&self, message: impl Into<String>) {
        self.manager.append_log(&self.id, message.into());
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub(crate) fn cancellation(&self) -> Arc<AtomicBool> {
        self.cancelled.clone()
    }
}

fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
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
}
