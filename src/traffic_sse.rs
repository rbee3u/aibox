//! Best-effort `text/event-stream` recognition and event indexing.
//!
//! A declared `text/event-stream` response is recognized from its Content-Type.
//! [`SsePrefixSniffer`] is the fallback for a successful, recognized model
//! request that asked for streaming but received no Content-Type. Identity
//! streams are then parsed by [`SseIndexer`] as they are forwarded; its
//! `response.events.jsonl` entries point into the unchanged `response.body`
//! rather than copying payloads. Content-encoded streams remain opaque.
//!
//! Indexing is deliberately subordinate to forwarding: a non-contiguous chunk or
//! a write failure disables it and becomes a Record warning without altering the
//! recorded bytes or the Traffic Outcome. The indexer also notes the first token
//! and the provider terminal event, so a Coding Agent that closes immediately
//! after a complete stream is not recorded as a client disconnect.
//!
//! Raw body recording remains unbounded, but this in-memory observer stops for
//! the rest of a response when one unterminated line or Event exceeds 16 MiB.
//! This releases buffered data and bounds diagnostic memory without truncating
//! forwarding or the raw Record.

use crate::traffic_store::FORMAT_VERSION;
use serde::Serialize;
use std::io::Write as _;

const MAX_SSE_EVENT_OBSERVATION_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct SsePrefixSniffer {
    prefix: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PrefixSniff {
    Pending,
    EventStream,
    Normal,
}

impl SsePrefixSniffer {
    pub(crate) fn observe(&mut self, chunk: &[u8]) -> PrefixSniff {
        const MAX_PREFIX_LEN: usize = 9;
        let remaining = MAX_PREFIX_LEN.saturating_sub(self.prefix.len());
        self.prefix
            .extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        classify_sse_prefix(&self.prefix)
    }
}

fn classify_sse_prefix(bytes: &[u8]) -> PrefixSniff {
    const BOM: &[u8] = b"\xef\xbb\xbf";
    const SSE_PREFIXES: &[&[u8]] = &[b"event:", b"data:", b"id:", b"retry:", b":"];

    let bytes = if bytes.starts_with(BOM) {
        &bytes[BOM.len()..]
    } else if BOM.starts_with(bytes) {
        return PrefixSniff::Pending;
    } else {
        bytes
    };
    if SSE_PREFIXES.iter().any(|prefix| bytes.starts_with(prefix)) {
        return PrefixSniff::EventStream;
    }
    if SSE_PREFIXES.iter().any(|prefix| prefix.starts_with(bytes)) {
        return PrefixSniff::Pending;
    }
    PrefixSniff::Normal
}

#[derive(Serialize)]
struct SseEventIndexEntry {
    schema_version: u32,
    record_id: String,
    kind: String,
    sequence: u64,
    body_start: u64,
    body_end: u64,
    first_arrival_at_ns: String,
    completed_at_ns: String,
}

/// One complete dispatchable Event observed in the raw stream.
///
/// Tuple fields are the optional `event` value, joined `data` bytes, and the
/// nanosecond offset at which the terminating blank line arrived.
pub(crate) type ObservedSseEvent = (Option<Vec<u8>>, Vec<u8>, String);

/// Incremental observer for one identity-encoded response body.
///
/// Callers feed chunks only after the same bytes have been flushed to the raw
/// Body file. Index failures may stop observation but must not stop recording
/// or forwarding the body.
pub(crate) struct SseIndexer {
    file: Option<std::fs::File>,
    record_id: String,
    buffer: Vec<u8>,
    /// Buffer offset below which no line terminator exists, so feeding one
    /// long line chunk by chunk does not rescan the accumulated prefix.
    scanned: usize,
    buffer_start: u64,
    body_offset: u64,
    event_start: Option<u64>,
    first_arrival_at_ns: Option<String>,
    data_seen: bool,
    event_name: Option<Vec<u8>>,
    data: Vec<u8>,
    protocol_events: Vec<ObservedSseEvent>,
    first_token_seen: bool,
    first_token_at_ns: Option<String>,
    terminal_seen: bool,
    terminal_at_ns: Option<String>,
    sequence: u64,
    indexing_disabled: bool,
    observation_disabled: bool,
    max_observation_bytes: usize,
    last_arrival_at_ns: String,
}

impl SseIndexer {
    pub(crate) fn new(file: Option<std::fs::File>, record_id: String) -> Self {
        Self::with_observation_limit(file, record_id, MAX_SSE_EVENT_OBSERVATION_BYTES)
    }

    fn with_observation_limit(
        file: Option<std::fs::File>,
        record_id: String,
        max_observation_bytes: usize,
    ) -> Self {
        Self {
            file,
            record_id,
            buffer: Vec::new(),
            scanned: 0,
            buffer_start: 0,
            body_offset: 0,
            event_start: None,
            first_arrival_at_ns: None,
            data_seen: false,
            event_name: None,
            data: Vec::new(),
            protocol_events: Vec::new(),
            first_token_seen: false,
            first_token_at_ns: None,
            terminal_seen: false,
            terminal_at_ns: None,
            sequence: 0,
            indexing_disabled: false,
            observation_disabled: false,
            max_observation_bytes,
            last_arrival_at_ns: "0".to_string(),
        }
    }

    pub(crate) fn disable_indexing(&mut self) {
        self.indexing_disabled = true;
    }

    pub(crate) fn terminal_seen(&self) -> bool {
        self.terminal_seen
    }

    pub(crate) fn terminal_at_ns(&self) -> Option<&str> {
        self.terminal_at_ns.as_deref()
    }

    pub(crate) fn body_offset(&self) -> u64 {
        self.body_offset
    }

    pub(crate) fn take_protocol_events(&mut self) -> Vec<ObservedSseEvent> {
        std::mem::take(&mut self.protocol_events)
    }

    pub(crate) fn take_first_token_at_ns(&mut self) -> Option<String> {
        self.first_token_at_ns.take()
    }

    /// Observe the next raw Body chunk at its absolute starting offset.
    ///
    /// `body_start` must equal [`Self::body_offset`]. A mismatch disables the
    /// byte-range index because later entries could no longer address the raw
    /// Body reliably.
    pub(crate) fn feed(
        &mut self,
        chunk: &[u8],
        body_start: u64,
        at_ns: &str,
    ) -> anyhow::Result<()> {
        let contiguous = body_start == self.body_offset;
        if !contiguous {
            self.indexing_disabled = true;
        }
        self.body_offset = self.body_offset.saturating_add(chunk.len() as u64);
        self.last_arrival_at_ns = at_ns.to_string();
        if self.observation_disabled {
            return if contiguous {
                Ok(())
            } else {
                Err(anyhow::anyhow!("SSE body offsets are not contiguous"))
            };
        }

        let mut remaining = chunk;
        let mut chunk_offset = 0usize;
        while !remaining.is_empty() {
            let available = self.max_observation_bytes.saturating_sub(self.buffer.len());
            if available == 0 {
                let error = self.observation_limit_error();
                self.disable_observation();
                return Err(error);
            }
            let take = remaining.len().min(available);
            let part = &remaining[..take];
            if self.event_start.is_none() {
                self.event_start = Some(body_start.saturating_add(chunk_offset as u64));
                self.first_arrival_at_ns = Some(at_ns.to_string());
            }
            self.buffer.extend_from_slice(part);
            if self.buffer_start == 0 && self.buffer.starts_with(&[0xef, 0xbb, 0xbf]) {
                self.buffer.drain(..3);
                self.scanned = self.scanned.saturating_sub(3);
                self.buffer_start = 3;
                if self.buffer.is_empty() {
                    self.event_start = None;
                    self.first_arrival_at_ns = None;
                } else {
                    self.event_start = Some(3);
                    self.first_arrival_at_ns = Some(at_ns.to_string());
                }
            }
            if let Err(error) = self.process(at_ns, false) {
                if self.observation_disabled {
                    self.disable_observation();
                }
                return Err(error);
            }
            remaining = &remaining[take..];
            chunk_offset = chunk_offset.saturating_add(take);
        }
        if !contiguous {
            return Err(anyhow::anyhow!("SSE body offsets are not contiguous"));
        }
        Ok(())
    }

    fn observation_limit_error(&self) -> anyhow::Error {
        anyhow::anyhow!(
            "SSE Event exceeds the {} byte observation limit; Event indexing and protocol interpretation stopped",
            self.max_observation_bytes
        )
    }

    fn disable_observation(&mut self) {
        self.indexing_disabled = true;
        self.observation_disabled = true;
        self.buffer = Vec::new();
        self.scanned = 0;
        self.event_start = None;
        self.first_arrival_at_ns = None;
        self.data_seen = false;
        self.event_name = None;
        self.data = Vec::new();
    }

    fn observed_event_bytes_with(&self, additional: usize) -> Option<usize> {
        self.event_name
            .as_ref()
            .map_or(0, Vec::len)
            .checked_add(self.data.len())?
            .checked_add(additional)
    }

    fn process(&mut self, at_ns: &str, final_input: bool) -> anyhow::Result<()> {
        let mut consumed = 0usize;
        let mut index_error = None;
        loop {
            // A terminator cannot hide below `scanned`, so a line's content
            // may start there while its end is searched further ahead.
            let search_start = consumed.max(self.scanned);
            let Some((line_end, separator_len)) =
                find_sse_line_end(&self.buffer[search_start..], final_input)
            else {
                break;
            };
            let line_end = search_start + line_end;
            let line = &self.buffer[consumed..line_end];
            let absolute_end = self.buffer_start + line_end as u64 + separator_len as u64;
            if self.event_start.is_none() && !line.is_empty() {
                self.event_start = Some(self.buffer_start + consumed as u64);
                self.first_arrival_at_ns = Some(at_ns.to_string());
            }
            if line.is_empty() {
                if is_terminal_sse_event(self.event_name.as_deref(), &self.data) {
                    self.terminal_seen = true;
                    if self.terminal_at_ns.is_none() {
                        self.terminal_at_ns = Some(at_ns.to_string());
                    }
                }
                if self.data_seen {
                    self.protocol_events.push((
                        self.event_name.take(),
                        std::mem::take(&mut self.data),
                        at_ns.to_string(),
                    ));
                }
                if self.data_seen
                    && !self.indexing_disabled
                    && let Some(file) = self.file.as_mut()
                {
                    let entry = SseEventIndexEntry {
                        schema_version: FORMAT_VERSION,
                        record_id: self.record_id.clone(),
                        kind: "sse_event".to_string(),
                        sequence: self.sequence,
                        body_start: self.event_start.unwrap_or(self.buffer_start),
                        body_end: absolute_end,
                        first_arrival_at_ns: self
                            .first_arrival_at_ns
                            .clone()
                            .unwrap_or_else(|| at_ns.to_string()),
                        completed_at_ns: at_ns.to_string(),
                    };
                    let write_result = (|| -> anyhow::Result<()> {
                        serde_json::to_writer(&mut *file, &entry)?;
                        file.write_all(b"\n")?;
                        file.flush()?;
                        Ok(())
                    })();
                    match write_result {
                        Ok(()) => self.sequence = self.sequence.saturating_add(1),
                        Err(error) => {
                            self.indexing_disabled = true;
                            index_error.get_or_insert(error);
                        }
                    }
                }
                self.event_start = None;
                self.first_arrival_at_ns = None;
                self.data_seen = false;
                self.event_name = None;
            } else if let Some(value) = sse_field_value(line, b"event") {
                let additional = value
                    .len()
                    .saturating_sub(self.event_name.as_ref().map_or(0, Vec::len));
                if self
                    .observed_event_bytes_with(additional)
                    .is_none_or(|bytes| bytes > self.max_observation_bytes)
                {
                    self.observation_disabled = true;
                    return Err(self.observation_limit_error());
                }
                self.event_name = Some(value.to_vec());
            } else if let Some(value) = sse_field_value(line, b"data") {
                let additional = value.len() + usize::from(self.data_seen);
                if self
                    .observed_event_bytes_with(additional)
                    .is_none_or(|bytes| bytes > self.max_observation_bytes)
                {
                    self.observation_disabled = true;
                    return Err(self.observation_limit_error());
                }
                if !self.first_token_seen && is_first_token_data(value) {
                    self.first_token_seen = true;
                    self.first_token_at_ns = Some(at_ns.to_string());
                }
                if self.data_seen {
                    self.data.push(b'\n');
                }
                self.data.extend_from_slice(value);
                self.data_seen = true;
            }
            consumed = line_end + separator_len;
        }
        if consumed > 0 {
            self.buffer.drain(..consumed);
            self.buffer_start += consumed as u64;
        }
        // Everything left is terminator-free except a possible trailing `\r`
        // that must pair with the next chunk's first byte.
        self.scanned = self.buffer.len().saturating_sub(1);
        match index_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    /// Process EOF, sync the optional index, and report an incomplete tail.
    ///
    /// The returned boolean concerns SSE framing only; it does not indicate
    /// whether the response or its model protocol reached a terminal event.
    pub(crate) fn finish(&mut self) -> anyhow::Result<bool> {
        if self.observation_disabled {
            if let Some(file) = self.file.as_mut() {
                file.sync_all()?;
            }
            return Ok(false);
        }
        let last_arrival_at_ns = self.last_arrival_at_ns.clone();
        if let Err(error) = self.process(&last_arrival_at_ns, true) {
            if self.observation_disabled {
                self.disable_observation();
            }
            return Err(error);
        }
        if self.indexing_disabled {
            return Ok(false);
        }
        if let Some(file) = self.file.as_mut() {
            file.sync_all()?;
        }
        Ok(self.event_start.is_some() || !self.buffer.is_empty())
    }
}

pub(crate) fn is_first_token_data(value: &[u8]) -> bool {
    match std::str::from_utf8(value) {
        Ok(value) => {
            let value = value.trim();
            !value.is_empty() && !value.starts_with("[DONE]")
        }
        Err(_) => true,
    }
}

fn sse_field_value<'a>(line: &'a [u8], field: &[u8]) -> Option<&'a [u8]> {
    if line == field {
        return Some(&[]);
    }
    let value = line.strip_prefix(field)?.strip_prefix(b":")?;
    Some(value.strip_prefix(b" ").unwrap_or(value))
}

fn is_terminal_sse_event(event_name: Option<&[u8]>, data: &[u8]) -> bool {
    if matches!(
        event_name,
        Some(
            b"message_stop"
                | b"response.completed"
                | b"response.failed"
                | b"response.incomplete"
                | b"response.cancelled"
        )
    ) {
        return true;
    }

    let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) else {
        return false;
    };
    let Some(kind) = value.get("type").and_then(serde_json::Value::as_str) else {
        return false;
    };
    if matches!(
        kind,
        "message_stop"
            | "response.completed"
            | "response.failed"
            | "response.incomplete"
            | "response.cancelled"
    ) {
        return true;
    }
    kind == "message_delta"
        && value
            .get("delta")
            .and_then(|delta| delta.get("stop_reason"))
            .is_some_and(|stop_reason| !stop_reason.is_null())
}

fn find_sse_line_end(bytes: &[u8], final_input: bool) -> Option<(usize, usize)> {
    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'\n' => return Some((index, 1)),
            b'\r' => {
                if index + 1 == bytes.len() {
                    return final_input.then_some((index, 1));
                }
                return Some((index, usize::from(bytes[index + 1] == b'\n') + 1));
            }
            _ => {}
        }
    }
    (final_input && !bytes.is_empty()).then_some((bytes.len(), 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_write_failure_does_not_replay_the_event_on_the_next_chunk() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("response.events.jsonl");
        std::fs::write(&path, []).unwrap();
        let read_only = std::fs::File::open(path).unwrap();
        let mut indexer = SseIndexer::new(Some(read_only), "record".to_string());
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
        let mut indexer = SseIndexer::with_observation_limit(None, "record".to_string(), 16);
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
        let mut indexer = SseIndexer::with_observation_limit(None, "record".to_string(), 12);
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
        let mut indexer = SseIndexer::with_observation_limit(None, "record".to_string(), 12);
        let chunk = b"data: 1234\ndata: 5678\ndata: 90\n\ndata: next\n\n";

        indexer.feed(chunk, 0, "1").unwrap();

        let events = indexer.take_protocol_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].1, b"1234\n5678\n90");
        assert_eq!(events[1].1, b"next");
        assert!(!indexer.observation_disabled);
    }

    #[test]
    fn completed_events_remain_observable_when_a_later_event_exceeds_the_limit() {
        let mut indexer = SseIndexer::with_observation_limit(None, "record".to_string(), 12);
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
        let mut indexer = SseIndexer::with_observation_limit(None, "record".to_string(), 12);
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
}
