use super::*;

#[test]
fn short_id_uses_the_last_twelve_characters() {
    assert_eq!(short_id("0198-demo-abcdef123456"), "abcdef123456");
    assert_eq!(short_id("short"), "short");
}

#[test]
fn request_events_report_only_error_assessed_abnormal_outcomes() {
    assert!(!should_report_request(
        Outcome::ClientDisconnected,
        AssessmentLevel::Ok
    ));
    assert!(!should_report_request(
        Outcome::ClientDisconnected,
        AssessmentLevel::Warning
    ));
    assert!(should_report_request(
        Outcome::ClientDisconnected,
        AssessmentLevel::Error
    ));
    assert!(!should_report_request(
        Outcome::Completed,
        AssessmentLevel::Error
    ));
    assert!(!should_report_request(
        Outcome::ServerShutdown,
        AssessmentLevel::Error
    ));
}

#[test]
fn report_timestamp_is_utc_with_fixed_truncated_milliseconds() {
    assert_eq!(
        format_report_timestamp("2026-08-13T13:08:24.976874375Z"),
        "2026-08-13T13:08:24.976Z"
    );
    assert_eq!(
        format_report_timestamp("2026-08-13T13:08:24Z"),
        "2026-08-13T13:08:24.000Z"
    );
    assert_eq!(
        format_report_timestamp("2026-08-13T13:08:24.1Z"),
        "2026-08-13T13:08:24.100Z"
    );
    assert_eq!(
        format_report_timestamp("2026-08-13T13:08:24.999999999Z"),
        "2026-08-13T13:08:24.999Z"
    );
    assert_eq!(
        format_report_timestamp("2026-08-13T15:08:24.976874375+02:00"),
        "2026-08-13T13:08:24.976Z"
    );
}

#[test]
fn report_timestamp_preserves_invalid_input() {
    assert_eq!(
        format_report_timestamp("not-a-timestamp"),
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
