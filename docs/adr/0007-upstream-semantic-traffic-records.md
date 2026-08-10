# Record upstream HTTP semantics with raw evidence and a protocol summary

Status: accepted; version-1 compatibility superseded by ADR-0011

A Traffic Record represents one upstream request attempt at the HTTP semantic
layer. It stores filtered application headers, raw body bytes, monotonic timing
checkpoints, and only a final upstream response; it does not pretend to be a
wire capture or persist proxy-generated responses. This boundary keeps the
record useful across independent downstream and upstream protocol negotiation
without coupling the format to TLS, HTTP/2 framing, or a particular client.

`summary.json` exists from Record creation and is atomically checkpointed at
observable milestones. Nanosecond offsets share an RFC 3339 wall-clock anchor,
so incomplete attempts retain meaningful timing without relying on wall-clock
duration arithmetic. A `text/event-stream` response may also have a best-effort
JSONL index whose entries point into the unchanged `response.body`; index
failure is a warning and never changes forwarding or the Traffic Outcome. The
stream loop also recognizes the terminal events used by Claude Messages and
OpenAI Responses so an Agent's normal close immediately after a complete SSE
response is not recorded as a failed client disconnect.

For OpenAI Responses and Claude Messages, the write path also materializes a
best-effort Model Protocol Summary inside `summary.json`. It records stable
requested and effective values separately, accumulates Token Usage in memory
until the protocol response is terminal, and checkpoints newly established
facts without changing the raw bodies. Protocol interpretation failures become
deduplicated warnings and do not affect forwarding or the Traffic Outcome; a
failure to publish canonical `summary.json` remains a recording failure.

## Considered Options

Wire capture would preserve header spelling, cross-name order, informational
responses, trailers, and transport frames, but it would require owning both
protocol stacks and TLS termination details. Persisting parsed SSE payloads
would duplicate source data. Deriving protocol facts on every list or detail
read would keep `summary.json` purely observational but would repeatedly open
and parse bodies, make list presentation expensive, and defer known facts until
a viewer asks for them. The selected format keeps raw application-visible bytes
as evidence while materializing only the stable overview needed by management
APIs. Header values and same-name duplicates are retained, while field-name
casing and cross-name order may be normalized by the HTTP library.

## Consequences

Traffic Records remain temporary diagnostics without a migration mechanism.
Incompatible schema versions are ignored as unsupported. ADR-0011 supersedes
the former additive version-1 compatibility decision with a complete version-2
Summary projection. Later presentation layers can derive durations, body sizes,
status lines, and SSE views without changing raw evidence or reopening bodies
for the protocol overview.
