//! Claude protocol event classification.

use crate::request::model::ProtocolFamily;

pub(super) fn event_family(kind: &str, event_name: Option<&str>) -> Option<ProtocolFamily> {
    (is_event_kind(kind) || event_name.is_some_and(is_event_kind))
        .then_some(ProtocolFamily::ClaudeMessages)
}

fn is_event_kind(value: &str) -> bool {
    matches!(
        value,
        "message_start"
            | "message_delta"
            | "message_stop"
            | "content_block_start"
            | "content_block_delta"
            | "content_block_stop"
    )
}

pub(super) fn is_terminal_event_kind(value: &str) -> bool {
    value == "message_stop"
}
