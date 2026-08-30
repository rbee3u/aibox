//! Management Operation lifecycle, log cursors, and image build coordination.

use crate::docker;
use crate::service::operation::{OperationContext, OperationSnapshot};
use crate::service::state::ServiceState;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

#[derive(Clone)]
pub(crate) struct OperationCoordinator {
    state: ServiceState,
}

pub(crate) struct OperationView {
    pub(crate) operation: Option<OperationSnapshot>,
    pub(crate) gap: bool,
}

#[derive(Default)]
pub(crate) struct OperationEventCursor {
    operation_id: Option<String>,
    after_sequence: u64,
}

impl OperationCoordinator {
    pub(crate) fn new(state: ServiceState) -> Self {
        Self { state }
    }

    pub(crate) fn current(&self, after_sequence: Option<u64>) -> OperationView {
        let mut operation = self.state.operation_snapshot();
        let gap = operation.as_ref().is_some_and(|snapshot| {
            after_sequence.is_some_and(|sequence| sequence < snapshot.first_sequence)
        });
        if let (Some(snapshot), Some(sequence)) = (&mut operation, after_sequence) {
            snapshot.logs.retain(|entry| entry.sequence >= sequence);
        }
        OperationView { operation, gap }
    }

    pub(crate) fn event_cursor(&self) -> OperationEventCursor {
        OperationEventCursor::default()
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<()> {
        self.state.subscribe_operations()
    }

    pub(crate) fn start_build(&self, force: bool) -> Result<OperationSnapshot> {
        let image = self.state.image();
        let kind = if force {
            "build image without cache"
        } else {
            "build image"
        };
        self.start(kind, move |context| {
            let cache = if force {
                docker::BuildCache::NoCachePull
            } else {
                docker::BuildCache::Cached
            };
            context.log(format!("Building {image}"));
            let log_context = context.clone();
            let log: Arc<dyn Fn(String) + Send + Sync> = Arc::new(move |line| {
                log_context.log(line);
            });
            docker::build_image_for_service(
                &docker::DockerCli::system(),
                docker::DOCKERFILE,
                &image,
                cache,
                context.cancellation(),
                log,
            )?;
            Ok(format!("Built {image}"))
        })
    }

    pub(super) fn start<F>(
        &self,
        kind: impl Into<String>,
        operation: F,
    ) -> Result<OperationSnapshot>
    where
        F: FnOnce(OperationContext) -> Result<String> + Send + 'static,
    {
        self.state.start_management_operation(kind, operation)
    }

    pub(crate) fn cancel(&self, id: &str) -> Result<()> {
        self.state.cancel_operation(id)
    }

    pub(crate) fn cancel_current(&self) {
        self.state.cancel_current_operation();
    }

    pub(crate) async fn wait_until_idle(&self) {
        while self.state.management_operation_is_running() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

impl OperationEventCursor {
    pub(crate) fn next(&mut self, coordinator: &OperationCoordinator) -> OperationView {
        let mut operation = coordinator.state.operation_snapshot();
        if operation.as_ref().map(|snapshot| &snapshot.id) != self.operation_id.as_ref() {
            self.operation_id = operation.as_ref().map(|snapshot| snapshot.id.clone());
            self.after_sequence = 0;
        }
        let gap = operation
            .as_ref()
            .is_some_and(|snapshot| self.after_sequence < snapshot.first_sequence);
        if let Some(snapshot) = &mut operation {
            snapshot
                .logs
                .retain(|entry| entry.sequence >= self.after_sequence);
            self.after_sequence = snapshot.next_sequence;
        } else {
            self.after_sequence = 0;
        }
        OperationView { operation, gap }
    }
}

#[cfg(test)]
#[path = "operation_tests.rs"]
mod tests;
