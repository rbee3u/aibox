//! Request write lifecycle from begin through exactly-once terminalization.

use super::layout::{
    RequestFile, ResponseFile, atomic_write_json, create_private_file, duration_ms,
    remove_controlled_request_dir, rename_noreplace, restrict_dir, safe_display_host,
    sanitize_host, utc_basic_at, validate_request_ancestor,
};
use super::reading::{error_phase, summary_ended_at, summary_to_result, terminal_summary_matches};
use super::{
    DiagnosticMetadata, ErrorMetadata, FORMAT_VERSION, FinishedRequest, NewRequest,
    ObservedRequest, Outcome, ProtocolSummary, REQUEST_BODY, REQUEST_JSON, RESPONSE_BODY,
    RESPONSE_EVENTS_JSONL, RESPONSE_JSON, RequestAssessment, RequestLocator, RequestMetadata,
    RequestStore, RequestWarningSink, ResponseMetadata, ResultMetadata, RuntimeMeasurements,
    SUMMARY_JSON, SummaryHandle, SummaryMetadata, SummaryRequestMetadata, SummaryResponseMetadata,
    TerminalRequestEvent, TimingMetadata, offset_ns, utc_now,
};
use crate::foundation::sync::{lock_unpoisoned, read_unpoisoned, write_unpoisoned};
use crate::request::assessment::refresh_assessment;
use crate::request::interpretation::coding_agent_session_id;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use uuid::Uuid;

impl RequestStore {
    #[cfg(test)]
    pub fn open(aibox_root: &Path) -> Result<Self> {
        Self::open_with_warning_sink(aibox_root, None)
    }

    pub fn open_with_warning_sink(
        aibox_root: &Path,
        warning_sink: Option<RequestWarningSink>,
    ) -> Result<Self> {
        crate::foundation::safe_fs::ensure_real_dir(aibox_root, "AIBox Root")?;
        let root = aibox_root.join("requests");
        crate::foundation::safe_fs::ensure_real_dir(&root, "Request collection")?;
        restrict_dir(&root)?;
        Ok(Self {
            root,
            active: Arc::new(Mutex::new(HashMap::new())),
            namespace: Arc::new(RwLock::new(())),
            warning_sink,
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn warning(&self, category: &str, id: Option<&str>) {
        if let Some(warning_sink) = &self.warning_sink {
            warning_sink(category, id);
        }
    }

    pub fn begin(&self, observed: ObservedRequest<'_>) -> Result<(NewRequest, RequestMetadata)> {
        let ObservedRequest {
            method,
            incoming_uri,
            upstream_url,
            http_version,
            headers,
            host_hint,
        } = observed;
        let _namespace = write_unpoisoned(&self.namespace);
        crate::foundation::safe_fs::real_dir_exists(&self.root, "Request collection")?;
        let id = Uuid::now_v7().to_string();
        let observed_at = utc_now();
        let origin = Instant::now();
        let coding_agent_session_id = coding_agent_session_id(upstream_url, &headers);
        let display_host = safe_display_host(host_hint.unwrap_or("invalid"));
        let host = sanitize_host(&display_host);
        let directory_name = format!("active-{}-{host}-{id}", utc_basic_at(&observed_at)?);
        let directory = self.root.join(directory_name);
        let locator = RequestLocator::new(directory.clone(), host, display_host);
        fs::create_dir(&directory)
            .with_context(|| format!("create Request {}", directory.display()))?;
        restrict_dir(&directory)?;
        lock_unpoisoned(&self.active).insert(id.clone(), origin);

        let created = (|| -> Result<_> {
            let request_body = create_private_file(&directory.join(REQUEST_BODY))?;
            let response_body = create_private_file(&directory.join(RESPONSE_BODY))?;
            let request = RequestMetadata {
                format_version: FORMAT_VERSION,
                id: id.clone(),
                started_at: observed_at.clone(),
                method: method.to_string(),
                incoming_uri: incoming_uri.to_string(),
                upstream_url: upstream_url.map(str::to_string),
                http_version: http_version.to_string(),
                headers,
            };
            let file = RequestFile {
                schema_version: FORMAT_VERSION,
                request_id: id.clone(),
                kind: "request".to_string(),
                method: request.method.clone(),
                upstream_url: request.upstream_url.clone(),
                headers: request.headers.clone(),
            };
            let summary = SummaryMetadata {
                schema_version: FORMAT_VERSION,
                request_id: id.clone(),
                kind: "summary".to_string(),
                observed_at,
                request: SummaryRequestMetadata {
                    method: request.method.clone(),
                    incoming_uri: request.incoming_uri.clone(),
                    upstream_url: request.upstream_url.clone(),
                    http_version: request.http_version.clone(),
                },
                response: None,
                terminal: false,
                timing: TimingMetadata::default(),
                coding_agent_session_id,
                protocol: Some(ProtocolSummary::for_url(upstream_url)),
                outcome: None,
                errors: Vec::new(),
                warnings: Vec::new(),
                assessment: RequestAssessment::active(0),
            };
            atomic_write_json(&directory, REQUEST_JSON, &file)?;
            atomic_write_json(&directory, SUMMARY_JSON, &summary)?;
            crate::foundation::safe_fs::sync_dir(&directory)?;
            crate::foundation::safe_fs::sync_dir(&self.root)?;
            Ok((
                request_body,
                response_body,
                request,
                SummaryHandle::new(summary),
            ))
        })();
        let (request_body, response_body, request, summary) = match created {
            Ok(value) => value,
            Err(error) => {
                lock_unpoisoned(&self.active).remove(&id);
                let _ = remove_controlled_request_dir(&directory);
                return Err(error);
            }
        };
        Ok((
            NewRequest {
                id,
                directory,
                locator,
                request_body,
                response_body,
                summary,
                origin,
            },
            request,
        ))
    }

    /// Atomically checkpoint a nonterminal Summary when `update` changes it.
    ///
    /// The callback is not run after terminalization; callers may therefore
    /// race optional observations with [`Self::finish`] without reopening a
    /// terminal Summary.
    pub fn update_summary(
        &self,
        locator: &RequestLocator,
        handle: &SummaryHandle,
        update: impl FnOnce(&mut SummaryMetadata) -> bool,
    ) -> Result<bool> {
        let _namespace = read_unpoisoned(&self.namespace);
        let directory = locator.path();
        validate_request_ancestor(&self.root, &directory)?;
        let mut summary = lock_unpoisoned(&handle.inner);
        if summary.terminal {
            return Ok(false);
        }
        let changed = update(&mut summary);
        if changed {
            refresh_assessment(&mut summary);
            atomic_write_json(&directory, SUMMARY_JSON, &*summary)?;
        }
        Ok(changed)
    }

    pub fn write_response(
        &self,
        locator: &RequestLocator,
        handle: &SummaryHandle,
        metadata: &ResponseMetadata,
    ) -> Result<()> {
        let _namespace = write_unpoisoned(&self.namespace);
        let directory = locator.path();
        validate_request_ancestor(&self.root, &directory)?;
        let mut summary = lock_unpoisoned(&handle.inner);
        if !summary.terminal {
            summary.response = Some(SummaryResponseMetadata {
                status: metadata.status,
                http_version: metadata.http_version.clone(),
            });
            refresh_assessment(&mut summary);
            atomic_write_json(&directory, SUMMARY_JSON, &*summary)?;
        }
        let request_id = summary.request_id.clone();
        drop(summary);
        let file = ResponseFile {
            schema_version: FORMAT_VERSION,
            request_id,
            kind: "response".to_string(),
            http_version: metadata.http_version.clone(),
            status: metadata.status,
            headers: metadata.headers.clone(),
        };
        atomic_write_json(&directory, RESPONSE_JSON, &file)
    }

    pub fn create_event_index(&self, request: &NewRequest) -> Result<fs::File> {
        let _namespace = read_unpoisoned(&self.namespace);
        let directory = request.locator.path();
        validate_request_ancestor(&self.root, &directory)?;
        create_private_file(&directory.join(RESPONSE_EVENTS_JSONL))
    }

    /// Run a path-based operation while terminal directory renames are blocked.
    ///
    /// The supplied path is valid only for the duration of `operation`; callers
    /// must not retain it after this method returns.
    pub fn with_request_path<R>(
        &self,
        locator: &RequestLocator,
        operation: impl FnOnce(&Path) -> R,
    ) -> Result<R> {
        let _namespace = read_unpoisoned(&self.namespace);
        let directory = locator.path();
        validate_request_ancestor(&self.root, &directory)?;
        Ok(operation(&directory))
    }

    /// Commit the terminal Summary and remove the Request from the active set.
    ///
    /// Repeated calls preserve the first terminal outcome. Renaming the Request
    /// directory to its end-time ordering key is best-effort and cannot undo a
    /// successfully committed terminal Summary.
    pub fn finish(
        &self,
        request: &NewRequest,
        started: Instant,
        measurements: &RuntimeMeasurements,
        outcome: Outcome,
        error: Option<ErrorMetadata>,
    ) -> Result<FinishedRequest> {
        let _namespace = write_unpoisoned(&self.namespace);
        let directory = request.locator.path();
        validate_request_ancestor(&self.root, &directory)?;
        let at_ns = offset_ns(request.origin);
        let mut summary = lock_unpoisoned(&request.summary.inner);
        if summary.terminal {
            let snapshot = summary.clone();
            drop(summary);
            let ended_at = summary_ended_at(&snapshot);
            self.finalize_directory_unlocked(request, &ended_at);
            lock_unpoisoned(&self.active).remove(&request.id);
            let mut result = summary_to_result(&snapshot);
            result.request_bytes = measurements.request_bytes;
            result.response_bytes = measurements.response_bytes;
            result.request_body_ms = measurements.request_body_duration.map(duration_ms);
            return Ok(FinishedRequest {
                result,
                terminal_event: None,
            });
        }
        let previous = summary.clone();
        summary.timing.finished_at_ns = Some(at_ns.clone());
        summary.terminal = true;
        summary.outcome = Some(outcome);
        if let Some(error) = &error {
            summary.errors.push(DiagnosticMetadata {
                phase: error_phase(error.kind).to_string(),
                kind: serde_json::to_string(&error.kind)
                    .unwrap_or_else(|_| "recording_failed".to_string())
                    .trim_matches('"')
                    .to_string(),
                message: error.message.clone(),
                at_ns: at_ns.clone(),
            });
        }
        refresh_assessment(&mut summary);
        if let Err(error) = atomic_write_json(&directory, SUMMARY_JSON, &*summary) {
            if terminal_summary_matches(&directory, &request.id, outcome, &at_ns) {
                self.warning("request summary sync failed", Some(&request.id));
            } else {
                *summary = previous;
                return Err(error);
            }
        }
        let snapshot = summary.clone();
        drop(summary);
        let ended_at = summary_ended_at(&snapshot);
        self.finalize_directory_unlocked(request, &ended_at);
        lock_unpoisoned(&self.active).remove(&request.id);
        let total_ms = snapshot
            .timing
            .finished_at_ns
            .as_deref()
            .and_then(|value| value.parse::<u128>().ok())
            .map(|ns| (ns / 1_000_000) as u64)
            .unwrap_or_else(|| duration_ms(started.elapsed()));
        let result = ResultMetadata {
            format_version: FORMAT_VERSION,
            ended_at,
            request_bytes: measurements.request_bytes,
            response_bytes: measurements.response_bytes,
            request_body_ms: measurements.request_body_duration.map(duration_ms),
            total_ms,
            outcome,
            error,
        };
        let terminal_event = TerminalRequestEvent {
            id: request.id.clone(),
            method: snapshot.request.method.clone(),
            host: request.locator.display_host.to_string(),
            outcome,
            assessment_level: snapshot.assessment.level,
            ended_at: result.ended_at.clone(),
            total_ms: result.total_ms,
            error_kind: result.error.as_ref().map(|error| error.kind),
        };
        Ok(FinishedRequest {
            result,
            terminal_event: Some(terminal_event),
        })
    }

    fn finalize_directory_unlocked(&self, request: &NewRequest, ended_at: &str) {
        let directory = request.locator.path();
        let target = match utc_basic_at(ended_at) {
            Ok(timestamp) => self.root.join(format!(
                "{timestamp}-{}-{}",
                request.locator.host, request.id
            )),
            Err(_) => {
                self.warning(
                    "request directory could not be finalized",
                    Some(&request.id),
                );
                return;
            }
        };
        if directory == target {
            return;
        }
        match rename_noreplace(&directory, &target) {
            Ok(()) => {
                request.locator.set_path(target.clone());
                if crate::foundation::safe_fs::sync_dir(&self.root).is_err() {
                    self.warning("request directory sync failed", Some(&request.id));
                }
            }
            Err(_) => self.warning("request directory rename failed", Some(&request.id)),
        }
    }

    pub fn abandon_active(&self, id: &str) {
        lock_unpoisoned(&self.active).remove(id);
    }
}
