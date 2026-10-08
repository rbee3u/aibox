//! Pure SSE observation and raw-byte ranges; persistence belongs to Store.
//!
//! [`SsePrefixSniffer`] handles successful recognized streaming responses without
//! Content-Type. Content-encoded bodies require decoded replay and have no live
//! raw-byte index. Non-contiguous chunks disable indexing; an unterminated line or
//! event over 16 MiB stops observation. Neither condition truncates raw recording
//! or forwarding. Terminal events distinguish normal Agent closure from disconnects.

use crate::request::model::ProtocolFamily;

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

/// Location and arrival timing of one dispatchable event in the raw body.
/// Storage attaches the Request identity, sequence, and schema version.
pub(crate) struct SseEventRange {
    pub(crate) body_start: u64,
    pub(crate) body_end: u64,
    pub(crate) first_arrival_at_ns: String,
    pub(crate) completed_at_ns: String,
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
pub(crate) struct SseObserver {
    index_ranges: Vec<SseEventRange>,
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
    terminal_at_ns: Option<String>,
    chat_done_at_ns: Option<String>,
    error_at_ns: Option<String>,
    indexing_disabled: bool,
    observation_disabled: bool,
    max_observation_bytes: usize,
    last_arrival_at_ns: String,
}

impl SseObserver {
    pub(crate) fn new() -> Self {
        Self::with_observation_limit(MAX_SSE_EVENT_OBSERVATION_BYTES)
    }

    fn with_observation_limit(max_observation_bytes: usize) -> Self {
        Self {
            index_ranges: Vec::new(),
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
            terminal_at_ns: None,
            chat_done_at_ns: None,
            error_at_ns: None,
            indexing_disabled: false,
            observation_disabled: false,
            max_observation_bytes,
            last_arrival_at_ns: "0".to_string(),
        }
    }

    pub(crate) fn take_index_ranges(&mut self) -> Vec<SseEventRange> {
        std::mem::take(&mut self.index_ranges)
    }

    pub(crate) fn observation_disabled(&self) -> bool {
        self.observation_disabled
    }

    pub(crate) fn disable_indexing(&mut self) {
        self.indexing_disabled = true;
    }

    pub(crate) fn terminal_seen(&self, family: ProtocolFamily) -> bool {
        self.terminal_at_ns(family).is_some()
    }

    pub(crate) fn terminal_at_ns(&self, family: ProtocolFamily) -> Option<&str> {
        self.terminal_at_ns
            .as_deref()
            .or_else(|| {
                (family == ProtocolFamily::OpenaiChatCompletions)
                    .then_some(self.chat_done_at_ns.as_deref())
                    .flatten()
            })
            .or_else(|| {
                (family != ProtocolFamily::Unknown)
                    .then_some(self.error_at_ns.as_deref())
                    .flatten()
            })
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
                match terminal_sse_event(self.event_name.as_deref(), &self.data) {
                    Some(TerminalSseEvent::Protocol) => {
                        self.terminal_at_ns.get_or_insert_with(|| at_ns.to_string());
                    }
                    Some(TerminalSseEvent::ChatDone) => {
                        self.chat_done_at_ns
                            .get_or_insert_with(|| at_ns.to_string());
                    }
                    Some(TerminalSseEvent::Error) => {
                        self.error_at_ns.get_or_insert_with(|| at_ns.to_string());
                    }
                    None => {}
                }
                if self.data_seen {
                    self.protocol_events.push((
                        self.event_name.take(),
                        std::mem::take(&mut self.data),
                        at_ns.to_string(),
                    ));
                }
                if self.data_seen && !self.indexing_disabled {
                    self.index_ranges.push(SseEventRange {
                        body_start: self.event_start.unwrap_or(self.buffer_start),
                        body_end: absolute_end,
                        first_arrival_at_ns: self
                            .first_arrival_at_ns
                            .clone()
                            .unwrap_or_else(|| at_ns.to_string()),
                        completed_at_ns: at_ns.to_string(),
                    });
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
        // A partial next event belongs to this arrival, even before its first
        // line is complete enough to process.
        if self.event_start.is_none() && !self.buffer.is_empty() {
            self.event_start = Some(self.buffer_start);
            self.first_arrival_at_ns = Some(at_ns.to_string());
        }
        // Everything left is terminator-free except a possible trailing `\r`
        // that must pair with the next chunk's first byte.
        self.scanned = self.buffer.len().saturating_sub(1);
        Ok(())
    }

    /// Process EOF and report an incomplete tail.
    ///
    /// The returned boolean concerns SSE framing only; it does not indicate
    /// whether the response or its model protocol reached a terminal event.
    pub(crate) fn finish(&mut self) -> anyhow::Result<bool> {
        if self.observation_disabled {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalSseEvent {
    Protocol,
    ChatDone,
    Error,
}

fn terminal_sse_event(event_name: Option<&[u8]>, data: &[u8]) -> Option<TerminalSseEvent> {
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
        return Some(TerminalSseEvent::Protocol);
    }
    if event_name == Some(b"error") {
        return Some(TerminalSseEvent::Error);
    }
    if std::str::from_utf8(data).is_ok_and(|value| value.trim() == "[DONE]") {
        return Some(TerminalSseEvent::ChatDone);
    }

    let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) else {
        return None;
    };
    let kind = value.get("type").and_then(serde_json::Value::as_str);
    if matches!(
        kind,
        Some(
            "message_stop"
                | "response.completed"
                | "response.failed"
                | "response.incomplete"
                | "response.cancelled"
        )
    ) {
        return Some(TerminalSseEvent::Protocol);
    }
    if kind == Some("error") || value.get("error").is_some_and(serde_json::Value::is_object) {
        return Some(TerminalSseEvent::Error);
    }
    (kind == Some("message_delta")
        && value
            .get("delta")
            .and_then(|delta| delta.get("stop_reason"))
            .is_some_and(|stop_reason| !stop_reason.is_null()))
    .then_some(TerminalSseEvent::Protocol)
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
#[path = "sse_tests.rs"]
mod tests;
