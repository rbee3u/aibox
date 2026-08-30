//! Coding Agent Transcript backend contract and selection.

#[cfg(test)]
use super::filesystem::test_transcript_home;
use super::filesystem::{
    SessionDiscovery, checked_session_dir, try_for_each_json_line, walk_jsonl, walk_jsonl_tolerant,
};
use super::model::{
    DetailRecord, PromptRecord, SessionNativeFacts, SessionSummary, ToolActivityStatus,
    TranscriptDiagnostics, evidence_for,
};
#[cfg(test)]
use super::model::{Prompt, ts_of};
use crate::agent::AgentKind;
use anyhow::Result;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) trait SessionBackend {
    /// Path components of the transcript tree beneath the tenant home
    /// (e.g. `[".claude", "projects"]`), resolved only through real directory
    /// entries so agent-created symlinks are never followed.
    fn session_dir_components(&self) -> &'static [&'static str];

    /// Whether a `.jsonl` file name is a transcript. Claude keeps all; Codex
    /// keeps only `rollout-` names. Shared by [`files`](Self::files) and
    /// [`list_files`](Self::list_files), so `list` can never show a row that
    /// `get`/`delete` then refuse to resolve.
    fn keep_transcript_name(&self, name: &str) -> bool;

    /// All transcript files under this tenant home (empty if none yet). The
    /// strict walk: `get`/`delete` use it, and a destructive or single-target
    /// action must not act on a partial view of the tree.
    fn files(&self, home: &Path) -> Result<Vec<PathBuf>> {
        let Some(base) = checked_session_dir(home, self.session_dir_components())? else {
            return Ok(Vec::new());
        };
        walk_jsonl(&base, |name| self.keep_transcript_name(name))
    }

    /// Transcript files for Console Session listing: the tolerant walk, so one bad
    /// child path does not hide every readable session.
    fn list_files(&self, home: &Path) -> Result<SessionDiscovery> {
        let Some(base) = checked_session_dir(home, self.session_dir_components())? else {
            return Ok(SessionDiscovery::default());
        };
        walk_jsonl_tolerant(&base, |name| self.keep_transcript_name(name))
    }

    /// The session id for a transcript path.
    fn id_of(&self, path: &Path) -> String;

    /// Classify one line for the list title and compatibility parser, filtering injected/wrapper
    /// turns while distinguishing recognized non-prompts from unsupported
    /// user-like records. This is the heart of the divergence: Claude keys off
    /// `promptSource:typed`, Codex off a wrapper-filtered `response_item` user
    /// message.
    fn prompt_record(&self, value: &Value) -> PromptRecord;

    /// Project one native Transcript Entry into the Console's shared detail
    /// vocabulary. Coding Agent formats intentionally keep this mapping local.
    fn detail_records(&self, value: &Value, entry_id: &str, line: u64) -> Vec<DetailRecord> {
        vec![DetailRecord::Evidence(evidence_for(
            value,
            entry_id,
            line,
            "unsupported",
        ))]
    }

    fn native_facts(&self, _value: &Value, _facts: &mut SessionNativeFacts) {}

    /// The session start timestamp from one parsed line; the first `Some` is
    /// retained. Claude answers for any line bearing a non-empty top-level
    /// `timestamp`; Codex answers for a `session_meta` timestamp.
    fn start_ts_of(&self, value: &Value) -> Option<String>;

    /// Lower-confidence timestamp candidate used only when
    /// [`start_ts_of`](Self::start_ts_of) never finds one.
    fn fallback_start_ts_of(&self, _value: &Value) -> Option<String> {
        None
    }

    /// A `list` row title candidate from one parsed line. The *last* non-empty
    /// candidate wins; a session with none falls back to its first readable
    /// user message. Default: no candidates (Codex has no ai-title); Claude overrides
    /// to surface `ai-title` lines.
    fn title_of(&self, _value: &Value) -> Option<String> {
        None
    }

    /// Summarize one transcript for `list`. Every transcript summarizes — a
    /// session with no readable message just gets an empty title (unless a backend's
    /// `title_of` finds something else, like Claude's `ai-title`), so tool/
    /// injected-only shells still list and can be cleared. One streaming pass
    /// with O(1) state; the Coding Agent-specific answers come from the methods
    /// above.
    /// `home` anchors no-follow traversal of every path component.
    fn summarize_in(&self, home: &Path, path: &Path) -> Result<SessionSummary> {
        let mut start_ts: Option<String> = None;
        let mut fallback_start_ts: Option<String> = None;
        let mut first_typed: Option<String> = None;
        let mut title: Option<String> = None;
        let mut latest_message = String::new();
        let mut message_count = 0;
        let mut tool_count = 0;
        let mut native_facts = SessionNativeFacts::default();
        let mut diagnostics = TranscriptDiagnostics::default();
        diagnostics.malformed_lines = try_for_each_json_line(home, path, |value| {
            if start_ts.is_none() {
                start_ts = self.start_ts_of(value);
            }
            if fallback_start_ts.is_none() {
                fallback_start_ts = self.fallback_start_ts_of(value);
            }
            let typed = diagnostics.observe_prompt_record(self.prompt_record(value));
            if first_typed.is_none() {
                first_typed = typed;
            }
            if let Some(candidate) = self.title_of(value)
                && !candidate.is_empty()
            {
                title = Some(candidate);
            }
            self.native_facts(value, &mut native_facts);
            for record in self.detail_records(value, "summary", 0) {
                match record {
                    DetailRecord::Message(message) => {
                        latest_message = message.text;
                        message_count += 1;
                    }
                    DetailRecord::Tool(tool) if tool.status == ToolActivityStatus::Started => {
                        tool_count += 1;
                    }
                    DetailRecord::Tool(_) | DetailRecord::Evidence(_) => {}
                }
            }
            Ok(true)
        })?;
        Ok(SessionSummary {
            id: self.id_of(path),
            start_ts: start_ts.or(fallback_start_ts).unwrap_or_default(),
            title: title.or(first_typed).unwrap_or_default(),
            latest_message,
            message_count,
            tool_count,
            native_facts,
            diagnostics,
        })
    }

    /// Test helper that derives the fixture home from the backend's tree.
    #[cfg(test)]
    fn summarize(&self, path: &Path) -> Result<SessionSummary>
    where
        Self: Sized,
    {
        let home = test_transcript_home(path, self.session_dir_components())?;
        self.summarize_in(&home, path)
    }

    /// Collect typed user records for parser tests.
    #[cfg(test)]
    fn prompts_in(&self, home: &Path, path: &Path) -> Result<Vec<Prompt>> {
        let mut out = Vec::new();
        self.for_each_prompt_in(home, path, &mut |prompt| {
            out.push(prompt);
            Ok(true)
        })?;
        Ok(out)
    }

    /// Visit typed user records for parser tests.
    #[cfg(test)]
    fn for_each_prompt_in(
        &self,
        home: &Path,
        path: &Path,
        visit: &mut dyn FnMut(Prompt) -> Result<bool>,
    ) -> Result<(usize, TranscriptDiagnostics)> {
        let mut count = 0;
        let mut diagnostics = TranscriptDiagnostics::default();
        diagnostics.malformed_lines = try_for_each_json_line(home, path, |value| {
            if let Some(text) = diagnostics.observe_prompt_record(self.prompt_record(value)) {
                count += 1;
                return visit(Prompt {
                    timestamp: ts_of(value),
                    text,
                });
            }
            Ok(true)
        })?;
        Ok((count, diagnostics))
    }

    /// Test helper that derives the fixture home from the backend's tree.
    #[cfg(test)]
    fn prompts(&self, path: &Path) -> Result<Vec<Prompt>>
    where
        Self: Sized,
    {
        let home = test_transcript_home(path, self.session_dir_components())?;
        self.prompts_in(&home, path)
    }
}

/// Resolve `AgentKind` to its backend. The one bridge between the enum and the
/// session trait objects.
pub(crate) fn backend_for(agent: AgentKind) -> Box<dyn SessionBackend> {
    match agent {
        AgentKind::Claude => Box::new(crate::session::claude::Claude),
        AgentKind::Codex => Box::new(crate::session::codex::Codex),
    }
}
