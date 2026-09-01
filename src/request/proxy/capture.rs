//! Capture context shared by a Request attempt and its request body stream.

use crate::foundation::sync::lock_unpoisoned;
use crate::request::interpretation::ProtocolObserver;
use crate::request::model::{ErrorKind, RecordedHeader, SummaryMetadata};
use crate::request::store::{RequestLocator, RequestStore, RuntimeMeasurements, SummaryHandle};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub(super) enum RequestTarget {
    Stored {
        store: RequestStore,
        locator: RequestLocator,
    },
    #[cfg(test)]
    Unstored { directory: std::path::PathBuf },
}

impl RequestTarget {
    pub(super) fn with_request_path<R>(
        &self,
        operation: impl FnOnce(&std::path::Path) -> R,
    ) -> anyhow::Result<R> {
        match self {
            Self::Stored { store, locator } => store.with_request_path(locator, operation),
            #[cfg(test)]
            Self::Unstored { directory } => Ok(operation(directory)),
        }
    }

    pub(super) fn update_summary(
        &self,
        summary: &SummaryHandle,
        update: impl FnOnce(&mut SummaryMetadata) -> bool,
    ) -> anyhow::Result<bool> {
        match self {
            Self::Stored { store, locator } => store.update_summary(locator, summary, update),
            #[cfg(test)]
            Self::Unstored { .. } => Ok(summary.update(update)),
        }
    }
}

pub(super) struct RequestStreamContext {
    pub(super) measurements: Arc<Mutex<RuntimeMeasurements>>,
    pub(super) error_slot: Arc<Mutex<Option<RequestStreamFailure>>>,
    pub(super) summary: SummaryHandle,
    pub(super) protocol: Arc<Mutex<ProtocolObserver>>,
    pub(super) request_headers: Vec<RecordedHeader>,
    pub(super) expected_body_bytes: Option<u64>,
    pub(super) request: RequestTarget,
    pub(super) origin: Instant,
    pub(super) shutdown: tokio_util::sync::CancellationToken,
}

#[derive(Clone, Debug)]
pub(super) struct RequestStreamFailure {
    pub(super) kind: ErrorKind,
    pub(super) message: String,
}

pub(super) fn request_failure(
    slot: &Mutex<Option<RequestStreamFailure>>,
    kind: ErrorKind,
    error: &impl ToString,
) {
    *lock_unpoisoned(slot) = Some(RequestStreamFailure {
        kind,
        message: error.to_string(),
    });
}
