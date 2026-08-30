//! OpenAI protocol event classification.

use crate::request::model::ProtocolFamily;

pub(super) fn event_family(
    kind: &str,
    event_name: Option<&str>,
    object: Option<&str>,
) -> Option<ProtocolFamily> {
    if kind.starts_with("response.")
        || event_name.is_some_and(|value| value.starts_with("response."))
    {
        Some(ProtocolFamily::OpenaiResponses)
    } else if matches!(object, Some("chat.completion" | "chat.completion.chunk")) {
        Some(ProtocolFamily::OpenaiChatCompletions)
    } else {
        None
    }
}

pub(super) fn is_terminal_event_kind(value: &str) -> bool {
    matches!(
        value,
        "response.completed" | "response.failed" | "response.incomplete" | "response.cancelled"
    )
}
