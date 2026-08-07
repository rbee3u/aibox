# Record upstream HTTP semantics with stable raw files

Status: accepted

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

## Considered Options

Wire capture would preserve header spelling, cross-name order, informational
responses, trailers, and transport frames, but it would require owning both
protocol stacks and TLS termination details. Persisting parsed SSE payloads
would simplify one current viewer, but would duplicate and reinterpret source
data. The selected format instead keeps raw application-visible bytes and
records its fidelity limits explicitly: header values and same-name duplicates
are retained, while field-name casing and cross-name order may be normalized by
the HTTP library.

## Consequences

The schema is intentionally replaced rather than migrated because Traffic
Records are temporary diagnostics and the project has no compatibility burden.
Legacy records are ignored as unsupported. Later presentation layers can derive
durations, body sizes, status lines, and SSE views without changing the raw
record layout.
