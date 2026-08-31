//! Client request-body recording and upstream stream construction.

use super::attempt::RequestAttempt;
use super::response_stream::recording_failure;
use crate::foundation::sync::lock_unpoisoned;
use crate::request::interpretation::ProtocolObserver;
#[cfg(test)]
use crate::request::model::ProtocolSummary;
use crate::request::model::{ErrorKind, RecordedHeader, SummaryMetadata};
use crate::request::store::{
    RequestLocator, RequestStore, RuntimeMeasurements, SummaryHandle, offset_ns,
};
use axum::body::Body;
use axum::http::Response;
use bytes::Bytes;
use futures_util::StreamExt;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::io::AsyncWriteExt;

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

pub(super) async fn prepare_recorded_request_stream(
    guard: &mut RequestAttempt,
    body: Body,
    context: RequestStreamContext,
) -> Result<
    impl futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static,
    Box<Response<Body>>,
> {
    let request_file = match guard.clone_request_body() {
        Ok(file) => tokio::fs::File::from_std(file),
        Err(error) => {
            return Err(Box::new(recording_failure(
                guard,
                format!("clone request body file: {error}"),
            )));
        }
    };
    if let Err(error) = guard.mark_timing(|timing| {
        timing.upstream_request_started_at_ns = Some(guard.at_ns());
    }) {
        return Err(Box::new(recording_failure(
            guard,
            format!("checkpoint request timing: {error:#}"),
        )));
    }
    if context.expected_body_bytes == Some(0) {
        if let Err(error) = request_file.sync_all().await {
            return Err(Box::new(recording_failure(
                guard,
                format!("sync request body: {error}"),
            )));
        }
        guard.mark_request_body_finished();
        if let Err(error) = checkpoint_request_complete(
            &context.request,
            &context.summary,
            &context.protocol,
            &context.request_headers,
            offset_ns(context.origin),
        ) {
            return Err(Box::new(recording_failure(
                guard,
                format!("checkpoint request completion: {error:#}"),
            )));
        }
    }
    Ok(recorded_request_stream_with_summary(
        body,
        request_file,
        context,
    ))
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

pub(super) fn recorded_request_stream_with_summary(
    body: Body,
    mut file: tokio::fs::File,
    context: RequestStreamContext,
) -> impl futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let mut stream = body.into_data_stream();
    async_stream::stream! {
        let mut reached_eof = false;
        let mut completed_by_length = context.expected_body_bytes == Some(0);
        loop {
            let next = tokio::select! {
                () = context.shutdown.cancelled() => {
                    let error = io::Error::new(io::ErrorKind::Interrupted, "Request Proxy is shutting down");
                    request_failure(&context.error_slot, ErrorKind::ServerShutdown, &error);
                    yield Err(error);
                    break;
                }
                next = stream.next() => next,
            };
            let Some(chunk) = next else {
                reached_eof = true;
                break;
            };
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    let error = io::Error::new(io::ErrorKind::UnexpectedEof, error.to_string());
                    request_failure(&context.error_slot, ErrorKind::RequestBodyFailed, &error);
                    yield Err(error);
                    break;
                }
            };
            if let Err(error) = file.write_all(&chunk).await {
                request_failure(&context.error_slot, ErrorKind::RequestRecordingFailed, &error);
                yield Err(error);
                break;
            }
            if let Err(error) = file.flush().await {
                request_failure(&context.error_slot, ErrorKind::RequestRecordingFailed, &error);
                yield Err(error);
                break;
            }
            let request_bytes = {
                let mut values = lock_unpoisoned(&context.measurements);
                values.request_bytes = values.request_bytes.saturating_add(chunk.len() as u64);
                values.request_bytes
            };
            let at_ns = offset_ns(context.origin);
            let first_request_byte = checkpoint_summary_update(
                &context.request,
                &context.summary,
                |value| {
                    if value.timing.upstream_request_body_first_byte_at_ns.is_some() {
                        return false;
                    }
                    value.timing.upstream_request_body_first_byte_at_ns = Some(at_ns);
                    true
                },
            );
            if let Err(error) = first_request_byte {
                request_failure(&context.error_slot, ErrorKind::RequestRecordingFailed, &error);
                yield Err(io::Error::other(error.to_string()));
                break;
            }
            if context.expected_body_bytes == Some(request_bytes) {
                if let Err(error) = complete_recorded_request(&mut file, &context).await {
                    request_failure(&context.error_slot, ErrorKind::RequestRecordingFailed, &error);
                    yield Err(error);
                    break;
                }
                completed_by_length = true;
            }
            yield Ok(chunk);
            if completed_by_length {
                break;
            }
        }
        if reached_eof
            && !completed_by_length
            && let Err(error) = complete_recorded_request(&mut file, &context).await
        {
            request_failure(&context.error_slot, ErrorKind::RequestRecordingFailed, &error);
            yield Err(error);
        }
    }
}

pub(super) async fn complete_recorded_request(
    file: &mut tokio::fs::File,
    context: &RequestStreamContext,
) -> io::Result<()> {
    file.sync_all().await?;
    lock_unpoisoned(&context.measurements).request_body_duration = Some(context.origin.elapsed());
    checkpoint_request_complete(
        &context.request,
        &context.summary,
        &context.protocol,
        &context.request_headers,
        offset_ns(context.origin),
    )
    .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(())
}

pub(super) fn checkpoint_request_complete(
    captured_request: &RequestTarget,
    summary: &SummaryHandle,
    protocol: &Mutex<ProtocolObserver>,
    request_headers: &[RecordedHeader],
    at_ns: String,
) -> anyhow::Result<bool> {
    if summary.read(|summary| {
        summary.terminal
            || summary
                .timing
                .upstream_request_body_completed_at_ns
                .is_some()
    }) {
        return Ok(false);
    }
    let mut observer = lock_unpoisoned(protocol);
    let _ = captured_request.with_request_path(|directory| {
        observer.observe_request(
            &directory.join("request.body"),
            request_headers,
            at_ns.clone(),
        )
    })?;
    let protocol_snapshot = observer.snapshot();
    checkpoint_summary_update(captured_request, summary, |value| {
        value.timing.upstream_request_body_completed_at_ns = Some(at_ns);
        value.protocol = Some(protocol_snapshot);
        true
    })
}

// Kept as a small test helper for deterministic body-stream tests that do not
// have a Request directory to checkpoint.
#[cfg(test)]
pub(super) fn recorded_request_stream(
    body: Body,
    file: tokio::fs::File,
    measurements: Arc<Mutex<RuntimeMeasurements>>,
    error_slot: Arc<Mutex<Option<RequestStreamFailure>>>,
    started: Instant,
    shutdown: tokio_util::sync::CancellationToken,
) -> impl futures_util::Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    let summary = SummaryHandle::new(SummaryMetadata::test(
        String::new(),
        Some(ProtocolSummary::default()),
    ));
    recorded_request_stream_with_summary(
        body,
        file,
        RequestStreamContext {
            measurements,
            error_slot,
            summary,
            protocol: Arc::new(Mutex::new(ProtocolObserver::new(None))),
            request_headers: Vec::new(),
            expected_body_bytes: None,
            request: RequestTarget::Unstored {
                directory: std::path::PathBuf::new(),
            },
            origin: started,
            shutdown,
        },
    )
}

pub(super) fn checkpoint_summary_update(
    captured_request: &RequestTarget,
    summary: &SummaryHandle,
    update: impl FnOnce(&mut SummaryMetadata) -> bool,
) -> anyhow::Result<bool> {
    captured_request.update_summary(summary, update)
}
