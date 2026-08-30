use super::*;

#[test]
fn index_write_failure_does_not_replay_the_event_on_the_next_chunk() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("response.events.jsonl");
    std::fs::write(&path, []).unwrap();
    let read_only = std::fs::File::open(path).unwrap();
    let mut indexer = SseIndexer::new(Some(read_only), "request".to_string());
    let first = b"data: first\n\n";
    let second = b"data: second\n\n";

    assert!(indexer.feed(first, 0, "1").is_err());
    assert_eq!(
        indexer.take_protocol_events(),
        vec![(None, b"first".to_vec(), "1".to_string())]
    );
    assert!(indexer.indexing_disabled);

    indexer.feed(second, first.len() as u64, "2").unwrap();
    assert_eq!(
        indexer.take_protocol_events(),
        vec![(None, b"second".to_vec(), "2".to_string())]
    );
    assert!(!indexer.finish().unwrap());
}

#[test]
fn oversized_sse_line_stops_observation_without_retaining_the_body() {
    let mut indexer = SseIndexer::with_observation_limit(None, "request".to_string(), 16);
    let chunk = b"data: 01234567890";

    let error = indexer.feed(chunk, 0, "1").unwrap_err().to_string();

    assert!(error.contains("16 byte observation limit"), "{error}");
    assert!(indexer.observation_disabled);
    assert!(indexer.buffer.is_empty());
    assert!(indexer.data.is_empty());
    assert_eq!(indexer.body_offset(), chunk.len() as u64);

    indexer
        .feed(b"data: ignored\n\n", chunk.len() as u64, "2")
        .unwrap();
    assert!(indexer.take_protocol_events().is_empty());
    assert!(!indexer.finish().unwrap());
}

#[test]
fn oversized_multiline_sse_event_stops_observation() {
    let mut indexer = SseIndexer::with_observation_limit(None, "request".to_string(), 12);
    let chunks: [&[u8]; 4] = [
        b"data: 1234\n",
        b"data: 5678\n",
        b"data: 90\n",
        b"data: x\n",
    ];
    let mut offset = 0u64;

    for chunk in &chunks[..3] {
        indexer.feed(chunk, offset, "1").unwrap();
        offset += chunk.len() as u64;
    }
    let error = indexer
        .feed(chunks[3], offset, "2")
        .unwrap_err()
        .to_string();

    assert!(error.contains("12 byte observation limit"), "{error}");
    assert!(indexer.observation_disabled);
    assert!(indexer.buffer.is_empty());
    assert!(indexer.data.is_empty());
    assert!(indexer.take_protocol_events().is_empty());
}

#[test]
fn observation_limit_accepts_the_exact_boundary_and_resets_per_event() {
    let mut indexer = SseIndexer::with_observation_limit(None, "request".to_string(), 12);
    let chunk = b"data: 1234\ndata: 5678\ndata: 90\n\ndata: next\n\n";

    indexer.feed(chunk, 0, "1").unwrap();

    let events = indexer.take_protocol_events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].1, b"1234\n5678\n90");
    assert_eq!(events[1].1, b"next");
    assert!(!indexer.observation_disabled);
}

#[test]
fn done_is_terminal_only_for_chat_completions() {
    let mut indexer = SseIndexer::new(None, "request".to_string());
    let body = b"data:  \t[DONE] \r\n\r\n";

    indexer.feed(body, 0, "7").unwrap();

    assert!(indexer.terminal_seen(ProtocolFamily::OpenaiChatCompletions));
    assert_eq!(
        indexer.terminal_at_ns(ProtocolFamily::OpenaiChatCompletions),
        Some("7")
    );
    assert!(!indexer.terminal_seen(ProtocolFamily::OpenaiResponses));
    assert!(!indexer.terminal_seen(ProtocolFamily::ClaudeMessages));
    assert!(!indexer.terminal_seen(ProtocolFamily::Unknown));
    assert_eq!(indexer.take_protocol_events()[0].1, b" \t[DONE] ");
}

#[test]
fn error_event_is_terminal_for_a_recognized_family_only() {
    let mut indexer = SseIndexer::new(None, "request".to_string());
    let body = b"data: {\"error\":{\"type\":\"server_error\"}}\n\n";

    indexer.feed(body, 0, "9").unwrap();

    assert!(indexer.terminal_seen(ProtocolFamily::OpenaiChatCompletions));
    assert!(!indexer.terminal_seen(ProtocolFamily::Unknown));
}

#[test]
fn completed_events_remain_observable_when_a_later_event_exceeds_the_limit() {
    let mut indexer = SseIndexer::with_observation_limit(None, "request".to_string(), 12);
    let chunk = b"data: ok\n\ndata: 01234567890";

    let error = indexer.feed(chunk, 0, "7").unwrap_err().to_string();

    assert!(error.contains("12 byte observation limit"), "{error}");
    assert_eq!(
        indexer.take_protocol_events(),
        vec![(None, b"ok".to_vec(), "7".to_string())]
    );
    assert!(indexer.observation_disabled);
    assert_eq!(indexer.body_offset(), chunk.len() as u64);
}

#[test]
fn eof_enforces_the_accumulated_event_limit_without_emitting_a_partial_event() {
    let mut indexer = SseIndexer::with_observation_limit(None, "request".to_string(), 12);
    let terminated = b"data: 1234\ndata: 5678\n";
    let unterminated = b"data: 123";

    indexer.feed(terminated, 0, "1").unwrap();
    indexer
        .feed(unterminated, terminated.len() as u64, "2")
        .unwrap();
    let error = indexer.finish().unwrap_err().to_string();

    assert!(error.contains("12 byte observation limit"), "{error}");
    assert!(indexer.observation_disabled);
    assert!(indexer.take_protocol_events().is_empty());
    assert_eq!(
        indexer.body_offset(),
        (terminated.len() + unterminated.len()) as u64
    );
    assert!(!indexer.finish().unwrap());
}
