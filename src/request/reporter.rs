//! Safe, serialized terminal presentation for the foreground Request Proxy.

use crate::request::model::{AssessmentLevel, ErrorKind, Outcome, TerminalRequestEvent, utc_now};
use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(crate) struct RequestReporter {
    state: Arc<Mutex<ReporterState>>,
    output: Arc<Mutex<()>>,
}

struct ReporterState {
    tty: bool,
    warned: HashSet<String>,
}

impl RequestReporter {
    pub(crate) fn new() -> Self {
        Self::with_tty(io::stderr().is_terminal())
    }

    fn with_tty(tty: bool) -> Self {
        Self {
            state: Arc::new(Mutex::new(ReporterState {
                tty,
                warned: HashSet::new(),
            })),
            output: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) fn request_finished(&self, event: &TerminalRequestEvent) {
        if !should_report_request(event.outcome, event.assessment_level) {
            return;
        }
        let reason = event
            .error_kind
            .map(error_reason)
            .unwrap_or_else(|| outcome_reason(event.outcome));
        self.event_at(
            "error",
            &format!(
                "#{} {} {} {}: {reason} ({} ms)",
                short_id(&event.id),
                event.method,
                event.host,
                event.outcome.as_str(),
                event.total_ms,
            ),
            Some(&event.ended_at),
        );
    }

    pub(crate) fn warning(&self, category: &str, id: Option<&str>) {
        let key = format!("{category}:{}", id.unwrap_or(""));
        let should_write = self.with_state_mut(|state| state.warned.insert(key));
        if !should_write {
            return;
        }
        let suffix = id
            .map(|id| format!(" #{}", short_id(id)))
            .unwrap_or_default();
        self.event("warn", &format!("{category}{suffix}"));
    }

    fn event(&self, level: &str, message: &str) {
        self.event_at(level, message, None);
    }

    fn event_at(&self, level: &str, message: &str, timestamp: Option<&str>) {
        let tty = self.with_state(|state| state.tty);
        let timestamp = timestamp.map(str::to_owned).unwrap_or_else(utc_now);
        let line = render_event(tty, &timestamp, level, message);
        self.write(&line);
    }

    fn write(&self, text: &str) {
        let _output = self
            .output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut stderr = io::stderr().lock();
        let _ = stderr.write_all(text.as_bytes());
        let _ = stderr.write_all(b"\n");
        let _ = stderr.flush();
    }

    fn with_state<R>(&self, read: impl FnOnce(&ReporterState) -> R) -> R {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        read(&state)
    }

    fn with_state_mut<R>(&self, update: impl FnOnce(&mut ReporterState) -> R) -> R {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        update(&mut state)
    }
}

fn should_report_request(outcome: Outcome, assessment_level: AssessmentLevel) -> bool {
    assessment_level == AssessmentLevel::Error
        && !matches!(outcome, Outcome::Completed | Outcome::ServerShutdown)
}

fn render_event(tty: bool, timestamp: &str, level: &str, message: &str) -> String {
    let timestamp = format_report_timestamp(timestamp);
    if !tty {
        return format!("{timestamp} [{level}] {message}");
    }
    let color = match level {
        "error" => "\x1b[1;31m",
        "warn" => "\x1b[1;33m",
        "stop" => "\x1b[1;35m",
        _ => "\x1b[0m",
    };
    format!("{timestamp} {color}[{level}]\x1b[0m {message}")
}

fn format_report_timestamp(timestamp: &str) -> String {
    let rfc3339 = time::format_description::well_known::Rfc3339;
    let Ok(observed) = time::OffsetDateTime::parse(timestamp, &rfc3339) else {
        return timestamp.to_string();
    };
    let milliseconds = time::format_description::parse_borrowed::<1>(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z",
    )
    .expect("static Request reporter timestamp format is valid");
    observed
        .to_offset(time::UtcOffset::UTC)
        .format(&milliseconds)
        .unwrap_or_else(|_| timestamp.to_string())
}

fn short_id(id: &str) -> &str {
    let start = id.len().saturating_sub(12);
    &id[start..]
}

fn outcome_reason(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Rejected => "request rejected",
        Outcome::UpstreamError => "upstream request failed",
        Outcome::ClientDisconnected => "client disconnected",
        Outcome::RecordingFailed => "request could not be finalized",
        Outcome::ServerShutdown => "server shutdown",
        Outcome::Completed => "completed",
    }
}

fn error_reason(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::ClientConfiguration => "client configuration failed",
        ErrorKind::ClientDisconnected => "client disconnected",
        ErrorKind::ConnectNotSupported => "CONNECT is not supported",
        ErrorKind::ConnectTimeout => "upstream connection timed out",
        ErrorKind::DnsError => "upstream name could not be resolved",
        ErrorKind::EventIndexFailed => "event index unavailable",
        ErrorKind::InvalidTargetUrl => "target URL is invalid",
        ErrorKind::NonPublicTarget => "target is not publicly reachable",
        ErrorKind::RecordingFailed
        | ErrorKind::RequestRecordingFailed
        | ErrorKind::ResponseRecordingFailed => "request could not be written",
        ErrorKind::RequestBodyFailed => "request body failed",
        ErrorKind::ServerShutdown => "server shutdown",
        ErrorKind::UpgradeNotSupported => "protocol upgrade is not supported",
        ErrorKind::UpstreamRequestFailed => "upstream request failed",
        ErrorKind::UpstreamResponseFailed => "upstream response failed",
    }
}

#[cfg(test)]
#[path = "reporter_tests.rs"]
mod tests;
