//! Session detail streaming and Transcript evidence access.

use super::backend::SessionBackend;
use super::catalog::resolve;
use super::filesystem::{MAX_TRANSCRIPT_LINE_BYTES, open_session_transcript, safe_path};
use super::model::{
    DetailRecord, EvidenceEncoding, SessionDetailMeta, SessionDetailStats, ToolActivity,
    ToolActivityStatus, TranscriptEvidence, TranscriptEvidenceSummary, bounded_preview, ts_of,
};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

fn snapshot_for_metadata(metadata: &fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    format!("{}:{modified}", metadata.len())
}

fn observed_duration_ms(start: &str, end: &str) -> Option<i64> {
    let start = OffsetDateTime::parse(start, &Rfc3339).ok()?;
    let end = OffsetDateTime::parse(end, &Rfc3339).ok()?;
    let duration = end - start;
    (duration.is_positive() || duration.is_zero()).then(|| duration.whole_milliseconds() as i64)
}

fn detail_entry_id(line: u64) -> String {
    format!("line-{line}")
}

#[cfg(test)]
pub(crate) fn detail_records_for_test(
    backend: &dyn SessionBackend,
    home: &Path,
    query: &str,
) -> Result<Vec<DetailRecord>> {
    let mut records = Vec::new();
    stream_detail_data(backend, home, query, &mut |_| Ok(true), &mut |record| {
        records.push(record);
        Ok(true)
    })?;
    Ok(records)
}

fn detail_file_path(backend: &dyn SessionBackend, home: &Path, query: &str) -> Result<PathBuf> {
    resolve(backend, home, query)
}

fn detail_meta(
    backend: &dyn SessionBackend,
    home: &Path,
    path: &Path,
    id: &str,
) -> Result<SessionDetailMeta> {
    let summary = backend.summarize_in(home, path)?;
    Ok(SessionDetailMeta {
        id: id.to_string(),
        title: summary.title,
        start_ts: summary.start_ts,
        transcript_path: path
            .strip_prefix(home)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string(),
        cwd: summary.native_facts.cwd,
        model_provider: summary.native_facts.model_provider,
        cli_version: summary.native_facts.cli_version,
    })
}

pub(crate) fn stream_detail_data(
    backend: &dyn SessionBackend,
    home: &Path,
    query: &str,
    begin: &mut dyn FnMut(&SessionDetailMeta) -> Result<bool>,
    visit: &mut dyn FnMut(DetailRecord) -> Result<bool>,
) -> Result<(SessionDetailMeta, SessionDetailStats, Vec<String>)> {
    let path = detail_file_path(backend, home, query)?;
    let id = backend.id_of(&path);
    let meta = detail_meta(backend, home, &path, &id)?;
    if !begin(&meta)? {
        return Ok((meta, SessionDetailStats::default(), Vec::new()));
    }
    let file = open_session_transcript(home, &path)?;
    let metadata = file
        .metadata()
        .with_context(|| format!("inspect session transcript {}", safe_path(&path)))?;
    let snapshot = snapshot_for_metadata(&metadata);
    let file_size = metadata.len();
    let mut stats = SessionDetailStats {
        start_ts: meta.start_ts.clone(),
        file_size,
        snapshot,
        ..SessionDetailStats::default()
    };
    let mut warnings = Vec::new();
    let mut reader = io::BufReader::new(file);
    let mut line = Vec::new();
    let mut line_number = 0_u64;
    let mut pending_tools = HashMap::<String, ToolActivity>::new();
    loop {
        line.clear();
        let read = (&mut reader)
            .take(MAX_TRANSCRIPT_LINE_BYTES.saturating_add(2))
            .read_until(b'\n', &mut line)
            .with_context(|| format!("read session transcript {}", safe_path(&path)))?;
        if read == 0 {
            break;
        }
        line_number += 1;
        let record_id = detail_entry_id(line_number);
        let record = line.strip_suffix(b"\n").unwrap_or(&line);
        let record = record.strip_suffix(b"\r").unwrap_or(record);
        if record.len() as u64 > MAX_TRANSCRIPT_LINE_BYTES {
            bail!(
                "session transcript line {} exceeds the {} byte limit: {}",
                line_number,
                MAX_TRANSCRIPT_LINE_BYTES,
                safe_path(&path)
            );
        }
        stats.entry_count += 1;
        let value = match serde_json::from_slice::<Value>(record) {
            Ok(value) => value,
            Err(error) => {
                stats.malformed_count += 1;
                warnings.push(format!("line {line_number}: malformed JSONL ({error})"));
                if !visit(DetailRecord::Evidence(TranscriptEvidenceSummary {
                    entry_id: record_id,
                    line: line_number,
                    timestamp: String::new(),
                    native_type: "malformed".to_string(),
                    role: None,
                    content_types: Vec::new(),
                    status: "malformed".to_string(),
                    preview: bounded_preview(&String::from_utf8_lossy(record)),
                }))? {
                    return Ok((meta, stats, warnings));
                }
                continue;
            }
        };
        let timestamp = ts_of(&value);
        if !timestamp.is_empty() {
            stats.last_event_ts = timestamp;
        }
        for projected in backend.detail_records(&value, &record_id, line_number) {
            if let DetailRecord::Tool(tool) = &projected
                && let Some(call_id) = &tool.call_id
            {
                if tool.status == ToolActivityStatus::Started {
                    pending_tools.insert(call_id.clone(), tool.clone());
                } else {
                    pending_tools.remove(call_id);
                }
            }
            match &projected {
                DetailRecord::Message(_) => stats.message_count += 1,
                DetailRecord::Tool(tool) if tool.status == ToolActivityStatus::Started => {
                    stats.tool_count += 1;
                }
                DetailRecord::Tool(_) => {}
                DetailRecord::Evidence(evidence) => {
                    if evidence.status == "hidden_internal" {
                        stats.hidden_internal_count += 1;
                    } else if evidence.status == "unsupported" {
                        stats.unsupported_count += 1;
                    }
                }
            }
            if !visit(projected)? {
                return Ok((meta, stats, warnings));
            }
        }
    }
    for mut tool in pending_tools.into_values() {
        tool.status = ToolActivityStatus::Incomplete;
        if !visit(DetailRecord::Tool(tool))? {
            return Ok((meta, stats, warnings));
        }
    }
    if stats.unsupported_count != 0 {
        warnings.push(format!(
            "encountered {} unsupported Transcript Entry projection(s)",
            stats.unsupported_count
        ));
    }
    stats.observed_duration_ms = observed_duration_ms(&stats.start_ts, &stats.last_event_ts);
    Ok((meta, stats, warnings))
}

pub(crate) fn read_evidence(
    backend: &dyn SessionBackend,
    home: &Path,
    query: &str,
    entry: &str,
    snapshot: &str,
) -> Result<TranscriptEvidence> {
    let path = detail_file_path(backend, home, query)?;
    let file = open_session_transcript(home, &path)?;
    let current_snapshot = snapshot_for_metadata(
        &file
            .metadata()
            .with_context(|| format!("inspect session transcript {}", safe_path(&path)))?,
    );
    if current_snapshot != snapshot {
        return Err(crate::application_error::application_error(
            crate::application_error::ApplicationErrorKind::Conflict,
            "Session Transcript changed since it was inspected; refresh the detail view",
        ));
    }
    let line_number = entry
        .strip_prefix("line-")
        .and_then(|value| value.parse::<u64>().ok())
        .with_context(|| format!("invalid Transcript Entry id: {entry}"))?;
    let mut reader = io::BufReader::new(file);
    let mut raw = Vec::new();
    let mut current_line = 0_u64;
    loop {
        raw.clear();
        let read = (&mut reader)
            .take(MAX_TRANSCRIPT_LINE_BYTES.saturating_add(2))
            .read_until(b'\n', &mut raw)
            .with_context(|| format!("read Transcript Entry {entry}"))?;
        if read == 0 {
            break;
        }
        current_line += 1;
        if current_line != line_number {
            continue;
        }
        let record = raw.strip_suffix(b"\n").unwrap_or(&raw);
        let record = record.strip_suffix(b"\r").unwrap_or(record);
        if record.len() as u64 > MAX_TRANSCRIPT_LINE_BYTES {
            bail!(
                "Transcript Entry exceeds the {} byte limit",
                MAX_TRANSCRIPT_LINE_BYTES
            );
        }
        let (encoding, content) = match std::str::from_utf8(record) {
            Ok(value) => (EvidenceEncoding::Utf8, value.to_string()),
            Err(_) => (
                EvidenceEncoding::Base64,
                base64::Engine::encode(&base64::engine::general_purpose::STANDARD, record),
            ),
        };
        if let Ok(value) = serde_json::from_slice::<Value>(record)
            && backend
                .detail_records(&value, entry, line_number)
                .iter()
                .any(|item| {
                    matches!(item, DetailRecord::Evidence(evidence) if evidence.status == "hidden_internal")
                })
        {
            bail!("internal reasoning is not available as Transcript evidence");
        }
        return Ok(TranscriptEvidence {
            entry_id: entry.to_string(),
            encoding,
            content,
            snapshot: current_snapshot,
        });
    }
    Err(crate::application_error::application_error(
        crate::application_error::ApplicationErrorKind::NotFound,
        format!("Transcript Entry does not exist: {entry}"),
    ))
}
