//! SSE index persistence and streaming reads. Readers report structural damage;
//! callers choose diagnostic wording.

use super::{FORMAT_VERSION, RESPONSE_EVENTS_JSONL};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::BufRead;
use std::path::Path;

#[derive(Deserialize, Serialize)]
pub(super) struct EventIndexEntry {
    schema_version: u32,
    request_id: String,
    kind: String,
    pub(super) sequence: u64,
    body_start: u64,
    body_end: u64,
    first_arrival_at_ns: String,
    pub(super) completed_at_ns: String,
}

impl EventIndexEntry {
    fn valid(&self, request_id: &str) -> bool {
        self.schema_version == FORMAT_VERSION
            && self.request_id == request_id
            && self.kind == "sse_event"
            && self.body_start <= self.body_end
            && self.first_arrival_at_ns.parse::<u128>().is_ok()
            && self.completed_at_ns.parse::<u128>().is_ok()
    }
}

/// One usable line, or the reason this line yields no timing.
pub(super) enum EventIndexLine {
    Entry(EventIndexEntry),
    InvalidMetadata,
    Unparsable(serde_json::Error),
}

/// Skips blank lines and an active Request's unfinished tail. Every outcome,
/// including read failures, carries its 1-based line number.
pub(super) struct EventIndexReader {
    reader: std::io::BufReader<std::fs::File>,
    request_id: String,
    active: bool,
    lines_read: usize,
}

impl EventIndexReader {
    /// Opens the index beside a Request, or reports its absence as `None`.
    pub(super) fn open(directory: &Path, request_id: &str, active: bool) -> Result<Option<Self>> {
        let path = directory.join(RESPONSE_EVENTS_JSONL);
        if !crate::foundation::safe_fs::real_file_exists(&path, "Request SSE event index")? {
            return Ok(None);
        }
        let file = crate::foundation::safe_fs::open_real_file(&path, "Request SSE event index")?;
        Ok(Some(Self {
            reader: std::io::BufReader::new(file),
            request_id: request_id.to_string(),
            active,
            lines_read: 0,
        }))
    }
}

impl Iterator for EventIndexReader {
    type Item = (usize, std::io::Result<EventIndexLine>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let mut line = Vec::new();
            let read = match self.reader.read_until(b'\n', &mut line) {
                Ok(read) => read,
                Err(error) => return Some((self.lines_read + 1, Err(error))),
            };
            if read == 0 {
                return None;
            }
            self.lines_read += 1;
            if line.last() == Some(&b'\n') {
                line.pop();
            } else if self.active {
                return None;
            }
            if line.is_empty() {
                continue;
            }
            let outcome = match serde_json::from_slice::<EventIndexEntry>(&line) {
                Ok(entry) if entry.valid(&self.request_id) => EventIndexLine::Entry(entry),
                Ok(_) => EventIndexLine::InvalidMetadata,
                Err(error) => EventIndexLine::Unparsable(error),
            };
            return Some((self.lines_read, Ok(outcome)));
        }
    }
}

/// Live best-effort event index persistence, composed with the pure observer.
pub(crate) struct SseIndexer {
    observer: crate::request::sse::SseObserver,
    file: Option<std::fs::File>,
    request_id: String,
    sequence: u64,
    disabled: bool,
}
impl SseIndexer {
    pub(crate) fn new(file: Option<std::fs::File>, request_id: String) -> Self {
        Self {
            observer: crate::request::sse::SseObserver::new(),
            file,
            request_id,
            sequence: 0,
            disabled: false,
        }
    }
    pub(crate) fn feed(
        &mut self,
        chunk: &[u8],
        body_start: u64,
        at_ns: &str,
    ) -> anyhow::Result<()> {
        let observed = self.observer.feed(chunk, body_start, at_ns);
        let written = self.write_ranges();
        observed.and(written)
    }
    fn write_ranges(&mut self) -> anyhow::Result<()> {
        use std::io::Write as _;
        let mut first_error = None;
        for range in self.observer.take_index_ranges() {
            if self.disabled {
                continue;
            }
            let Some(file) = self.file.as_mut() else {
                continue;
            };
            let entry = EventIndexEntry {
                schema_version: FORMAT_VERSION,
                request_id: self.request_id.clone(),
                kind: "sse_event".to_string(),
                sequence: self.sequence,
                body_start: range.body_start,
                body_end: range.body_end,
                first_arrival_at_ns: range.first_arrival_at_ns,
                completed_at_ns: range.completed_at_ns,
            };
            let result = (|| -> anyhow::Result<()> {
                serde_json::to_writer(&mut *file, &entry)?;
                file.write_all(b"\n")?;
                file.flush()?;
                Ok(())
            })();
            match result {
                Ok(()) => self.sequence = self.sequence.saturating_add(1),
                Err(error) => {
                    self.disabled = true;
                    self.observer.disable_indexing();
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
    pub(crate) fn finish(&mut self) -> anyhow::Result<bool> {
        let stopped = self.observer.observation_disabled();
        let incomplete = self.observer.finish()?;
        self.write_ranges()?;
        if (!self.disabled || stopped)
            && let Some(file) = self.file.as_mut()
        {
            file.sync_all()?;
        }
        Ok(incomplete)
    }
    pub(crate) fn disable_indexing(&mut self) {
        self.disabled = true;
        self.observer.disable_indexing();
    }
    pub(crate) fn body_offset(&self) -> u64 {
        self.observer.body_offset()
    }
    pub(crate) fn take_protocol_events(&mut self) -> Vec<crate::request::sse::ObservedSseEvent> {
        self.observer.take_protocol_events()
    }
    pub(crate) fn take_first_token_at_ns(&mut self) -> Option<String> {
        self.observer.take_first_token_at_ns()
    }
    pub(crate) fn terminal_at_ns(
        &self,
        family: crate::request::model::ProtocolFamily,
    ) -> Option<&str> {
        self.observer.terminal_at_ns(family)
    }
}

#[cfg(test)]
#[path = "event_index_tests.rs"]
mod tests;
