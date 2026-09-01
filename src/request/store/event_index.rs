//! Reading the SSE Event timing index one line at a time.
//!
//! Two readers walk this index: the timing view a Console body panel polls, and
//! the diagnostic pass that folds index damage into a Request detail's warnings.
//! They report damage in their own words, so this module owns only the decode
//! and the walk, leaving each caller its own message wording.

use super::{FORMAT_VERSION, RESPONSE_EVENTS_JSONL};
use anyhow::Result;
use serde::Deserialize;
use std::io::BufRead;
use std::path::Path;

#[derive(Deserialize)]
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

/// Walks the event index, skipping blank lines and stopping at the first line an
/// active Request has not finished writing.
///
/// Yields the 1-based line number alongside each outcome so a caller can name the
/// damaged line. A read failure carries the number of the line it was reading.
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
