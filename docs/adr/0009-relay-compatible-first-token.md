# Define First Token by relay-compatible SSE data arrival

Status: accepted

Traffic diagnostics need a First Token value comparable to the values exposed
by common model API relays. aibox therefore records First Token for recognized
OpenAI Responses and Claude Messages streams when the first trim-nonempty SSE
`data:` line not beginning with `[DONE]` is completely received. The timing
origin remains the start of the aibox Traffic Record, preserving its end-to-end
view rather than adopting a relay's internal request origin.

This definition intentionally follows the new-api accounting boundary. It also
matches the principal Claude behavior of sub2api. In particular, OpenAI
`response.created` and Claude `message_start` qualify, as do ping, error,
malformed JSON, and empty-delta data. Comments, non-data fields, blank data, and
`[DONE]` prefixes do not. Completion is a line-level observation: a split line
uses the arrival time of its terminator, and an unterminated final line uses the
last body arrival time at EOF.

## Considered Options

Waiting for the first output-bearing, parseable model delta more closely
describes semantic First Output, but it diverges sharply from relay dashboards
when an upstream spends substantial time between its initial protocol event
and later model content. Using response headers or the first body byte would be
cheaper but would count comments and other non-data bytes that the target relay
definition excludes.

## Consequences

`summary.protocol.first_token_at_ns` keeps its existing name and type but no
longer promises a tokenizer token or semantic model output. It can be present
for a failed stream, a ping, or unparseable data. Protocol parsing, diagnostics,
Usage, and terminal detection still wait for complete SSE Events and remain
independent of this checkpoint. Unknown protocols and non-streaming responses
retain no First Token. The Traffic schema is not upgraded and existing Records
are neither migrated nor backfilled, so old and new semantics can coexist until
operators clear older Traffic Records.
