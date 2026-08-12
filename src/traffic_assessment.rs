//! Classifying one Traffic Record into its display Record Assessment.
//!
//! A Record Assessment is a presentation label derived from independent evidence
//! — Traffic Outcome, HTTP status, Provider Error, and protocol diagnostics — not
//! a replacement for it. Active takes temporary visual precedence, every finding
//! stays separately available for Diagnostics, and one prioritized primary
//! finding supplies the compact label.
//!
//! [`refresh_assessment`] materializes the value into the Summary on the write
//! path so lists never recompute it, while [`effective_assessment`] re-derives
//! the interrupted case at read time. See
//! `docs/adr/0011-materialize-traffic-summary-assessment.md`.

use crate::traffic_interpretation::{ProtocolFamily, ResponseModeValue};
use crate::traffic_store::{
    AssessmentFinding, AssessmentLevel, AssessmentPrimary, AssessmentSource, Outcome,
    RecordAssessment, SummaryMetadata,
};

pub(crate) fn effective_assessment(summary: &SummaryMetadata, active: bool) -> RecordAssessment {
    calculate_assessment(summary, active, !summary.terminal && !active)
}

pub(crate) fn diagnostic_findings(
    summary: &SummaryMetadata,
    interrupted: bool,
) -> Vec<AssessmentFinding> {
    let mut findings = Vec::new();
    collect_traffic_findings(summary, &mut findings);
    collect_http_findings(summary, &mut findings);
    collect_protocol_findings(summary, &mut findings);
    collect_diagnostic_warnings(summary, &mut findings);

    if interrupted {
        push_finding(
            &mut findings,
            AssessmentFinding {
                level: AssessmentLevel::Warning,
                source: AssessmentSource::Traffic,
                kind: "interrupted".to_string(),
                message: "Traffic Proxy stopped before the Traffic Record was finalized"
                    .to_string(),
                phase: None,
                at_ns: None,
            },
        );
    }

    findings
}

fn collect_traffic_findings(summary: &SummaryMetadata, findings: &mut Vec<AssessmentFinding>) {
    for error in &summary.errors {
        let level = if matches!(
            error.kind.as_str(),
            "client_disconnected" | "request_body_failed" | "event_index_failed"
        ) {
            AssessmentLevel::Warning
        } else {
            AssessmentLevel::Error
        };
        push_finding(
            findings,
            AssessmentFinding {
                level,
                source: AssessmentSource::Traffic,
                kind: error.kind.clone(),
                message: error.message.clone(),
                phase: Some(error.phase.clone()),
                at_ns: Some(error.at_ns.clone()),
            },
        );
    }

    if summary.errors.is_empty()
        && let Some(outcome) = summary.outcome
        && outcome != Outcome::Completed
    {
        let level = if outcome == Outcome::ClientDisconnected {
            AssessmentLevel::Warning
        } else {
            AssessmentLevel::Error
        };
        push_finding(
            findings,
            AssessmentFinding {
                level,
                source: AssessmentSource::Traffic,
                kind: outcome.as_str().to_string(),
                message: outcome_fallback_message(outcome).to_string(),
                phase: None,
                at_ns: summary.timing.finished_at_ns.clone(),
            },
        );
    }
}

fn collect_http_findings(summary: &SummaryMetadata, findings: &mut Vec<AssessmentFinding>) {
    if let Some(response) = &summary.response
        && response.status >= 400
    {
        push_finding(
            findings,
            AssessmentFinding {
                level: AssessmentLevel::Error,
                source: AssessmentSource::Http,
                kind: format!("http_{}", response.status),
                message: format!("Upstream returned HTTP {}", response.status),
                phase: Some("response".to_string()),
                at_ns: summary.timing.upstream_response_headers_at_ns.clone(),
            },
        );
    }
}

fn collect_protocol_findings(summary: &SummaryMetadata, findings: &mut Vec<AssessmentFinding>) {
    let Some(protocol) = &summary.protocol else {
        return;
    };
    for error in &protocol.errors {
        push_finding(
            findings,
            AssessmentFinding {
                level: AssessmentLevel::Error,
                source: AssessmentSource::Provider,
                kind: error.kind.clone(),
                message: error.message.clone(),
                phase: Some("model_api".to_string()),
                at_ns: error.at_ns.clone(),
            },
        );
    }
    for warning in &protocol.warnings {
        push_finding(
            findings,
            AssessmentFinding {
                level: AssessmentLevel::Warning,
                source: if warning.kind == "cancelled" {
                    AssessmentSource::Provider
                } else {
                    AssessmentSource::Diagnostic
                },
                kind: warning.kind.clone(),
                message: warning.message.clone(),
                phase: Some("model_api".to_string()),
                at_ns: warning.at_ns.clone(),
            },
        );
    }
    let streaming = protocol.response_mode.observed == Some(ResponseModeValue::Stream)
        || (protocol.response_mode.observed.is_none()
            && protocol.response_mode.requested == Some(ResponseModeValue::Stream));
    if summary.terminal
        && summary.outcome == Some(Outcome::Completed)
        && protocol.family != ProtocolFamily::Unknown
        && streaming
        && !protocol.response_terminal
    {
        push_finding(
            findings,
            AssessmentFinding {
                level: AssessmentLevel::Warning,
                source: AssessmentSource::Diagnostic,
                kind: "model_response_terminal_not_observed".to_string(),
                message: "The recognized model stream ended without a terminal protocol event"
                    .to_string(),
                phase: Some("model_api".to_string()),
                at_ns: summary
                    .timing
                    .upstream_response_body_completed_at_ns
                    .clone(),
            },
        );
    }
}

fn collect_diagnostic_warnings(summary: &SummaryMetadata, findings: &mut Vec<AssessmentFinding>) {
    for warning in &summary.warnings {
        push_finding(
            findings,
            AssessmentFinding {
                level: AssessmentLevel::Warning,
                source: AssessmentSource::Diagnostic,
                kind: warning.kind.clone(),
                message: warning.message.clone(),
                phase: Some(warning.phase.clone()),
                at_ns: Some(warning.at_ns.clone()),
            },
        );
    }
}

pub(crate) fn calculate_assessment(
    summary: &SummaryMetadata,
    active: bool,
    interrupted: bool,
) -> RecordAssessment {
    let findings = diagnostic_findings(summary, interrupted);
    if active {
        return RecordAssessment::active(findings.len());
    }
    let Some(primary) = findings
        .iter()
        .min_by_key(|finding| finding_sort_key(finding))
    else {
        return RecordAssessment::ok();
    };
    RecordAssessment {
        level: primary.level,
        primary: Some(AssessmentPrimary {
            source: primary.source,
            kind: primary.kind.clone(),
            message: primary.message.clone(),
        }),
        issue_count: findings.len(),
    }
}

pub(crate) fn refresh_assessment(summary: &mut SummaryMetadata) {
    summary.assessment = calculate_assessment(summary, !summary.terminal, false);
}

fn push_finding(findings: &mut Vec<AssessmentFinding>, finding: AssessmentFinding) {
    if let Some(existing) = findings.iter_mut().find(|existing| {
        existing.source == finding.source
            && existing.kind == finding.kind
            && existing.message == finding.message
    }) {
        if offset_key(finding.at_ns.as_deref()) < offset_key(existing.at_ns.as_deref()) {
            *existing = finding;
        }
        return;
    }
    findings.push(finding);
}

fn finding_sort_key(finding: &AssessmentFinding) -> (u8, u8, u128) {
    let severity = match finding.level {
        AssessmentLevel::Error => 0,
        AssessmentLevel::Warning => 1,
        AssessmentLevel::Active | AssessmentLevel::Ok => 2,
    };
    let source = if finding.source == AssessmentSource::Traffic
        && matches!(
            finding.kind.as_str(),
            "recording_failed" | "request_recording_failed" | "response_recording_failed"
        ) {
        0
    } else {
        match finding.source {
            AssessmentSource::Provider => 1,
            AssessmentSource::Traffic => 2,
            AssessmentSource::Http => 3,
            AssessmentSource::Diagnostic => 4,
        }
    };
    (severity, source, offset_key(finding.at_ns.as_deref()))
}

fn offset_key(value: Option<&str>) -> u128 {
    value
        .and_then(|value| value.parse().ok())
        .unwrap_or(u128::MAX)
}

fn outcome_fallback_message(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Completed => "The proxy attempt completed",
        Outcome::Rejected => "The proxy rejected the upstream request",
        Outcome::UpstreamError => "The upstream request or response failed",
        Outcome::ClientDisconnected => "The client disconnected before the proxy attempt completed",
        Outcome::RecordingFailed => "The Traffic Record could not be recorded completely",
        Outcome::ServerShutdown => "Traffic Proxy stopped before the attempt completed",
    }
}
