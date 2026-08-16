//! Safe, serialized terminal presentation for the foreground Traffic Proxy.

use crate::traffic_store::utc_now;
use crate::traffic_store::{AssessmentLevel, ErrorKind, Outcome};
use std::collections::HashSet;
use std::io::{self, IsTerminal, Write};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(crate) struct TrafficConsole {
    state: Arc<Mutex<ConsoleState>>,
    output: Arc<Mutex<()>>,
}

struct ConsoleState {
    tty: bool,
    warned: HashSet<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ShutdownReason {
    Interrupt,
    Terminate,
}

pub(crate) struct AbnormalRecordEvent<'a> {
    pub(crate) id: &'a str,
    pub(crate) method: &'a str,
    pub(crate) host: &'a str,
    pub(crate) outcome: Outcome,
    pub(crate) assessment_level: AssessmentLevel,
    pub(crate) ended_at: &'a str,
    pub(crate) total_ms: u64,
    pub(crate) error_kind: Option<ErrorKind>,
}

impl ShutdownReason {
    pub(crate) fn completion_exit_code(self) -> i32 {
        match self {
            Self::Interrupt => 0,
            Self::Terminate => 143,
        }
    }

    pub(crate) fn forced_exit_code(self) -> i32 {
        match self {
            Self::Interrupt => 130,
            Self::Terminate => 143,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Interrupt => "Ctrl-C",
            Self::Terminate => "SIGTERM",
        }
    }
}

impl TrafficConsole {
    pub(crate) fn new() -> Self {
        Self::with_tty(io::stderr().is_terminal())
    }

    fn with_tty(tty: bool) -> Self {
        Self {
            state: Arc::new(Mutex::new(ConsoleState {
                tty,
                warned: HashSet::new(),
            })),
            output: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) fn startup(&self, listen: &str, traffic_viewer: &str) {
        self.write(&render_startup(listen, traffic_viewer));
    }

    pub(crate) fn begin_shutdown(&self, reason: ShutdownReason, active: usize) {
        let message = format!(
            "Shutting down after {}; finalizing {active} active Traffic Record{}...",
            reason.label(),
            if active == 1 { "" } else { "s" }
        );
        if reason == ShutdownReason::Interrupt {
            self.event_after_terminal_interrupt("stop", &message);
        } else {
            self.event("stop", &message);
        }
    }

    pub(crate) fn stopped(&self, code: i32) {
        self.event("stop", &format!("Stopped (exit {code})"));
    }

    pub(crate) fn forced_shutdown(&self, reason: ShutdownReason) {
        self.event(
            "stop",
            &format!("Forced shutdown after second {}", reason.label()),
        );
    }

    pub(crate) fn record_finished(&self, event: AbnormalRecordEvent<'_>) {
        if !should_report_record(event.outcome, event.assessment_level) {
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
                short_id(event.id),
                event.method,
                event.host,
                event.outcome.as_str(),
                event.total_ms,
            ),
            Some(event.ended_at),
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

    fn event_after_terminal_interrupt(&self, level: &str, message: &str) {
        let tty = self.with_state(|state| state.tty);
        let timestamp = utc_now();
        let line = render_event_after_terminal_interrupt(tty, &timestamp, level, message);
        self.write(&line);
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

    fn with_state<R>(&self, read: impl FnOnce(&ConsoleState) -> R) -> R {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        read(&state)
    }

    fn with_state_mut<R>(&self, update: impl FnOnce(&mut ConsoleState) -> R) -> R {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        update(&mut state)
    }
}

fn render_startup(listen: &str, traffic_viewer: &str) -> String {
    format!("Running · Listen: {listen} · Viewer: {traffic_viewer}")
}

fn should_report_record(outcome: Outcome, assessment_level: AssessmentLevel) -> bool {
    assessment_level == AssessmentLevel::Error
        && !matches!(outcome, Outcome::Completed | Outcome::ServerShutdown)
}

fn render_event(tty: bool, timestamp: &str, level: &str, message: &str) -> String {
    let timestamp = format_console_timestamp(timestamp);
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

fn render_event_after_terminal_interrupt(
    tty: bool,
    timestamp: &str,
    level: &str,
    message: &str,
) -> String {
    let event = render_event(tty, timestamp, level, message);
    if tty { format!("\n{event}") } else { event }
}

fn format_console_timestamp(timestamp: &str) -> String {
    let rfc3339 = time::format_description::well_known::Rfc3339;
    let Ok(observed) = time::OffsetDateTime::parse(timestamp, &rfc3339) else {
        return timestamp.to_string();
    };
    let milliseconds = time::format_description::parse_borrowed::<1>(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z",
    )
    .expect("static Traffic Console timestamp format is valid");
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
        Outcome::RecordingFailed => "traffic record could not be finalized",
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
        | ErrorKind::ResponseRecordingFailed => "traffic record could not be written",
        ErrorKind::RequestBodyFailed => "request body failed",
        ErrorKind::ServerShutdown => "server shutdown",
        ErrorKind::UpgradeNotSupported => "protocol upgrade is not supported",
        ErrorKind::UpstreamRequestFailed => "upstream request failed",
        ErrorKind::UpstreamResponseFailed => "upstream response failed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_id_uses_the_last_twelve_characters() {
        assert_eq!(short_id("0198-demo-abcdef123456"), "abcdef123456");
        assert_eq!(short_id("short"), "short");
    }

    #[test]
    fn completed_ctrl_c_is_success_but_forced_signals_use_unix_exit_codes() {
        assert_eq!(ShutdownReason::Interrupt.completion_exit_code(), 0);
        assert_eq!(ShutdownReason::Terminate.completion_exit_code(), 143);
        assert_eq!(ShutdownReason::Interrupt.forced_exit_code(), 130);
        assert_eq!(ShutdownReason::Terminate.forced_exit_code(), 143);
    }

    #[test]
    fn startup_is_one_stable_plain_text_line() {
        let output = render_startup("127.0.0.1:9923", "http://127.0.0.1:9923/");
        assert_eq!(
            output,
            "Running · Listen: 127.0.0.1:9923 · Viewer: http://127.0.0.1:9923/"
        );
        assert!(!output.contains('\x1b'));
        assert_eq!(output.lines().count(), 1);
        assert!(!output.contains("Records"));
        assert!(!output.contains("Ctrl-C"));
    }

    #[test]
    fn record_events_report_only_error_assessed_abnormal_outcomes() {
        assert!(!should_report_record(
            Outcome::ClientDisconnected,
            AssessmentLevel::Ok
        ));
        assert!(!should_report_record(
            Outcome::ClientDisconnected,
            AssessmentLevel::Warning
        ));
        assert!(should_report_record(
            Outcome::ClientDisconnected,
            AssessmentLevel::Error
        ));
        assert!(!should_report_record(
            Outcome::Completed,
            AssessmentLevel::Error
        ));
        assert!(!should_report_record(
            Outcome::ServerShutdown,
            AssessmentLevel::Error
        ));
    }

    #[test]
    fn console_timestamp_is_utc_with_fixed_truncated_milliseconds() {
        assert_eq!(
            format_console_timestamp("2026-08-13T13:08:24.976874375Z"),
            "2026-08-13T13:08:24.976Z"
        );
        assert_eq!(
            format_console_timestamp("2026-08-13T13:08:24Z"),
            "2026-08-13T13:08:24.000Z"
        );
        assert_eq!(
            format_console_timestamp("2026-08-13T13:08:24.1Z"),
            "2026-08-13T13:08:24.100Z"
        );
        assert_eq!(
            format_console_timestamp("2026-08-13T13:08:24.999999999Z"),
            "2026-08-13T13:08:24.999Z"
        );
        assert_eq!(
            format_console_timestamp("2026-08-13T15:08:24.976874375+02:00"),
            "2026-08-13T13:08:24.976Z"
        );
    }

    #[test]
    fn console_timestamp_preserves_invalid_input() {
        assert_eq!(
            format_console_timestamp("not-a-timestamp"),
            "not-a-timestamp"
        );
    }

    #[test]
    fn event_rendering_uses_milliseconds_and_limits_color_to_tty() {
        let timestamp = "2026-08-13T14:32:08.976874375Z";
        let rendered_timestamp = "2026-08-13T14:32:08.976Z";
        assert_eq!(
            render_event(false, timestamp, "error", "safe reason"),
            "2026-08-13T14:32:08.976Z [error] safe reason"
        );
        let tty = render_event(true, timestamp, "error", "safe reason");
        assert!(tty.starts_with(rendered_timestamp));
        assert!(tty.contains("\x1b[1;31m[error]\x1b[0m"));
    }

    #[test]
    fn terminal_interrupt_event_starts_on_a_new_line_only_for_a_tty() {
        let timestamp = "2026-08-13T14:32:08.976Z";
        let tty = render_event_after_terminal_interrupt(true, timestamp, "stop", "stopping");
        assert!(tty.starts_with('\n'));
        assert!(tty.contains("2026-08-13T14:32:08.976Z"));

        let redirected =
            render_event_after_terminal_interrupt(false, timestamp, "stop", "stopping");
        assert_eq!(redirected, "2026-08-13T14:32:08.976Z [stop] stopping");
    }

    #[test]
    fn fixed_error_reasons_never_echo_third_party_text() {
        for kind in [
            ErrorKind::DnsError,
            ErrorKind::UpstreamRequestFailed,
            ErrorKind::ResponseRecordingFailed,
        ] {
            let reason = error_reason(kind);
            assert!(!reason.contains("http://"));
            assert!(!reason.contains('\n'));
            assert!(!reason.contains("token"));
        }
    }
}
