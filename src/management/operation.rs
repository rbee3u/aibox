//! Management Operation lifecycle, log cursors, and image build coordination.

use super::{OperationContext, OperationSnapshot};
use crate::docker;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

#[derive(Clone)]
pub(crate) struct OperationCoordinator {
    image: Arc<String>,
    manager: super::OperationManager,
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
    pub(super) fn new(image: Arc<String>, manager: super::OperationManager) -> Self {
        Self { image, manager }
    }

    pub(crate) fn current(&self, after_sequence: Option<u64>) -> OperationView {
        let mut operation = self.manager.snapshot();
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
        self.manager.subscribe()
    }

    pub(crate) fn start_build(&self, force: bool) -> Result<OperationSnapshot> {
        let image = self.image.clone();
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
            let log: docker::LogCallback = Arc::new(move |line| log_context.log(line));
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
        self.manager.start(kind, operation)
    }

    /// Seed a deterministic operation while testing Service shutdown and HTTP reads.
    #[cfg(test)]
    pub(crate) fn start_for_test<F>(
        &self,
        kind: impl Into<String>,
        operation: F,
    ) -> Result<OperationSnapshot>
    where
        F: FnOnce(OperationContext) -> Result<String> + Send + 'static,
    {
        self.start(kind, operation)
    }

    pub(crate) fn cancel(&self, id: &str) -> Result<()> {
        self.manager.cancel(id)?;
        docker::cancel_active_container_operation();
        Ok(())
    }

    pub(crate) fn cancel_current(&self) {
        if let Some(snapshot) = self.manager.snapshot()
            && snapshot.state == super::OperationState::Running
        {
            let _ = self.cancel(&snapshot.id);
        }
    }

    pub(crate) async fn wait_until_idle(&self) {
        while self.manager.is_running() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

impl OperationEventCursor {
    pub(crate) fn next(&mut self, coordinator: &OperationCoordinator) -> OperationView {
        let mut operation = coordinator.manager.snapshot();
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
