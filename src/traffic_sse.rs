//! Best-effort `text/event-stream` recognition and event indexing.
//!
//! [`SsePrefixSniffer`] classifies a response body from its first few bytes,
//! because a streaming response must be recognized before a declared content
//! type can be trusted. [`SseIndexer`] then parses the byte stream as it is
//! forwarded, appending `response.events.jsonl` entries that point into the
//! unchanged `response.body` rather than copying payloads.
//!
//! Indexing is deliberately subordinate to forwarding: a non-contiguous chunk or
//! a write failure disables it and becomes a Record warning without altering the
//! recorded bytes or the Traffic Outcome. The indexer also notes the first token
//! and the provider terminal event, so a Coding Agent that closes immediately
//! after a complete stream is not recorded as a client disconnect.

use crate::traffic_store::FORMAT_VERSION;
use serde::Serialize;
use std::io::Write as _;

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

pub(crate) type ObservedSseEvent = (Option<Vec<u8>>, Vec<u8>, String);

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
    last_arrival_at_ns: String,
}

impl SseIndexer {
    pub(crate) fn new(file: Option<std::fs::File>, record_id: String) -> Self {
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
        if self.event_start.is_none() && !chunk.is_empty() {
            self.event_start = Some(body_start);
            self.first_arrival_at_ns = Some(at_ns.to_string());
        }
        self.buffer.extend_from_slice(chunk);
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
        self.process(at_ns, false)?;
        if !contiguous {
            return Err(anyhow::anyhow!("SSE body offsets are not contiguous"));
        }
        Ok(())
    }

    fn process(&mut self, at_ns: &str, final_input: bool) -> anyhow::Result<()> {
        let mut consumed = 0usize;
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
                        self.event_name.clone(),
                        self.data.clone(),
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
                    serde_json::to_writer(&mut *file, &entry)?;
                    file.write_all(b"\n")?;
                    file.flush()?;
                    self.sequence = self.sequence.saturating_add(1);
                }
                self.event_start = None;
                self.first_arrival_at_ns = None;
                self.data_seen = false;
                self.event_name = None;
                self.data.clear();
            } else if let Some(value) = sse_field_value(line, b"event") {
                self.event_name = Some(value.to_vec());
            } else if let Some(value) = sse_field_value(line, b"data") {
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
        Ok(())
    }

    pub(crate) fn finish(&mut self) -> anyhow::Result<bool> {
        let last_arrival_at_ns = self.last_arrival_at_ns.clone();
        self.process(&last_arrival_at_ns, true)?;
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
