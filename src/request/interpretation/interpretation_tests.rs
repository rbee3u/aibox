use super::*;
use base64::Engine as _;
use std::fs;

fn header(name: &str, value: &[u8]) -> RecordedHeader {
    RecordedHeader {
        name: name.to_string(),
        value_base64: base64::engine::general_purpose::STANDARD.encode(value),
    }
}

#[test]
fn body_content_coding_accepts_identity_and_one_case_insensitive_coding() {
    assert_eq!(
        body_content_coding(&[]).unwrap(),
        BodyContentCoding::Identity
    );
    assert_eq!(
        body_content_coding(&[header("Content-Encoding", b"  ")]).unwrap(),
        BodyContentCoding::Identity
    );
    assert_eq!(
        body_content_coding(&[header("CONTENT-ENCODING", b" ZsTd ")]).unwrap(),
        BodyContentCoding::Zstd
    );
    assert_eq!(
        body_content_coding(&[header("content-encoding", b" GzIp ")]).unwrap(),
        BodyContentCoding::Gzip
    );
    let error = body_content_coding(&[header("content-encoding", b"identity, zstd")])
        .unwrap_err()
        .to_string();
    assert_eq!(error, "unsupported Content-Encoding \"identity, zstd\"");

    let invalid_utf8 = header("content-encoding", &[0xff]);
    assert_eq!(
        body_content_coding(&[invalid_utf8])
            .unwrap_err()
            .to_string(),
        "Content-Encoding header is not UTF-8"
    );
}

#[test]
fn coding_agent_session_id_uses_protocol_specific_exact_headers() {
    let headers = [
        header("X-Claude-Code-Session-Id", b"claude-session"),
        header("SESSION-ID", b"codex-session"),
    ];
    assert_eq!(
        coding_agent_session_id(Some("https://example.test/v1/responses"), &headers).as_deref(),
        Some("codex-session")
    );
    assert_eq!(
        coding_agent_session_id(Some("https://example.test/v1/messages"), &headers).as_deref(),
        Some("claude-session")
    );
    assert_eq!(
        coding_agent_session_id(Some("https://example.test/v1/responses"), &headers[..1])
            .as_deref(),
        Some("claude-session")
    );
    assert_eq!(
        coding_agent_session_id(
            Some("https://example.test/openai/deployments/gpt/chat/completions/?api-version=1"),
            &headers,
        )
        .as_deref(),
        Some("codex-session")
    );
    assert_eq!(
        coding_agent_session_id(Some("https://example.test/health"), &headers),
        None
    );
}

#[test]
fn coding_agent_session_id_keeps_the_first_nonempty_utf8_value() {
    let headers = [
        header("session-id", b""),
        header("session-id", b"opaque-session-value"),
        header("session-id", &[0xff]),
        header("x-session-id", b"ignored"),
    ];
    assert_eq!(
        coding_agent_session_id(Some("https://example.test/v1/responses"), &headers).as_deref(),
        Some("opaque-session-value")
    );
}

#[test]
fn request_metadata_maps_provider_specific_reasoning_effort() {
    let temp = tempfile::tempdir().unwrap();
    let openai_path = temp.path().join("openai-request.json");
    fs::write(
        &openai_path,
        br#"{"model":"gpt-requested","stream":true,"reasoning":{"effort":"high"}}"#,
    )
    .unwrap();
    let mut openai = ProtocolObserver::new(Some("https://example.test/v1/responses"));
    assert!(openai.observe_request(&openai_path, &[], "10".to_string()));
    let summary = openai.snapshot();
    assert_eq!(summary.model.requested.as_deref(), Some("gpt-requested"));
    assert_eq!(summary.reasoning_effort.requested.as_deref(), Some("high"));
    assert_eq!(
        summary.response_mode.requested,
        Some(ResponseModeValue::Stream)
    );

    let claude_path = temp.path().join("claude-request.json");
    fs::write(
        &claude_path,
        br#"{"model":"claude-requested","output_config":{"effort":"max"}}"#,
    )
    .unwrap();
    let mut claude = ProtocolObserver::new(Some("https://example.test/v1/messages"));
    assert!(claude.observe_request(&claude_path, &[], "20".to_string()));
    let summary = claude.snapshot();
    assert_eq!(summary.reasoning_effort.requested.as_deref(), Some("max"));
    assert_eq!(
        summary.response_mode.requested,
        Some(ResponseModeValue::Normal)
    );

    let chat_path = temp.path().join("chat-request.json");
    fs::write(
        &chat_path,
        br#"{"model":"gpt-chat","reasoning_effort":"medium","stream":true,"stream_options":{"include_usage":true}}"#,
    )
    .unwrap();
    let mut chat = ProtocolObserver::new(Some(
        "https://example.test/openai/deployments/gpt/chat/completions?api-version=1",
    ));
    assert!(chat.observe_request(&chat_path, &[], "30".to_string()));
    let summary = chat.snapshot();
    assert_eq!(summary.family, ProtocolFamily::OpenaiChatCompletions);
    assert_eq!(summary.model.requested.as_deref(), Some("gpt-chat"));
    assert_eq!(
        summary.reasoning_effort.requested.as_deref(),
        Some("medium")
    );
    assert_eq!(
        summary.response_mode.requested,
        Some(ResponseModeValue::Stream)
    );
    // Nothing has reported usage yet, so this is exactly the recorded
    // `stream_options.include_usage` expectation, read through the accessor
    // `apply_chat_done` uses rather than by peeking at a private field.
    assert!(chat.usage.stream_usage_missing());
}

#[test]
fn zstd_request_metadata_is_interpreted_after_the_recorded_body_is_complete() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("request.zstd");
    let compressed = zstd::stream::encode_all(
        br#"{"model":"gpt-compressed","reasoning":{"effort":"medium"},"stream":true}"#.as_slice(),
        0,
    )
    .unwrap();
    fs::write(&path, compressed).unwrap();
    let headers = [RecordedHeader {
        name: "content-encoding".to_string(),
        value_base64: base64::engine::general_purpose::STANDARD.encode(" ZsTd "),
    }];
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));

    assert!(observer.observe_request(&path, &headers, "10".to_string()));
    let summary = observer.snapshot();
    assert_eq!(summary.model.requested.as_deref(), Some("gpt-compressed"));
    assert_eq!(
        summary.reasoning_effort.requested.as_deref(),
        Some("medium")
    );
    assert_eq!(
        summary.response_mode.requested,
        Some(ResponseModeValue::Stream)
    );
}

#[test]
fn response_headers_and_nonstream_body_publish_stable_effective_facts() {
    let temp = tempfile::tempdir().unwrap();
    let response_path = temp.path().join("response.json");
    fs::write(
        &response_path,
        br#"{"object":"response","model":"body-model","reasoning_effort":"medium","usage":{"input_tokens":12,"output_tokens":4}}"#,
    )
    .unwrap();
    let headers = [RecordedHeader {
        name: "openai-model".to_string(),
        value_base64: "aGVhZGVyLW1vZGVs".to_string(),
    }];
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
    assert!(observer.observe_response_headers(&headers, Some(false), "10".to_string()));
    assert!(observer.observe_json_response(&response_path, 200, &[], "20".to_string()));
    let summary = observer.snapshot();
    assert_eq!(summary.model.effective.as_deref(), Some("header-model"));
    assert_eq!(
        summary.reasoning_effort.effective.as_deref(),
        Some("medium")
    );
    assert_eq!(
        summary.response_mode.observed,
        Some(ResponseModeValue::Normal)
    );
    assert!(summary.response_terminal);
    assert!(summary.first_token_at_ns.is_none());
    assert_eq!(summary.token_usage.unwrap().output_tokens, Some(4));
    assert_eq!(summary.warnings.len(), 1);
    assert_eq!(summary.warnings[0].kind, "effective_model_conflict");
}

#[test]
fn malformed_protocol_data_warns_without_synthesizing_provider_errors() {
    let temp = tempfile::tempdir().unwrap();
    let response_path = temp.path().join("response.json");
    fs::write(&response_path, b"not json").unwrap();
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
    assert!(observer.observe_sse_data(b"not json", "10".to_string()));
    assert!(observer.observe_json_response(&response_path, 503, &[], "20".to_string()));
    let summary = observer.snapshot();
    assert!(summary.response_terminal);
    assert_eq!(summary.warnings.len(), 2);
    assert!(summary.errors.is_empty());

    let unknown = ProtocolObserver::new(Some("https://example.test/health"));
    let summary = unknown.snapshot();
    assert_eq!(summary.family, ProtocolFamily::Unknown);
    assert!(!summary.response_terminal);
    assert!(summary.errors.is_empty());
}

#[test]
fn zstd_response_metadata_is_interpreted_after_the_recorded_body_is_complete() {
    let temp = tempfile::tempdir().unwrap();
    let response_path = temp.path().join("response.zstd");
    let compressed = zstd::stream::encode_all(
        br#"{"object":"response","model":"gpt-compressed","usage":{"input_tokens":12,"output_tokens":4}}"#
            .as_slice(),
        0,
    )
    .unwrap();
    fs::write(&response_path, compressed).unwrap();
    let headers = [RecordedHeader {
        name: "content-encoding".to_string(),
        value_base64: base64::engine::general_purpose::STANDARD.encode("zstd"),
    }];
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));

    assert!(observer.observe_json_response(&response_path, 200, &headers, "20".to_string()));
    let summary = observer.snapshot();
    assert_eq!(summary.model.effective.as_deref(), Some("gpt-compressed"));
    assert_eq!(summary.token_usage.unwrap().output_tokens, Some(4));
    assert!(summary.response_terminal);
}

#[test]
fn gzip_response_metadata_is_interpreted_after_the_recorded_body_is_complete() {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write as _;

    let temp = tempfile::tempdir().unwrap();
    let response_path = temp.path().join("response.gzip");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(
            br#"{"type":"message","model":"claude-compressed","usage":{"input_tokens":12,"output_tokens":4}}"#,
        )
        .unwrap();
    fs::write(&response_path, encoder.finish().unwrap()).unwrap();
    let headers = [RecordedHeader {
        name: "content-encoding".to_string(),
        value_base64: base64::engine::general_purpose::STANDARD.encode("gzip"),
    }];
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/messages"));

    assert!(observer.observe_json_response(&response_path, 200, &headers, "20".to_string()));
    let summary = observer.snapshot();
    assert_eq!(
        summary.model.effective.as_deref(),
        Some("claude-compressed")
    );
    assert_eq!(summary.token_usage.unwrap().output_tokens, Some(4));
    assert!(summary.warnings.is_empty());
    assert!(summary.response_terminal);
}

#[test]
fn chat_nonstream_body_infers_family_and_normalizes_usage() {
    let temp = tempfile::tempdir().unwrap();
    let response_path = temp.path().join("chat-response.json");
    fs::write(
        &response_path,
        br#"{"object":"chat.completion","model":"gpt-effective","choices":[{"finish_reason":"length"}],"usage":{"prompt_tokens":100,"prompt_tokens_details":{"cached_tokens":40,"cache_write_tokens":10},"completion_tokens":20,"completion_tokens_details":{"reasoning_tokens":5},"total_tokens":120}}"#,
    )
    .unwrap();
    let mut observer = ProtocolObserver::new(Some("https://example.test/gateway"));

    assert!(observer.observe_json_response(&response_path, 200, &[], "20".to_string()));
    let summary = observer.snapshot();
    assert_eq!(summary.family, ProtocolFamily::OpenaiChatCompletions);
    assert_eq!(summary.model.effective.as_deref(), Some("gpt-effective"));
    assert!(summary.response_terminal);
    let usage = summary.token_usage.unwrap();
    assert_eq!(usage.total_input_tokens, Some(100));
    assert_eq!(usage.base_input_tokens, Some(50));
    assert_eq!(usage.cached_input_tokens, Some(40));
    assert_eq!(usage.cache_write_tokens, Some(10));
    assert_eq!(usage.output_tokens, Some(20));
    assert_eq!(usage.reasoning_output_tokens, Some(5));
    assert_eq!(summary.errors[0].kind, "response_incomplete");
}

#[test]
fn openai_usage_is_not_published_before_terminal_event() {
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
    assert!(observer.observe_sse_data(
        br#"{"type":"response.created","response":{"model":"effective","usage":{"input_tokens":100}}}"#,
        "10".to_string(),
    ));
    assert!(observer.snapshot().token_usage.is_none());
    assert!(observer.observe_sse_data(
        br#"{"type":"response.completed","response":{"usage":{"input_tokens":100,"input_tokens_details":{"cached_tokens":40,"cache_write_tokens":10},"output_tokens":20,"output_tokens_details":{"reasoning_tokens":5}}}}"#,
        "20".to_string(),
    ));
    let usage = observer.snapshot().token_usage.unwrap();
    assert_eq!(usage.total_input_tokens, Some(100));
    assert_eq!(usage.base_input_tokens, Some(50));
    assert_eq!(usage.reasoning_output_tokens, Some(5));
}

#[test]
fn chat_stream_holds_usage_until_done_and_reports_finish_reasons() {
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/chat/completions"));
    assert!(observer.observe_sse_data(
        br#"{"object":"chat.completion.chunk","model":"gpt-stream","choices":[{"finish_reason":null}]}"#,
        "10".to_string(),
    ));
    assert!(observer.observe_sse_data(
        br#"{"object":"chat.completion.chunk","choices":[{"finish_reason":"content_filter"},{"finish_reason":"vendor_stop"}],"usage":{"prompt_tokens":50,"prompt_tokens_details":{"cached_tokens":20},"completion_tokens":7,"completion_tokens_details":{"reasoning_tokens":2},"total_tokens":57}}"#,
        "20".to_string(),
    ));
    let partial = observer.snapshot();
    assert_eq!(partial.model.effective.as_deref(), Some("gpt-stream"));
    assert!(!partial.response_terminal);
    assert!(partial.token_usage.is_none());
    assert_eq!(partial.errors[0].kind, "content_filtered");
    assert_eq!(partial.warnings[0].kind, "finish_reason_unknown");

    assert!(observer.observe_sse_data(b" \t[DONE]\r\n", "30".to_string()));
    let summary = observer.snapshot();
    assert!(summary.response_terminal);
    let usage = summary.token_usage.unwrap();
    assert_eq!(usage.total_input_tokens, Some(50));
    assert_eq!(usage.base_input_tokens, Some(30));
    assert_eq!(usage.output_tokens, Some(7));
    assert_eq!(usage.reasoning_output_tokens, Some(2));
}

#[test]
fn chat_done_warns_when_requested_stream_usage_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let request_path = temp.path().join("chat-request.json");
    fs::write(
        &request_path,
        br#"{"model":"gpt-chat","stream":true,"stream_options":{"include_usage":true}}"#,
    )
    .unwrap();
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/chat/completions"));
    observer.observe_request(&request_path, &[], "10".to_string());

    assert!(observer.observe_sse_data(b"[DONE]", "20".to_string()));
    let summary = observer.snapshot();
    assert!(summary.response_terminal);
    assert!(summary.token_usage.is_none());
    assert_eq!(summary.warnings[0].kind, "token_usage_missing");
}

#[test]
fn chat_stream_error_is_terminal_and_usage_inconsistency_warns() {
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/chat/completions"));
    observer.observe_sse_data(
        br#"{"object":"chat.completion.chunk","usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":99},"error":{"type":"server_error","message":"failed"}}"#,
        "10".to_string(),
    );

    let summary = observer.snapshot();
    assert!(summary.response_terminal);
    assert_eq!(summary.errors[0].kind, "server_error");
    assert_eq!(summary.warnings[0].kind, "token_usage_inconsistent");
    assert_eq!(summary.token_usage.unwrap().output_tokens, Some(2));
}

#[test]
fn claude_usage_is_accumulated_until_message_stop() {
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/messages"));
    observer.observe_sse_data(
        br#"{"type":"message_start","message":{"model":"claude","usage":{"input_tokens":37,"cache_read_input_tokens":340,"cache_creation_input_tokens":38}}}"#,
        "10".to_string(),
    );
    observer.observe_sse_data(
        br#"{"type":"message_delta","usage":{"output_tokens":13}}"#,
        "20".to_string(),
    );
    assert!(observer.snapshot().token_usage.is_none());
    observer.observe_sse_data(br#"{"type":"message_stop"}"#, "30".to_string());
    let usage = observer.snapshot().token_usage.unwrap();
    assert_eq!(usage.total_input_tokens, Some(415));
    assert_eq!(usage.output_tokens, Some(13));
}

#[test]
fn claude_missing_cache_counters_stay_unreported() {
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/messages"));
    observer.observe_sse_data(
        br#"{"type":"message_start","message":{"usage":{"input_tokens":37}}}"#,
        "10".to_string(),
    );
    observer.observe_sse_data(br#"{"type":"message_stop"}"#, "20".to_string());

    let usage = observer.snapshot().token_usage.unwrap();
    assert_eq!(usage.total_input_tokens, Some(37));
    assert_eq!(usage.cached_input_tokens, None);
    assert_eq!(usage.cache_write_tokens, None);
    assert_eq!(usage.cache_write_5m_tokens, None);
    assert_eq!(usage.cache_write_1h_tokens, None);
}

#[test]
fn first_model_and_effort_values_win_and_conflicts_are_deduplicated() {
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
    observer.observe_sse_data(
        br#"{"type":"response.created","response":{"model":"first","reasoning_effort":"high"}}"#,
        "10".to_string(),
    );
    observer.observe_sse_data(
        br#"{"type":"response.completed","response":{"model":"second","reasoning_effort":"low"}}"#,
        "20".to_string(),
    );
    observer.observe_sse_data(
        br#"{"type":"response.completed","response":{"model":"second","reasoning_effort":"low"}}"#,
        "30".to_string(),
    );
    let summary = observer.snapshot();
    assert_eq!(summary.model.effective.as_deref(), Some("first"));
    assert_eq!(summary.reasoning_effort.effective.as_deref(), Some("high"));
    assert_eq!(summary.warnings.len(), 2);
    assert_eq!(summary.warnings[0].kind, "effective_model_conflict");
    assert_eq!(
        summary.warnings[1].kind,
        "effective_reasoning_effort_conflict"
    );
}

#[test]
fn failed_openai_terminal_event_commits_final_usage_and_error() {
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
    observer.observe_sse_data(
        br#"{"type":"response.failed","response":{"usage":{"input_tokens":9,"output_tokens":2},"error":{"type":"server_error","message":"failed"}}}"#,
        "10".to_string(),
    );
    let summary = observer.snapshot();
    assert!(summary.response_terminal);
    assert_eq!(summary.token_usage.unwrap().total_input_tokens, Some(9));
    assert_eq!(summary.errors.len(), 1);
    assert_eq!(summary.errors[0].kind, "server_error");
}

#[test]
fn response_failed_event_with_top_level_error_is_a_provider_error() {
    let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
    observer.observe_sse_event(
        Some(b"response.failed"),
        br#"{"type":"error","error":{"type":"service_unavailable_error","code":"server_error","message":"Our servers are currently overloaded. Please try again later.","param":null},"sequence_number":2}"#,
        "20".to_string(),
    );
    let summary = observer.snapshot();
    assert_eq!(summary.family, ProtocolFamily::OpenaiResponses);
    assert!(summary.response_terminal);
    assert_eq!(summary.errors.len(), 1);
    assert_eq!(summary.errors[0].kind, "service_unavailable_error");
    assert!(summary.errors[0].message.contains("overloaded"));
}

#[test]
fn incomplete_and_cancelled_openai_events_are_terminal_with_final_usage() {
    for (event, expected_error) in [
        (
            br#"{"type":"response.incomplete","response":{"usage":{"input_tokens":5,"output_tokens":1},"incomplete_details":{"reason":"max_output_tokens"}}}"#.as_slice(),
            "response_incomplete",
        ),
        (
            br#"{"type":"response.cancelled","response":{"usage":{"input_tokens":6,"output_tokens":2}}}"#.as_slice(),
            "cancelled",
        ),
    ] {
        let mut observer = ProtocolObserver::new(Some("https://example.test/v1/responses"));
        assert!(observer.observe_sse_data(event, "10".to_string()));
        let summary = observer.snapshot();
        assert!(summary.response_terminal);
        assert!(summary.token_usage.is_some());
        if expected_error == "cancelled" {
            assert!(summary.errors.is_empty());
            assert_eq!(summary.warnings[0].kind, expected_error);
        } else {
            assert_eq!(summary.errors[0].kind, expected_error);
        }
    }
}

#[test]
fn first_token_is_recorded_once_only_for_recognized_protocols() {
    for url in [
        "https://example.test/v1/responses",
        "https://example.test/v1/messages",
    ] {
        let mut observer = ProtocolObserver::new(Some(url));
        assert!(observer.observe_first_token("10".to_string()));
        assert!(!observer.observe_first_token("20".to_string()));
        assert_eq!(observer.snapshot().first_token_at_ns.as_deref(), Some("10"));
    }

    let mut unknown = ProtocolObserver::new(Some("https://example.test/health"));
    assert!(!unknown.observe_first_token("10".to_string()));
    assert!(unknown.snapshot().first_token_at_ns.is_none());
}
