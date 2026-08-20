# Session conversation projection

## Status

Accepted

## Context

Native Claude and Codex Transcripts contain several kinds of records in one
ordered JSONL stream: user input, Agent-facing text, tool calls and results,
injected context, reasoning, protocol events, and malformed or future records.
A typed-user-prompt list loses the Agent reply, hides tool placement, and makes
diagnostic-only Sessions look empty.

The Console also needs to remain bounded when a Transcript is large. Showing
complete tool arguments and raw records in the initial response would make the
detail view expensive and would unnecessarily expose internal reasoning.

## Decision

Session detail uses one shared domain projection:

- `Conversation Message` contains readable user input or Agent-facing text.
- `Tool Activity` contains a tool invocation or result, with status and a safe
  bounded summary.
- `Transcript Evidence` represents every other native entry that is useful for
  diagnosis, including malformed and unsupported records.

The projection preserves native order. The Console may merge adjacent messages
from the same role into one visual bubble while retaining every source Entry
id. Tool Activity and evidence remain inline disclosures so their placement is
not lost.

Reasoning and thinking are diagnostic facts, not Conversation Messages. They
are counted and can be represented by a hidden-internal evidence row, but no
endpoint returns their raw text. This keeps the conversation useful without
turning internal deliberation into user-facing content.

The detail endpoint streams `meta`, `message`, `tool_activity`, `evidence`, and
terminal `complete` frames as NDJSON. It reads the Transcript line by line and
does not retain the complete native file in memory. Full raw evidence is a
separate request for one Entry id. That request validates the safe Transcript
path and a metadata snapshot before returning UTF-8 text or an explicit base64
encoding; a changed snapshot is a conflict requiring refresh.

## Consequences

The Sessions list can summarize the latest readable message and message/tool
counts in the same scan, while still listing tool-only or evidence-only
Sessions. Parser warnings remain visible and deletion keeps its existing strict
filesystem validation. The Console no longer depends on the old typed-prompt
stream and does not promise an external compatibility route for it.
