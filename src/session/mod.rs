//! Browse saved Sessions directly from a Tenant Home or Host Home without
//! starting a container. Discovery, id resolution, listing, and deletion are shared;
//! [`SessionBackend`] isolates the two Agents' Transcript formats.
//! Strict discovery protects Console detail and deletion from partial views,
//! while listing can report traversal errors alongside readable Sessions.

mod backend;
mod catalog;
mod claude;
mod codex;
mod detail;
mod filesystem;
mod model;
mod text;

use crate::agent::AgentKind;
use anyhow::Result;
use backend::{SessionBackend, backend_for};
pub(crate) use catalog::is_canonical_uuid;
#[cfg(test)]
use detail::detail_records_for_test;
use std::path::Path;

pub(crate) use filesystem::{SessionDiscoverySummary, UUID_TEXT_LEN};
#[cfg(test)]
pub(crate) use model::EvidenceEncoding;
pub(crate) use model::{
    ConversationMessage, ConversationNotice, ConversationRole, DetailRecord, SessionDetailMeta,
    SessionDetailStats, SessionListData, ToolActivity, ToolActivityStatus, TranscriptEvidence,
    TranscriptEvidenceSummary,
};
use model::{
    PromptRecord, SessionNativeFacts, evidence_for, tool_input_preview, tool_output_preview, ts_of,
};

/// List Sessions using the Agent's native Transcript format.
pub(crate) fn list_session_data(agent: AgentKind, home: &Path) -> Result<SessionListData> {
    catalog::list_data(backend_for(agent), home)
}

/// Discover Sessions without parsing their Transcripts.
pub(crate) fn session_discovery_summary(
    agent: AgentKind,
    home: &Path,
) -> Result<SessionDiscoverySummary> {
    catalog::discovery_summary(backend_for(agent), home)
}

/// Stream one Session's detail through its Agent-specific parser.
pub(crate) fn stream_session_detail(
    agent: AgentKind,
    home: &Path,
    query: &str,
    begin: &mut dyn FnMut(&SessionDetailMeta) -> Result<bool>,
    visit: &mut dyn FnMut(DetailRecord) -> Result<bool>,
) -> Result<(SessionDetailMeta, SessionDetailStats, Vec<String>)> {
    detail::stream_detail_data(backend_for(agent), home, query, begin, visit)
}

/// Read evidence from an unchanged Transcript snapshot.
pub(crate) fn read_session_evidence(
    agent: AgentKind,
    home: &Path,
    query: &str,
    entry: &str,
    snapshot: &str,
) -> Result<TranscriptEvidence> {
    detail::read_evidence(backend_for(agent), home, query, entry, snapshot)
}

/// Delete explicit Session identities or an explicitly selected whole catalog.
pub(crate) fn delete_session_catalog(
    agent: AgentKind,
    home: &Path,
    ids: &[String],
    all: bool,
) -> Result<usize> {
    catalog::delete_sessions(backend_for(agent), home, ids, all)
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
