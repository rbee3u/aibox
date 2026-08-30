//! One Request attempt's terminal state, protocol observation, and reporting.

use super::request_stream::{RequestStreamContext, RequestStreamFailure, RequestTarget};
use crate::foundation::sync::lock_unpoisoned;
use crate::request::interpretation::ProtocolObserver;
use crate::request::model::{
    DiagnosticMetadata, ErrorKind, ErrorMetadata, Outcome, ProtocolSummary, RecordedHeader,
    TimingMetadata,
};
use crate::request::reporter::RequestReporter;
use crate::request::response_observation::replay_complete_zstd_sse;
use crate::request::sse::ObservedSseEvent;
use crate::request::store::{NewRequest, RequestStore, RuntimeMeasurements, offset_ns};
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
pub(super) struct RequestTerminal {
    pub(super) outcome: Outcome,
    pub(super) error: Option<ErrorMetadata>,
}

pub(super) enum RequestAttemptState {
    Active,
    Finalizing(RequestTerminal),
    Finished,
}

pub(super) struct RequestAttempt {
    store: RequestStore,
    request: NewRequest,
    measurements: Arc<Mutex<RuntimeMeasurements>>,
    protocol: Arc<Mutex<ProtocolObserver>>,
    /// Set by the request body stream on failure. The response task consults
    /// it because reqwest surfaces a request-side abort as a response stream
    /// error once the response headers have already arrived.
    request_error: Arc<Mutex<Option<RequestStreamFailure>>>,
    state: RequestAttemptState,
    reporter: Option<RequestReporter>,
}

impl RequestAttempt {
    pub(super) fn new(
        store: RequestStore,
        request: NewRequest,
        measurements: Arc<Mutex<RuntimeMeasurements>>,
        protocol: Arc<Mutex<ProtocolObserver>>,
    ) -> Self {
        Self {
            store,
            request,
            measurements,
            protocol,
            request_error: Arc::new(Mutex::new(None)),
            state: RequestAttemptState::Active,
            reporter: None,
        }
    }

    pub(super) fn with_reporter(mut self, reporter: Option<RequestReporter>) -> Self {
        self.reporter = reporter;
        self
    }

    pub(super) fn request_stream_context(
        &self,
        request_headers: Vec<RecordedHeader>,
        expected_body_bytes: Option<u64>,
        shutdown: tokio_util::sync::CancellationToken,
    ) -> RequestStreamContext {
        RequestStreamContext {
            measurements: self.measurements.clone(),
            error_slot: self.request_error.clone(),
            summary: self.request.summary.clone(),
            protocol: self.protocol.clone(),
            request_headers,
            expected_body_bytes,
            request: RequestTarget::Stored {
                store: self.store.clone(),
                locator: self.request.locator.clone(),
            },
            origin: self.request.origin,
            shutdown,
        }
    }

    pub(super) fn clone_request_body(&self) -> std::io::Result<File> {
        self.request.request_body.try_clone()
    }

    pub(super) fn clone_response_body(&self) -> std::io::Result<File> {
        self.request.response_body.try_clone()
    }

    pub(super) fn write_response(
        &self,
        response: &crate::request::model::ResponseMetadata,
    ) -> anyhow::Result<()> {
        self.store
            .write_response(&self.request.locator, &self.request.summary, response)
    }

    pub(super) fn create_event_index(&self) -> anyhow::Result<File> {
        self.store.create_event_index(&self.request)
    }

    pub(super) fn with_request_path<R>(
        &self,
        operation: impl FnOnce(&Path) -> R,
    ) -> anyhow::Result<R> {
        self.store
            .with_request_path(&self.request.locator, operation)
    }

    pub(super) fn request_id(&self) -> &str {
        &self.request.id
    }

    pub(super) fn at_ns(&self) -> String {
        offset_ns(self.request.origin)
    }

    pub(super) fn elapsed(&self) -> Duration {
        self.request.origin.elapsed()
    }

    pub(super) fn response_first_byte_unseen(&self) -> bool {
        self.request.summary.read(|summary| {
            summary
                .timing
                .upstream_response_body_first_byte_at_ns
                .is_none()
        })
    }

    pub(super) fn add_response_bytes(&self, count: usize) {
        let mut values = lock_unpoisoned(&self.measurements);
        values.response_bytes = values.response_bytes.saturating_add(count as u64);
    }

    pub(super) fn add_request_bytes(&self, count: usize) {
        let mut values = lock_unpoisoned(&self.measurements);
        values.request_bytes = values.request_bytes.saturating_add(count as u64);
    }

    pub(super) fn mark_request_body_finished(&self) {
        lock_unpoisoned(&self.measurements).request_body_duration = Some(self.elapsed());
    }

    pub(super) fn request_stream_failure(&self) -> Option<RequestStreamFailure> {
        lock_unpoisoned(&self.request_error).clone()
    }

    pub(super) fn warn_finalization_failed(&self) {
        self.store
            .warning("request finalization failed", Some(self.request_id()));
    }

    #[cfg(test)]
    pub(super) fn summary_handle(&self) -> crate::request::store::SummaryHandle {
        self.request.summary.clone()
    }

    pub(super) fn mark_timing(
        &self,
        update: impl FnOnce(&mut TimingMetadata),
    ) -> anyhow::Result<()> {
        self.store
            .update_summary(&self.request.locator, &self.request.summary, |summary| {
                update(&mut summary.timing);
                true
            })?;
        Ok(())
    }

    pub(super) fn observe_response_headers(
        &self,
        headers: &[RecordedHeader],
        event_stream: Option<bool>,
    ) -> anyhow::Result<()> {
        let at_ns = offset_ns(self.request.origin);
        let mut observer = lock_unpoisoned(&self.protocol);
        observer.observe_response_headers(headers, event_stream, at_ns.clone());
        let protocol = observer.snapshot();
        self.store
            .update_summary(&self.request.locator, &self.request.summary, |summary| {
                summary.timing.upstream_response_headers_at_ns = Some(at_ns);
                summary.protocol = Some(protocol);
                true
            })?;
        Ok(())
    }

    pub(super) fn observe_response_mode(&self, event_stream: bool) -> anyhow::Result<()> {
        let mut observer = lock_unpoisoned(&self.protocol);
        if observer.observe_response_mode(event_stream, offset_ns(self.request.origin)) {
            self.publish_protocol(observer.snapshot())?;
        }
        Ok(())
    }

    pub(super) fn protocol_summary(&self) -> ProtocolSummary {
        lock_unpoisoned(&self.protocol).snapshot()
    }

    pub(super) fn observe_sse_events(&self, events: &[ObservedSseEvent]) -> anyhow::Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let mut observer = lock_unpoisoned(&self.protocol);
        let changed = events
            .iter()
            .fold(false, |changed, (event_name, data, at_ns)| {
                observer.observe_sse_event(event_name.as_deref(), data, at_ns.clone()) | changed
            });
        if changed {
            self.publish_protocol(observer.snapshot())?;
        }
        Ok(())
    }

    pub(super) fn observe_first_token(&self, at_ns: String) -> anyhow::Result<()> {
        let mut observer = lock_unpoisoned(&self.protocol);
        if observer.observe_first_token(at_ns) {
            self.publish_protocol(observer.snapshot())?;
        }
        Ok(())
    }

    pub(super) fn observe_json_response(
        &self,
        status: u16,
        headers: &[RecordedHeader],
    ) -> anyhow::Result<()> {
        let at_ns = offset_ns(self.request.origin);
        let mut observer = lock_unpoisoned(&self.protocol);
        self.store
            .with_request_path(&self.request.locator, |directory| {
                observer.observe_json_response(
                    &directory.join("response.body"),
                    status,
                    headers,
                    at_ns,
                )
            })?;
        self.publish_protocol(observer.snapshot())
    }

    pub(super) fn observe_encoded_sse_response(&self) -> anyhow::Result<()> {
        let at_ns = offset_ns(self.request.origin);
        let opened = self
            .store
            .with_request_path(&self.request.locator, |directory| {
                crate::foundation::safe_fs::open_real_file(
                    &directory.join("response.body"),
                    "Upstream Response body",
                )
            })?;
        let file = match opened {
            Ok(file) => file,
            Err(error) => {
                self.add_warning("response_interpretation_failed", error.to_string());
                return Ok(());
            }
        };
        let observation = match replay_complete_zstd_sse(file, self.request.id.clone(), &at_ns) {
            Ok(observation) => observation,
            Err(error) => {
                self.add_warning("response_interpretation_failed", error.to_string());
                return Ok(());
            }
        };
        if let Some(warning) = observation.warning {
            self.add_warning("response_interpretation_failed", warning);
        }
        self.observe_sse_events(&observation.events)
    }

    pub(super) fn publish_protocol(&self, protocol: ProtocolSummary) -> anyhow::Result<()> {
        self.store
            .update_summary(&self.request.locator, &self.request.summary, |summary| {
                if summary.protocol.as_ref() == Some(&protocol) {
                    return false;
                }
                summary.protocol = Some(protocol);
                true
            })?;
        Ok(())
    }

    pub(super) fn add_warning(&self, kind: &str, message: String) {
        let result =
            self.store
                .update_summary(&self.request.locator, &self.request.summary, |summary| {
                    summary.warnings.push(DiagnosticMetadata {
                        phase: "recording".to_string(),
                        kind: kind.to_string(),
                        message,
                        at_ns: offset_ns(self.request.origin),
                    });
                    true
                });
        if result.is_err() {
            self.store
                .warning("request summary checkpoint failed", Some(&self.request.id));
        }
    }

    pub(super) fn finish(
        &mut self,
        outcome: Outcome,
        error: Option<ErrorMetadata>,
    ) -> anyhow::Result<()> {
        self.finish_terminal(RequestTerminal { outcome, error })
    }

    pub(super) fn finish_terminal(&mut self, proposed: RequestTerminal) -> anyhow::Result<()> {
        let terminal = match &self.state {
            RequestAttemptState::Active => {
                self.state = RequestAttemptState::Finalizing(proposed.clone());
                proposed
            }
            RequestAttemptState::Finalizing(terminal) => terminal.clone(),
            RequestAttemptState::Finished => return Ok(()),
        };
        let values = lock_unpoisoned(&self.measurements).clone();
        self.request.request_body.sync_all().ok();
        self.request.response_body.sync_all().ok();
        let finished = self.store.finish(
            &self.request,
            self.request.origin,
            &values,
            terminal.outcome,
            terminal.error,
        );
        if let Ok(finished) = &finished {
            if let (Some(reporter), Some(event)) = (&self.reporter, &finished.terminal_event) {
                reporter.request_finished(event);
            }
            self.state = RequestAttemptState::Finished;
        }
        finished.map(|_| ())
    }
}

impl Drop for RequestAttempt {
    fn drop(&mut self) {
        let terminal = match &self.state {
            RequestAttemptState::Finished => return,
            RequestAttemptState::Finalizing(terminal) => terminal.clone(),
            RequestAttemptState::Active => RequestTerminal {
                outcome: Outcome::ClientDisconnected,
                error: Some(ErrorMetadata {
                    kind: ErrorKind::ClientDisconnected,
                    message: "client disconnected before the proxy attempt completed".to_string(),
                }),
            },
        };
        let _ = self.finish_terminal(terminal);
        self.store.abandon_active(&self.request.id);
    }
}
