//! Browse saved Sessions directly from a Tenant Home or Host Home without
//! starting a container. Discovery, id resolution, listing, and deletion are shared;
//! [`SessionBackend`] isolates the two Coding Agents' Transcript formats.
//! Strict discovery protects Console detail and deletion from partial views,
//! while listing can report traversal errors alongside readable Sessions.

mod backend;
mod catalog;
mod claude;
mod codex;
mod detail;
mod filesystem;
mod model;

pub(crate) use backend::{SessionBackend, backend_for};
pub(crate) use catalog::{
    delete_sessions as delete_session_catalog, discovery_summary as session_discovery_summary,
    is_canonical_uuid, list_data as list_session_data,
};
#[cfg(test)]
pub(crate) use detail::detail_records_for_test;
pub(crate) use detail::{
    read_evidence as read_session_evidence, stream_detail_data as stream_session_detail,
};
pub(crate) use filesystem::{SessionDiscoverySummary, UUID_TEXT_LEN};
pub(crate) use model::{
    ConversationMessage, ConversationRole, DetailRecord, PromptRecord, SessionDetailMeta,
    SessionDetailStats, SessionListData, SessionNativeFacts, ToolActivity, ToolActivityStatus,
    TranscriptEvidence, TranscriptEvidenceSummary, bounded_preview, evidence_for, ts_of,
};
#[cfg(test)]
pub(crate) use model::{EvidenceEncoding, SessionListRow};

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
