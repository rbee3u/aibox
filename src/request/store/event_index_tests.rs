use super::*;

#[test]
fn live_index_round_trips_through_the_store_reader() {
    let directory = tempfile::tempdir().unwrap();
    let file = std::fs::File::create(directory.path().join(RESPONSE_EVENTS_JSONL)).unwrap();
    let mut indexer = SseIndexer::new(Some(file), "request".to_string());
    let first = b"data: first\n\n";
    let second = b"data: second\n\n";
    indexer.feed(first, 0, "1").unwrap();
    indexer.feed(second, first.len() as u64, "2").unwrap();
    assert!(!indexer.finish().unwrap());

    let rows = EventIndexReader::open(directory.path(), "request", false)
        .unwrap()
        .unwrap()
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    for (index, (_, row)) in rows.into_iter().enumerate() {
        let EventIndexLine::Entry(entry) = row.unwrap() else {
            panic!("writer must produce a valid reader entry");
        };
        assert_eq!(entry.sequence, index as u64);
        assert_eq!(entry.schema_version, FORMAT_VERSION);
        assert_eq!(entry.request_id, "request");
        assert_eq!(
            entry.body_start,
            if index == 0 { 0 } else { first.len() as u64 }
        );
        assert_eq!(
            entry.body_end,
            if index == 0 {
                first.len() as u64
            } else {
                (first.len() + second.len()) as u64
            }
        );
        assert_eq!(entry.completed_at_ns, (index + 1).to_string());
    }
}

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
    assert!(indexer.disabled);

    indexer.feed(second, first.len() as u64, "2").unwrap();
    assert_eq!(
        indexer.take_protocol_events(),
        vec![(None, b"second".to_vec(), "2".to_string())]
    );
    assert!(!indexer.finish().unwrap());
}
