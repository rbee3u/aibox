use super::*;
use crate::traffic_interpretation::ProtocolDiagnostic;
use std::os::unix::fs::PermissionsExt;

#[test]
fn host_slug_and_flat_record_layout_are_safe() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let (record, request) = store
        .begin(ObservedRequest {
            upstream_url: Some("https://example.com/v1"),
            host_hint: Some("example.com"),
            ..ObservedRequest::test("POST", "/https://example.com/v1")
        })
        .unwrap();
    assert_eq!(record.directory.parent(), Some(store.root()));
    assert_eq!(request.format_version, FORMAT_VERSION);
    assert!(
        record
            .directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("active-")
    );
    assert!(
        record
            .directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("example.com")
    );
    assert_eq!(
        fs::metadata(store.root()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(record.directory.join(REQUEST_BODY))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(record.directory.join(SUMMARY_JSON).exists());
    assert!(!record.directory.join(RESULT_JSON).exists());
}

#[test]
fn summary_is_terminal_and_legacy_result_is_derived() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let (record, _) = store.begin(ObservedRequest::test("GET", "/bad")).unwrap();
    store
        .finish(
            &record,
            Instant::now(),
            &RuntimeMeasurements::default(),
            Outcome::Rejected,
            None,
        )
        .unwrap();
    let found = store.find(&record.id).unwrap();
    assert!(found.summary.terminal);
    let result = found.result.unwrap();
    assert_eq!(result.outcome, Outcome::Rejected);
    assert!(!result.ended_at.is_empty());
    let terminal_name = found.directory.file_name().unwrap().to_string_lossy();
    assert!(!terminal_name.starts_with("active-"));
    assert_eq!(terminal_name, found.sort_key);
    assert_eq!(record.locator.path(), found.directory);
}

#[test]
fn every_terminal_outcome_has_an_end_time_and_terminal_directory() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    for outcome in [
        Outcome::Completed,
        Outcome::Rejected,
        Outcome::UpstreamError,
        Outcome::ClientDisconnected,
        Outcome::RecordingFailed,
        Outcome::ServerShutdown,
    ] {
        let (record, _) = store
            .begin(ObservedRequest {
                host_hint: Some("example.test"),
                ..ObservedRequest::test("GET", "/outcome")
            })
            .unwrap();
        let result = store
            .finish(
                &record,
                Instant::now(),
                &RuntimeMeasurements::default(),
                outcome,
                None,
            )
            .unwrap();
        let stored = store.find(&record.id).unwrap();
        assert_eq!(stored.result.as_ref().unwrap().ended_at, result.ended_at);
        assert_eq!(stored.result.as_ref().unwrap().outcome, outcome);
        assert!(
            !stored
                .directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("active-")
        );
    }
}

#[test]
fn terminal_summary_is_immutable_to_late_checkpoints() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let (record, _) = store.begin(ObservedRequest::test("GET", "/late")).unwrap();
    store
        .finish(
            &record,
            Instant::now(),
            &RuntimeMeasurements::default(),
            Outcome::Completed,
            None,
        )
        .unwrap();
    let before = serde_json::to_value(store.find(&record.id).unwrap().summary).unwrap();

    let changed = store
        .update_summary(&record.locator, &record.summary, |summary| {
            summary.timing.upstream_request_body_completed_at_ns = Some("999".to_string());
            true
        })
        .unwrap();

    assert!(!changed);
    assert_eq!(
        serde_json::to_value(store.find(&record.id).unwrap().summary).unwrap(),
        before
    );
}

#[test]
fn response_metadata_publication_excludes_detail_readers() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let (record, _) = store
        .begin(ObservedRequest::test("GET", "/response"))
        .unwrap();
    let reader = read_unpoisoned(&store.namespace);
    let (started_sender, started_receiver) = std::sync::mpsc::sync_channel(0);
    let (finished_sender, finished_receiver) = std::sync::mpsc::sync_channel(0);
    let writer_store = store.clone();
    let locator = record.locator.clone();
    let summary = record.summary.clone();
    let writer = std::thread::spawn(move || {
        started_sender.send(()).unwrap();
        let result = writer_store.write_response(
            &locator,
            &summary,
            &ResponseMetadata {
                format_version: FORMAT_VERSION,
                source: ResponseSource::Upstream,
                headers_at: utc_now(),
                status: 200,
                http_version: "HTTP/1.1".to_string(),
                headers: Vec::new(),
            },
        );
        finished_sender.send(result).unwrap();
    });

    started_receiver.recv().unwrap();
    assert!(matches!(
        finished_receiver.recv_timeout(Duration::from_millis(100)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    drop(reader);
    finished_receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();
    writer.join().unwrap();

    assert_eq!(
        store.find(&record.id).unwrap().response.unwrap().status,
        200
    );
}

#[test]
fn safe_unprefixed_nonterminal_directory_remains_readable_without_migration() {
    let temp = tempfile::tempdir().unwrap();
    let first = TrafficStore::open(temp.path()).unwrap();
    let (record, _) = first
        .begin(ObservedRequest {
            host_hint: Some("legacy.test"),
            ..ObservedRequest::test("GET", "/legacy")
        })
        .unwrap();
    let active_name = record.directory.file_name().unwrap().to_string_lossy();
    let legacy_name = active_name.strip_prefix("active-").unwrap();
    let legacy_path = first.root().join(legacy_name);
    fs::rename(&record.directory, &legacy_path).unwrap();
    drop(first);

    let reopened = TrafficStore::open(temp.path()).unwrap();
    let stored = reopened.find(&record.id).unwrap();
    assert!(!stored.active);
    assert!(stored.result.is_none());
    assert_eq!(stored.directory, legacy_path);
    assert!(stored.sort_key.starts_with("active-"));
}

#[test]
fn terminal_summary_under_active_name_stays_terminal_and_uses_expected_sort_key() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let (record, _) = store
        .begin(ObservedRequest {
            host_hint: Some("example.test"),
            ..ObservedRequest::test("GET", "/stranded")
        })
        .unwrap();
    let active_path = record.directory.clone();
    store
        .finish(
            &record,
            Instant::now(),
            &RuntimeMeasurements::default(),
            Outcome::UpstreamError,
            None,
        )
        .unwrap();
    let terminal_path = record.locator.path();
    fs::rename(&terminal_path, &active_path).unwrap();

    let reopened = TrafficStore::open(temp.path()).unwrap();
    let stored = reopened.find(&record.id).unwrap();
    assert_eq!(stored.result.unwrap().outcome, Outcome::UpstreamError);
    assert!(
        stored
            .directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("active-")
    );
    assert!(!stored.sort_key.starts_with("active-"));
}

#[test]
fn no_clobber_rename_failure_preserves_terminal_outcome_and_source_directory() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let (record, _) = store
        .begin(ObservedRequest {
            host_hint: Some("example.test"),
            ..ObservedRequest::test("GET", "/collision")
        })
        .unwrap();
    let active_path = record.directory.clone();
    let first = store
        .finish(
            &record,
            Instant::now(),
            &RuntimeMeasurements::default(),
            Outcome::ServerShutdown,
            None,
        )
        .unwrap();
    let target = record.locator.path();
    fs::rename(&target, &active_path).unwrap();
    record.locator.set_path(active_path.clone());
    fs::create_dir(&target).unwrap();

    let repeated = store
        .finish(
            &record,
            Instant::now(),
            &RuntimeMeasurements::default(),
            Outcome::Completed,
            None,
        )
        .unwrap();

    assert_eq!(repeated.outcome, Outcome::ServerShutdown);
    assert_eq!(repeated.ended_at, first.ended_at);
    assert!(active_path.exists());
    assert!(target.exists());
    let listed = store.scan().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].request.id, record.id);
    assert!(!listed[0].active);
    let error = format!("{:#}", store.find(&record.id).unwrap_err());
    assert!(
        error.contains("Traffic request metadata does not exist"),
        "the preserved collision directory must remain visibly invalid: {error}"
    );
}

#[test]
fn normal_directory_order_matches_scanned_sort_keys_exactly() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let (first_active, _) = store
        .begin(ObservedRequest {
            host_hint: Some("z.test"),
            ..ObservedRequest::test("GET", "/first")
        })
        .unwrap();
    let (terminal, _) = store
        .begin(ObservedRequest {
            host_hint: Some("a.test"),
            ..ObservedRequest::test("GET", "/terminal")
        })
        .unwrap();
    store
        .finish(
            &terminal,
            Instant::now(),
            &RuntimeMeasurements::default(),
            Outcome::Completed,
            None,
        )
        .unwrap();
    let (last_active, _) = store
        .begin(ObservedRequest {
            host_hint: Some("a.test"),
            ..ObservedRequest::test("GET", "/last")
        })
        .unwrap();

    let scan = store.scan().unwrap();
    let scanned: Vec<_> = scan.iter().map(|record| record.sort_key.clone()).collect();
    let mut names: Vec<_> = fs::read_dir(store.root())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort_by(|left, right| right.cmp(left));

    assert_eq!(scanned, names);
    assert_eq!(scan[0].request.id, last_active.id);
    assert_eq!(scan[1].request.id, first_active.id);
    assert_eq!(scan[2].request.id, terminal.id);
}

#[test]
fn derived_result_uses_the_finished_monotonic_offset() {
    let mut summary = SummaryMetadata::test("018f4c8e-4b6b-7c13-8a22-2e4d6d6b6e12", None);
    summary.terminal = true;
    summary.timing.finished_at_ns = Some("1500000000".to_string());
    summary.outcome = Some(Outcome::Completed);
    refresh_assessment(&mut summary);

    let result = summary_to_result(&summary);
    assert!(result.ended_at.starts_with("2026-08-06T04:00:01"));
}

#[test]
fn assessment_preserves_evidence_and_prioritizes_recording_provider_transport_http_then_warning() {
    let mut summary = SummaryMetadata::test(
        "018f4c8e-4b6b-7c13-8a22-2e4d6d6b6e12",
        Some(ProtocolSummary::for_url(Some(
            "https://api.example.test/v1/responses",
        ))),
    );
    summary.terminal = true;
    summary.outcome = Some(Outcome::UpstreamError);
    summary.timing.finished_at_ns = Some("90".to_string());
    summary.response = Some(SummaryResponseMetadata {
        status: 401,
        http_version: "HTTP/2".to_string(),
    });
    summary.errors.extend([
        DiagnosticMetadata {
            phase: "response".to_string(),
            kind: "response_recording_failed".to_string(),
            message: "response bytes could not be recorded".to_string(),
            at_ns: "90".to_string(),
        },
        DiagnosticMetadata {
            phase: "response".to_string(),
            kind: "response_recording_failed".to_string(),
            message: "response bytes could not be recorded".to_string(),
            at_ns: "70".to_string(),
        },
    ]);
    summary
        .protocol
        .as_mut()
        .unwrap()
        .errors
        .push(ProtocolDiagnostic {
            kind: "service_unavailable_error".to_string(),
            message: "provider overloaded".to_string(),
            at_ns: Some("10".to_string()),
        });
    summary.warnings.push(DiagnosticMetadata {
        phase: "recording".to_string(),
        kind: "event_index_failed".to_string(),
        message: "timing index unavailable".to_string(),
        at_ns: "20".to_string(),
    });

    refresh_assessment(&mut summary);
    assert_eq!(summary.assessment.level, AssessmentLevel::Error);
    assert_eq!(summary.assessment.issue_count, 4);
    let primary = summary.assessment.primary.as_ref().unwrap();
    assert_eq!(primary.source, AssessmentSource::Traffic);
    assert_eq!(primary.kind, "response_recording_failed");
    assert_eq!(
        diagnostic_findings(&summary, false)
            .iter()
            .find(|finding| finding.kind == "response_recording_failed")
            .unwrap()
            .at_ns
            .as_deref(),
        Some("70")
    );

    summary
        .errors
        .retain(|error| error.kind != "response_recording_failed");
    refresh_assessment(&mut summary);
    assert_eq!(
        summary.assessment.primary.as_ref().unwrap().source,
        AssessmentSource::Provider
    );

    summary.protocol.as_mut().unwrap().errors.clear();
    summary.errors.push(DiagnosticMetadata {
        phase: "response".to_string(),
        kind: "upstream_response_failed".to_string(),
        message: "connection reset".to_string(),
        at_ns: "30".to_string(),
    });
    refresh_assessment(&mut summary);
    assert_eq!(
        summary.assessment.primary.as_ref().unwrap().source,
        AssessmentSource::Traffic
    );

    summary.errors.clear();
    summary.outcome = Some(Outcome::Completed);
    refresh_assessment(&mut summary);
    assert_eq!(
        summary.assessment.primary.as_ref().unwrap().source,
        AssessmentSource::Http
    );

    summary.response = None;
    refresh_assessment(&mut summary);
    assert_eq!(
        summary.assessment.primary.as_ref().unwrap().source,
        AssessmentSource::Diagnostic
    );
    assert_eq!(summary.assessment.level, AssessmentLevel::Warning);
}

#[test]
fn client_disconnect_and_request_abort_are_warnings_but_recording_failure_is_error() {
    for (kind, outcome, level) in [
        (
            "client_disconnected",
            Outcome::ClientDisconnected,
            AssessmentLevel::Warning,
        ),
        (
            "request_body_failed",
            Outcome::ClientDisconnected,
            AssessmentLevel::Warning,
        ),
        (
            "request_recording_failed",
            Outcome::RecordingFailed,
            AssessmentLevel::Error,
        ),
    ] {
        let mut summary = SummaryMetadata::test("018f4c8e-4b6b-7c13-8a22-2e4d6d6b6e12", None);
        summary.terminal = true;
        summary.outcome = Some(outcome);
        summary.timing.finished_at_ns = Some("10".to_string());
        summary.errors.push(DiagnosticMetadata {
            phase: "request".to_string(),
            kind: kind.to_string(),
            message: "request stream ended".to_string(),
            at_ns: "10".to_string(),
        });
        refresh_assessment(&mut summary);
        assert_eq!(summary.assessment.level, level, "{kind}");
    }
}

#[test]
fn summary_scan_ignores_body_and_metadata_corruption_but_detail_is_strict() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let outside = temp.path().join("outside");
    fs::write(&outside, b"outside").unwrap();
    let mut corrupted_records = Vec::new();
    for corruption in ["request_metadata", "response_metadata", "request_body"] {
        let (mut record, _) = store
            .begin(ObservedRequest::test("GET", "/corrupt"))
            .unwrap();
        record.request_body.write_all(b"raw request").unwrap();
        record.response_body.write_all(b"raw response").unwrap();
        store
            .write_response(
                &record.locator,
                &record.summary,
                &ResponseMetadata {
                    format_version: FORMAT_VERSION,
                    source: ResponseSource::Upstream,
                    headers_at: utc_now(),
                    status: 200,
                    http_version: "HTTP/1.1".to_string(),
                    headers: Vec::new(),
                },
            )
            .unwrap();
        store
            .finish(
                &record,
                Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Completed,
                None,
            )
            .unwrap();
        let directory = record.locator.path();
        match corruption {
            "request_metadata" => {
                fs::write(directory.join(REQUEST_JSON), b"not json").unwrap();
            }
            "response_metadata" => {
                fs::remove_file(directory.join(RESPONSE_JSON)).unwrap();
                std::os::unix::fs::symlink(&outside, directory.join(RESPONSE_JSON)).unwrap();
            }
            "request_body" => {
                fs::remove_file(directory.join(REQUEST_BODY)).unwrap();
                std::os::unix::fs::symlink(&outside, directory.join(REQUEST_BODY)).unwrap();
            }
            _ => unreachable!(),
        }
        corrupted_records.push((corruption, record.id));
    }

    assert_eq!(
        store.scan_summaries().unwrap().len(),
        corrupted_records.len()
    );
    for (corruption, id) in corrupted_records {
        let error = format!("{:#}", store.find(&id).unwrap_err());
        let expected = match corruption {
            "request_metadata" => "parse Traffic request metadata",
            "response_metadata" => "Traffic response metadata is not a regular file",
            "request_body" => "Traffic request body is not a regular file",
            _ => unreachable!(),
        };
        assert!(
            error.contains(expected),
            "{corruption} should fail detail reads for its own reason: {error}"
        );
    }
}

#[test]
fn recorded_headers_drop_connection_named_fields() {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("connection", "x-hop, keep-alive".parse().unwrap());
    headers.insert("x-hop", "secret".parse().unwrap());
    headers.insert("x-app", "kept".parse().unwrap());

    let recorded = RecordedHeader::from_headers(&headers);

    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].name, "x-app");
}

#[test]
fn persisted_metadata_uses_the_stable_schema_names() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let (record, _) = store
        .begin(ObservedRequest {
            upstream_url: Some("https://example.com/v1/responses"),
            http_version: "HTTP/2",
            headers: vec![RecordedHeader {
                name: "Session-Id".to_string(),
                value_base64: base64::engine::general_purpose::STANDARD.encode("opaque-session"),
            }],
            host_hint: Some("example.com"),
            ..ObservedRequest::test("POST", "/https://example.com/v1/responses")
        })
        .unwrap();
    store
        .write_response(
            &record.locator,
            &record.summary,
            &ResponseMetadata {
                format_version: FORMAT_VERSION,
                source: ResponseSource::Upstream,
                headers_at: utc_now(),
                status: 200,
                http_version: "HTTP/2".to_string(),
                headers: vec![],
            },
        )
        .unwrap();
    let request: serde_json::Value =
        serde_json::from_reader(fs::File::open(record.directory.join(REQUEST_JSON)).unwrap())
            .unwrap();
    let response: serde_json::Value =
        serde_json::from_reader(fs::File::open(record.directory.join(RESPONSE_JSON)).unwrap())
            .unwrap();
    let summary: serde_json::Value =
        serde_json::from_reader(fs::File::open(record.directory.join(SUMMARY_JSON)).unwrap())
            .unwrap();
    assert_eq!(request["schema_version"], FORMAT_VERSION);
    assert_eq!(request["record_id"], record.id);
    assert_eq!(request["kind"], "request");
    assert!(request.get("format_version").is_none());
    assert_eq!(response["kind"], "response");
    assert!(response.get("source").is_none());
    assert_eq!(summary["kind"], "summary");
    assert_eq!(summary["request"]["method"], "POST");
    assert_eq!(
        summary["request"]["incoming_uri"],
        "/https://example.com/v1/responses"
    );
    assert_eq!(summary["request"]["http_version"], "HTTP/2");
    assert_eq!(summary["response"]["status"], 200);
    assert_eq!(summary["assessment"]["level"], "active");
    assert_eq!(summary["coding_agent_session_id"], "opaque-session");
    assert_eq!(summary["protocol"]["family"], "openai_responses");
    assert_eq!(summary["protocol"]["response_terminal"], false);
    assert!(summary["protocol"]["model"]["requested"].is_null());
    assert!(record.directory.join(RESPONSE_BODY).is_file());
    assert!(!record.directory.join(RESULT_JSON).exists());
}

#[test]
fn version_one_summaries_are_unsupported() {
    let error = validate_schema(1, "summary", "summary").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("unsupported Traffic schema version 1")
    );
}

#[test]
fn protocol_checkpoints_survive_restart_without_lazy_backfill() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let (record, _) = store
        .begin(ObservedRequest {
            upstream_url: Some("https://example.com/v1/responses"),
            http_version: "HTTP/2",
            host_hint: Some("example.com"),
            ..ObservedRequest::test("POST", "/https://example.com/v1/responses")
        })
        .unwrap();
    store
        .update_summary(&record.locator, &record.summary, |summary| {
            summary.protocol.as_mut().unwrap().model.requested = Some("gpt-requested".to_string());
            true
        })
        .unwrap();

    let restarted = TrafficStore::open(temp.path()).unwrap();
    let found = restarted.find(&record.id).unwrap();
    assert_eq!(
        found
            .summary
            .protocol
            .as_ref()
            .unwrap()
            .model
            .requested
            .as_deref(),
        Some("gpt-requested")
    );

    let summary_path = record.directory.join(SUMMARY_JSON);
    let mut legacy: serde_json::Value =
        serde_json::from_reader(fs::File::open(&summary_path).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("protocol");
    fs::write(&summary_path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
    let before_read = fs::read(&summary_path).unwrap();

    let legacy_store = TrafficStore::open(temp.path()).unwrap();
    assert!(
        legacy_store
            .find(&record.id)
            .unwrap()
            .summary
            .protocol
            .is_none()
    );
    assert_eq!(fs::read(summary_path).unwrap(), before_read);
}

#[test]
fn concurrent_summary_updates_publish_a_single_monotonic_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let (record, _) = store
        .begin(ObservedRequest {
            upstream_url: Some("https://example.com/v1/responses"),
            http_version: "HTTP/2",
            host_hint: Some("example.com"),
            ..ObservedRequest::test("POST", "/https://example.com/v1/responses")
        })
        .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let locator = record.locator.clone();
    let summary = record.summary.clone();
    let first_store = store.clone();
    let first_barrier = barrier.clone();
    let first = std::thread::spawn(move || {
        first_barrier.wait();
        first_store
            .update_summary(&locator, &summary, |value| {
                value.timing.upstream_request_body_completed_at_ns = Some("10".to_string());
                true
            })
            .unwrap();
    });
    let locator = record.locator.clone();
    let summary = record.summary.clone();
    let second_store = store;
    let second_barrier = barrier.clone();
    let second = std::thread::spawn(move || {
        second_barrier.wait();
        second_store
            .update_summary(&locator, &summary, |value| {
                value.protocol.as_mut().unwrap().model.effective =
                    Some("gpt-effective".to_string());
                true
            })
            .unwrap();
    });
    barrier.wait();
    first.join().unwrap();
    second.join().unwrap();

    let persisted = TrafficStore::open(temp.path())
        .unwrap()
        .find(&record.id)
        .unwrap()
        .summary;
    assert_eq!(
        persisted
            .timing
            .upstream_request_body_completed_at_ns
            .as_deref(),
        Some("10")
    );
    assert_eq!(
        persisted.protocol.unwrap().model.effective.as_deref(),
        Some("gpt-effective")
    );
}

#[test]
fn missing_terminal_metadata_is_interrupted_unless_currently_active() {
    let temp = tempfile::tempdir().unwrap();
    let first = TrafficStore::open(temp.path()).unwrap();
    let (record, _) = first.begin(ObservedRequest::test("GET", "/bad")).unwrap();
    assert!(first.find(&record.id).unwrap().active);
    let restarted = TrafficStore::open(temp.path()).unwrap();
    assert!(!restarted.find(&record.id).unwrap().active);
    assert!(restarted.find(&record.id).unwrap().result.is_none());
}

#[test]
fn collection_ignores_unknown_and_misnamed_direct_children() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    fs::write(store.root().join("unknown-file"), b"leave me alone").unwrap();
    fs::create_dir(store.root().join("unknown-directory")).unwrap();
    let (record, _) = store.begin(ObservedRequest::test("GET", "/bad")).unwrap();
    let id = record.id.clone();
    let renamed = store.root().join(format!("wrong-name-{id}"));
    fs::rename(&record.directory, &renamed).unwrap();
    assert!(store.scan().unwrap().is_empty());
    let find_error = format!("{:#}", store.find(&id).unwrap_err());
    assert!(
        find_error.contains("Traffic Record directory name is not structurally valid"),
        "{find_error}"
    );
    store.abandon_active(&id);
    let delete_error = format!("{:#}", store.delete_ids(&[id]).unwrap_err());
    assert!(
        delete_error.contains("Traffic Record directory name is not structurally valid"),
        "{delete_error}"
    );
    assert!(renamed.exists());
}

#[test]
fn explicit_lookup_rejects_duplicate_record_directories() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let (record, _) = store
        .begin(ObservedRequest::test("GET", "/duplicate"))
        .unwrap();
    let original_name = record.directory.file_name().unwrap().to_str().unwrap();
    let duplicate = store.root().join(original_name.replace(
        &format!("-invalid-{}", record.id),
        &format!("-duplicate-{}", record.id),
    ));
    fs::create_dir(&duplicate).unwrap();
    for entry in fs::read_dir(&record.directory).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), duplicate.join(entry.file_name())).unwrap();
    }

    let error = store.find(&record.id).unwrap_err().to_string();
    assert!(
        error.contains("multiple Traffic Record directories"),
        "{error}"
    );
}

#[cfg(unix)]
#[test]
fn opening_and_scanning_never_follow_symlinked_traffic_paths() {
    use std::os::unix::fs::symlink;

    let linked_root = tempfile::tempdir().unwrap();
    let outside_collection = tempfile::tempdir().unwrap();
    fs::write(outside_collection.path().join("keep"), b"outside").unwrap();
    symlink(
        outside_collection.path(),
        linked_root.path().join("traffic"),
    )
    .unwrap();
    let error = TrafficStore::open(linked_root.path())
        .err()
        .expect("a symlinked collection must be rejected")
        .to_string();
    assert!(error.contains("not a real directory"), "{error}");
    assert_eq!(
        fs::read(outside_collection.path().join("keep")).unwrap(),
        b"outside"
    );

    let root = tempfile::tempdir().unwrap();
    let outside_body = tempfile::tempdir().unwrap();
    let target = outside_body.path().join("request.body");
    fs::write(&target, b"secret").unwrap();
    let store = TrafficStore::open(root.path()).unwrap();
    let (record, _) = store
        .begin(ObservedRequest::test("POST", "/unsafe"))
        .unwrap();
    let body = record.directory.join(REQUEST_BODY);
    fs::remove_file(&body).unwrap();
    symlink(&target, &body).unwrap();

    assert!(store.scan().unwrap().is_empty());
    assert_eq!(fs::read(target).unwrap(), b"secret");
}

#[cfg(unix)]
#[test]
fn deletion_rejects_symlinked_record_entries_without_touching_targets() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("secret");
    fs::write(&target, b"keep").unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let (record, _) = store.begin(ObservedRequest::test("GET", "/bad")).unwrap();
    store
        .finish(
            &record,
            Instant::now(),
            &RuntimeMeasurements::default(),
            Outcome::Rejected,
            None,
        )
        .unwrap();
    let terminal_directory = record.locator.path();
    symlink(&target, terminal_directory.join("unsafe-link")).unwrap();
    let error = store
        .delete_ids(std::slice::from_ref(&record.id))
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsafe entry"), "{error}");
    assert_eq!(fs::read(target).unwrap(), b"keep");
    assert!(terminal_directory.exists());
}

#[test]
fn delete_all_rechecks_the_non_active_count_and_preserves_active_records() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    for _ in 0..2 {
        let (record, _) = store.begin(ObservedRequest::test("GET", "/bad")).unwrap();
        store
            .finish(
                &record,
                Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Rejected,
                None,
            )
            .unwrap();
    }
    let (active, _) = store
        .begin(ObservedRequest::test("GET", "/active"))
        .unwrap();
    let error = store.delete_all(1).unwrap_err().to_string();
    assert_eq!(
        error,
        "deletable Traffic Record count changed (expected 1, now 2)"
    );
    assert_eq!(store.scan().unwrap().len(), 3);
    assert_eq!(store.delete_all(2).unwrap(), 2);
    let remaining = store.scan().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].request.id, active.id);
    assert!(remaining[0].active);
}

#[test]
fn delete_ids_requires_a_unique_valid_non_active_selection_before_removing_anything() {
    let temp = tempfile::tempdir().unwrap();
    let store = TrafficStore::open(temp.path()).unwrap();
    let make_finished = |uri| {
        let (record, _) = store.begin(ObservedRequest::test("GET", uri)).unwrap();
        let id = record.id.clone();
        store
            .finish(
                &record,
                Instant::now(),
                &RuntimeMeasurements::default(),
                Outcome::Rejected,
                None,
            )
            .unwrap();
        id
    };
    let first = make_finished("/first");
    let second = make_finished("/second");
    let (active, _) = store
        .begin(ObservedRequest::test("GET", "/active"))
        .unwrap();
    let missing = Uuid::now_v7().to_string();

    for (ids, expected) in [
        (Vec::new(), "at least one"),
        (vec![first.clone(), first.clone()], "must not be repeated"),
        (
            vec![
                first.clone(),
                "550e8400-e29b-41d4-a716-446655440000".to_string(),
            ],
            "not UUID v7",
        ),
        (vec![first.clone(), missing], "not found"),
        (vec![first.clone(), active.id.clone()], "active Traffic"),
    ] {
        let error = store.delete_ids(&ids).unwrap_err().to_string();
        assert!(error.contains(expected), "{ids:?}: {error}");
        assert!(
            store.find(&first).is_ok(),
            "{ids:?} removed the first record"
        );
        assert!(
            store.find(&second).is_ok(),
            "{ids:?} removed the second record"
        );
    }

    assert_eq!(store.delete_ids(&[first, second]).unwrap(), 2);
    let remaining = store.scan().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].request.id, active.id);
    assert!(remaining[0].active);
}
