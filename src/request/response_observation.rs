//! Pure response evidence plus replay of an already-recorded response body.

use crate::request::interpretation::{BodyContentCoding, body_reader};
use crate::request::model::ProtocolFamily;
use crate::request::sse::{ObservedSseEvent, SseIndexer};
use anyhow::{Context, Result, bail};
use std::fs::File;
use std::io::Read as _;

/// Evidence extracted from response bytes without owning Request lifecycle state.
pub(crate) struct ResponseObservation {
    pub(crate) events: Vec<ObservedSseEvent>,
    pub(crate) terminal_seen: bool,
    pub(crate) warning: Option<String>,
}

/// Replay a complete recorded content-coded SSE body after upstream EOF.
pub(crate) fn replay_complete_encoded_sse(
    file: File,
    coding: BodyContentCoding,
    request_id: String,
    at_ns: &str,
) -> Result<ResponseObservation> {
    if !coding.is_encoded() {
        bail!("identity SSE should use the live indexer");
    }
    let mut decoder = body_reader(file, coding).context("create encoded response decoder")?;
    let mut indexer = SseIndexer::new(None, request_id);
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = decoder
            .read(&mut buffer)
            .context("decode encoded response body")?;
        if read == 0 {
            break;
        }
        indexer.feed(&buffer[..read], indexer.body_offset(), at_ns)?;
    }
    let warning = indexer.finish().err().map(|error| error.to_string());
    Ok(ResponseObservation {
        events: indexer.take_protocol_events(),
        terminal_seen: false,
        warning,
    })
}

/// Replay the decodable prefix of a recorded content-coded SSE body after client close.
///
/// A close can truncate the final compressed frame. Decode and parser errors
/// therefore stop replay without discarding terminal evidence already seen.
pub(crate) fn replay_encoded_sse_prefix(
    file: File,
    coding: BodyContentCoding,
    request_id: String,
    family: ProtocolFamily,
) -> Result<ResponseObservation> {
    if !coding.is_encoded() {
        bail!("identity SSE should use the live indexer");
    }
    let mut decoder = body_reader(file, coding).context("create encoded response decoder")?;
    let mut indexer = SseIndexer::new(None, request_id);
    let mut buffer = [0u8; 16 * 1024];
    while let Ok(read) = decoder.read(&mut buffer) {
        if read == 0 {
            break;
        }
        if indexer
            .feed(&buffer[..read], indexer.body_offset(), "0")
            .is_err()
            || indexer.terminal_seen(family)
        {
            break;
        }
    }
    let _ = indexer.finish();
    Ok(ResponseObservation {
        terminal_seen: indexer.terminal_seen(family),
        events: indexer.take_protocol_events(),
        warning: None,
    })
}
