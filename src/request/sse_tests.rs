use super::*;
use proptest::prelude::*;

proptest! {
    #[test]
    fn generated_events_preserve_content_and_raw_ranges_across_chunk_partitions(
        events in proptest::collection::vec(
            (
                proptest::option::of("[a-z]{1,12}"),
                proptest::collection::vec("[^\r\n]{0,24}", 1..5),
                prop::sample::select(vec!["\n", "\r", "\r\n"]),
            ),
            1..8,
        ),
        bom in any::<bool>(),
        incomplete_tail in any::<bool>(),
        cuts in proptest::collection::vec(any::<u16>(), 0..64),
    ) {
        let mut body = if bom { "\u{feff}" } else { "" }.to_string();
        let mut expected_events = Vec::new();
        let mut expected_ranges = Vec::new();
        // Derive the oracle from generated records, without parsing the body or
        // using a second SseObserver as the expected result.
        for (name, lines, newline) in events {
            let start = body.len() as u64;
            if let Some(name) = &name {
                body.push_str(&format!("event: {name}{newline}"));
            }
            for line in &lines {
                body.push_str(&format!("data: {line}{newline}"));
            }
            body.push_str(newline);
            expected_events.push((
                name.map(String::into_bytes),
                lines.join("\n").into_bytes(),
                "7".to_string(),
            ));
            expected_ranges.push((start, body.len() as u64));
        }
        if incomplete_tail {
            body.push_str("data: unfinished");
        }

        let bytes = body.as_bytes();
        let mut ends: Vec<_> = cuts.into_iter().map(|cut| usize::from(cut) % (bytes.len() + 1)).collect();
        ends.push(bytes.len());
        ends.sort_unstable();
        let mut observer = SseObserver::new();
        let mut start = 0;
        for end in ends {
            // Arrival-time behavior has separate deterministic scenarios.
            observer.feed(&bytes[start..end], start as u64, "7").unwrap();
            start = end;
        }

        prop_assert_eq!(observer.finish().unwrap(), incomplete_tail);
        prop_assert_eq!(observer.body_offset(), bytes.len() as u64);
        prop_assert_eq!(observer.take_protocol_events(), expected_events);
        let ranges: Vec<_> = observer.take_index_ranges().into_iter()
            .map(|range| (range.body_start, range.body_end))
            .collect();
        prop_assert_eq!(ranges, expected_ranges);
    }
}

#[test]
fn prefix_detection_is_independent_of_chunk_boundaries() {
    for (body, expected) in [
        (
            b"\xef\xbb\xbfevent: message\n".as_slice(),
            PrefixSniff::EventStream,
        ),
        (b"data: hello", PrefixSniff::EventStream),
        (b"id: 1", PrefixSniff::EventStream),
        (b"retry: 1000", PrefixSniff::EventStream),
        (b": comment", PrefixSniff::EventStream),
        (br#"{"object":"response"}"#, PrefixSniff::Normal),
        (b"database:", PrefixSniff::Normal),
        (b"", PrefixSniff::Pending),
        (b"\xef\xbb", PrefixSniff::Pending),
        (b"eve", PrefixSniff::Pending),
    ] {
        for split in 0..=body.len() {
            let mut sniffer = SsePrefixSniffer::default();
            sniffer.observe(&body[..split]);
            assert_eq!(
                sniffer.observe(&body[split..]),
                expected,
                "body={body:?}, split={split}"
            );
        }
    }
}

#[test]
fn event_content_ranges_and_eof_are_independent_of_chunk_boundaries() {
    for newline in ["\n", "\r", "\r\n"] {
        let first = format!(
            "\u{feff}: comment{newline}event: message{newline}data: 你好{newline}data: 😀{newline}{newline}"
        );
        let second = format!("data: [DONE]{newline}{newline}");
        for tail in ["", "data: pending"] {
            let body = format!("{first}{second}{tail}");
            let bytes = body.as_bytes();
            let partitions = (0..=bytes.len())
                .map(|split| vec![split, bytes.len()])
                .chain(std::iter::once((1..=bytes.len()).collect()));
            for ends in partitions {
                let context = format!("newline={newline:?}, tail={tail:?}, chunk ends={ends:?}");
                let mut observer = SseObserver::new();
                let mut start = 0;
                for end in ends {
                    // Timing has its own tests: this property keeps arrivals fixed.
                    observer
                        .feed(&bytes[start..end], start as u64, "7")
                        .unwrap();
                    start = end;
                }
                assert_eq!(observer.finish().unwrap(), !tail.is_empty(), "{context}");
                assert_eq!(observer.body_offset(), bytes.len() as u64, "{context}");
                assert_eq!(
                    observer.take_protocol_events(),
                    vec![
                        (
                            Some(b"message".to_vec()),
                            "你好\n😀".as_bytes().to_vec(),
                            "7".to_string()
                        ),
                        (None, b"[DONE]".to_vec(), "7".to_string()),
                    ],
                    "{context}"
                );
                let ranges: Vec<_> = observer
                    .take_index_ranges()
                    .into_iter()
                    .map(|range| (range.body_start, range.body_end))
                    .collect();
                assert_eq!(
                    ranges,
                    vec![
                        (3, first.len() as u64),
                        (first.len() as u64, (first.len() + second.len()) as u64)
                    ],
                    "{context}"
                );
                assert_eq!(
                    observer.take_first_token_at_ns().as_deref(),
                    Some("7"),
                    "{context}"
                );
                assert_eq!(
                    observer.terminal_at_ns(ProtocolFamily::OpenaiChatCompletions),
                    Some("7"),
                    "{context}"
                );
            }
        }
    }
}

#[test]
fn event_ranges_track_raw_bytes_and_arrivals_without_storage() {
    let mut observer = SseObserver::new();
    let first = b"event: message\r\ndata: fir";
    let second = b"st\r\n\r\ndata: sec";
    let third = b"ond\n\n";
    observer.feed(first, 0, "1").unwrap();
    assert!(observer.take_index_ranges().is_empty());
    observer.feed(second, first.len() as u64, "2").unwrap();
    observer
        .feed(third, (first.len() + second.len()) as u64, "3")
        .unwrap();

    let ranges = observer.take_index_ranges();
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].body_start, 0);
    assert_eq!(ranges[0].body_end, (first.len() + 6) as u64);
    assert_eq!(ranges[0].first_arrival_at_ns, "1");
    assert_eq!(ranges[0].completed_at_ns, "2");
    assert_eq!(ranges[1].body_start, (first.len() + 6) as u64);
    assert_eq!(
        ranges[1].body_end,
        (first.len() + second.len() + third.len()) as u64
    );
    assert_eq!(ranges[1].first_arrival_at_ns, "2");
    assert_eq!(ranges[1].completed_at_ns, "3");
    assert!(observer.take_index_ranges().is_empty());
    assert_eq!(
        observer.take_protocol_events(),
        vec![
            (
                Some(b"message".to_vec()),
                b"first".to_vec(),
                "2".to_string()
            ),
            (None, b"second".to_vec(), "3".to_string()),
        ]
    );
}

#[test]
fn oversized_sse_line_stops_observation_without_retaining_the_body() {
    let mut indexer = SseObserver::with_observation_limit(16);
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
    let mut indexer = SseObserver::with_observation_limit(12);
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
    let mut indexer = SseObserver::with_observation_limit(12);
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
    let mut indexer = SseObserver::new();
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
    let mut indexer = SseObserver::new();
    let body = b"data: {\"error\":{\"type\":\"server_error\"}}\n\n";

    indexer.feed(body, 0, "9").unwrap();

    assert!(indexer.terminal_seen(ProtocolFamily::OpenaiChatCompletions));
    assert!(!indexer.terminal_seen(ProtocolFamily::Unknown));
}

#[test]
fn completed_events_remain_observable_when_a_later_event_exceeds_the_limit() {
    let mut indexer = SseObserver::with_observation_limit(12);
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
    let mut indexer = SseObserver::with_observation_limit(12);
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

#[test]
fn first_token_data_matches_relay_line_filtering() {
    assert!(!is_first_token_data(b""));
    assert!(!is_first_token_data(b" \t\r\n"));
    assert!(!is_first_token_data("\u{00a0}".as_bytes()));
    assert!(!is_first_token_data(b" [DONE]"));
    assert!(!is_first_token_data(b"[DONE] trailing relay text"));
    assert!(is_first_token_data(b"ping"));
    assert!(is_first_token_data(b"{"));
    assert!(is_first_token_data(b"\xff"));
}

#[test]
fn sse_first_token_counts_any_eligible_data_line_and_never_overwrites_it() {
    let ignored = b"\xef\xbb\xbf: comment\nevent: response.created\r\ndata:\rdata: \t \ndata: [DONE] trailing\r\n";
    let mut indexer = SseObserver::new();
    indexer.feed(ignored, 0, "1").unwrap();
    assert!(indexer.take_first_token_at_ns().is_none());

    let message_start = b"data:\ndata: {\"type\":\"message_start\"}\n\n";
    indexer
        .feed(message_start, ignored.len() as u64, "2")
        .unwrap();
    assert_eq!(indexer.take_first_token_at_ns().as_deref(), Some("2"));

    indexer
        .feed(
            b"data: ping\n\n",
            (ignored.len() + message_start.len()) as u64,
            "3",
        )
        .unwrap();
    assert!(indexer.take_first_token_at_ns().is_none());
}

#[test]
fn sse_first_token_accepts_relay_compatible_non_output_data() {
    for line in [
        b"data: ping\n".as_slice(),
        b"data: {\"type\":\"error\",\"error\":{}}\n".as_slice(),
        b"data: {malformed json\n".as_slice(),
        b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"\"}\n".as_slice(),
        b"data: {\"type\":\"response.created\"}\n".as_slice(),
    ] {
        let mut indexer = SseObserver::new();
        indexer.feed(line, 0, "7").unwrap();
        assert_eq!(indexer.take_first_token_at_ns().as_deref(), Some("7"));
    }
}

#[test]
fn sse_first_token_uses_line_completion_and_eof_arrival_times() {
    let mut indexer = SseObserver::new();
    let first = b"\xef\xbb\xbfdata: {\"type\":\"response.created\"}";
    indexer.feed(first, 0, "1").unwrap();
    assert!(indexer.take_first_token_at_ns().is_none());
    indexer.feed(b"\r", first.len() as u64, "2").unwrap();
    assert!(indexer.take_first_token_at_ns().is_none());
    indexer.feed(b"\n", (first.len() + 1) as u64, "3").unwrap();
    assert_eq!(indexer.take_first_token_at_ns().as_deref(), Some("3"));

    let mut eof = SseObserver::new();
    eof.feed(b"data: ping", 0, "8").unwrap();
    assert!(eof.take_first_token_at_ns().is_none());
    assert!(eof.finish().unwrap());
    assert_eq!(eof.take_first_token_at_ns().as_deref(), Some("8"));
}
