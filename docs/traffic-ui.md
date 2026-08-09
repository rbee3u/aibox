# Traffic UI Development

The Traffic viewer is a React and TypeScript application under
`web/traffic/`. Node and npm are development tools only. The Rust binary
continues to embed the generated files in `assets/traffic.html`,
`assets/traffic.css`, and `assets/traffic.js`.

## Requirements

Use Node 24, matching the bundled aibox development image (`v24.4.0`). The
repository commits `package-lock.json`, so install the exact dependency tree
with:

```sh
make traffic-deps
```

## Common Commands

```sh
make traffic-format     # Format frontend source files
make traffic-build      # Generate the three embedded assets
make traffic-test       # Vitest and React interaction tests
make traffic-lint       # ESLint frontend source files
make traffic-check      # Format check, typecheck, build, node check, test, lint
```

There is intentionally no required Vite development server. Run `make
traffic-build`, then start `aibox traffic` and open the embedded viewer to
check the complete page and real Traffic API.

Do not edit the generated files in `assets/traffic.*` directly. Change the
source in `web/traffic/src/` and rebuild them before committing. The
generated HTML keeps the Rust-injected `__AIBOX_CSRF__` placeholder and the
existing management routes.

## Code Boundaries

`src/api.ts` is the only browser-facing Traffic API client. Its TypeScript
interfaces mirror the Rust JSON responses, including raw Summary timing,
record-outcome fields, the top-level Coding Agent Session ID, and the persisted
Model Protocol Summary; the Rust routes, CSRF rules, CSP, and loopback checks
remain unchanged. Components receive an API interface so tests can use
deterministic fakes without sockets.

React hooks own pagination, selection, body offsets, request cancellation, and
the 5-second list / 3-second active-record polling. The Summary is
the default detail tab, and request/response bodies load only for the visible
body tab. Formatting and binary decoding stay in pure functions covered by
Vitest.

## Body Views

The Request and Response tabs open in `Pretty` when the complete decoded Body
has a renderer. The viewer keeps three deliberately separate representations:

- The Traffic Record and the existing `request-body` / `response-body` routes
  contain the exact original application-visible bytes. Download always uses
  these routes and preserves those bytes.
- `Source` is the unformatted content after applying the supported HTTP
  `Content-Encoding`. The top-level Copy action copies this text regardless of
  the selected view.
- `Pretty` is derived in the browser from Source. It never changes or persists
  a Traffic Record.

The read-only `request-body-decoded` and `response-body-decoded` management
routes accept no coding, an empty coding, `identity`, or one case-insensitive
`zstd` coding. Rust streams zstd decoding from a blocking worker; unsupported
or combined codings do not alter the raw Body. An active encoded Body must be
complete before it can be decoded. Source can show a partially received
identity Body, while an incomplete zstd Body is explicitly shown as encoded
hex until decoding is possible.

JSON uses a lossless parser so Pretty and per-value Copy retain the source
spelling of numbers outside JavaScript's safe range. Duplicate object keys,
invalid JSON, invalid UTF-8, unsupported coding, and decode failures fall back
to Source or encoded hex without hiding the original download. The JSON root
opens initially, nested objects and arrays start folded, and strings longer
than 200 Unicode characters start truncated. A decoded Body over 5 MiB starts
in Source and requires the explicit `Render Pretty` action; this is a UI guard,
not a hard rendering or recording limit.

For an event-stream response, Pretty derives complete `SSE Event` cards from
decoded Source. It handles the UTF-8 BOM, CR/LF/CRLF delimiters, multiline
`data`, comments, and the default `message` event type. Only an empty-line
terminated block with at least one `data` field is an SSE Event. A partial tail
is identified separately, and text such as `[DONE]` remains visible rather
than being treated as JSON.

The `response-event-timings` route reads the existing best-effort
`response.events.jsonl` index on demand and returns only each sequence and its
complete-receipt offset. The browser joins those offsets to independently
parsed SSE Events by sequence. A missing, truncated, or partly malformed index
shows `Time unavailable` plus one warning and never suppresses Event data.
Active views request later sequences during their normal poll. Event time is
shown relative to Record start at millisecond precision, with the absolute
timestamp in a tooltip using the viewer's existing timezone convention.

## Protocol Summary

For recognized model requests, the Traffic Proxy also records an optional
top-level `summary.coding_agent_session_id`. OpenAI Responses prefers the first
nonempty UTF-8 `session-id` request-header value and falls back to
`x-claude-code-session-id`; Claude Messages uses the reverse precedence.
Header names are matched exactly and case-insensitively. Unknown protocols do
not derive this value, bodies are never searched for it, and older Records are
not backfilled.

The Traffic Proxy derives model, reasoning effort, response mode, First Token,
final Token Usage, and Provider Errors from native OpenAI Responses or Claude
Messages data while it records the exchange. Stable facts are atomically
checkpointed in the optional `summary.protocol` object. List and detail APIs
return that same object without opening or parsing request/response bodies.
Older version-1 Records without the object remain readable and are never lazily
backfilled. Raw bodies and the best-effort SSE index remain available for
diagnosis.

For a recognized streaming response, First Token is the offset at which the
first trim-nonempty SSE `data:` line not beginning with `[DONE]` is completely
received. It is deliberately compatible with common relay accounting rather
than a claim that tokenizer or semantic output has arrived: ping, error,
malformed JSON, empty-delta JSON, Claude `message_start`, and OpenAI
`response.created` data all qualify. Comments, other SSE fields, blank data,
and `[DONE]` prefixes do not. A line split across body chunks uses the arrival
time of its terminator; an unterminated final line uses the last body arrival
time at EOF. Unknown protocols and non-streaming responses have no First Token.

The browser never parses model bodies for Summary. It receives decimal
nanosecond offsets, uses `BigInt` to build Timing Stages on a shared axis, and
falls back to a single Response body stage when a protocol has no observable
First Token. Unknown protocols retain generic Timing and diagnostics while
Token Usage reports unsupported. Recognized active protocols without final
usage report waiting; terminal records without final usage report not reported.
For streaming responses with First Token, the interval after that checkpoint
is named `Response stream` rather than implying that every byte is model
output.

The detail Summary presents Model and Token Usage in one pale hierarchy card.
The effective-or-requested model is the primary value, followed by a weaker
reasoning effort and a `Streaming` or `Non-streaming` badge when those facts are
available. Session ID remains on its own secondary row with an inline copy
control. A missing model says `Not reported`, or `Detecting…` while active;
missing optional qualifiers are omitted.

Token Usage follows the provider billing categories in one responsive table.
Its wider input block places three categories side by side, with a weaker Total
input row spanning beneath them, while the narrower Output block centers its
primary value above a lower-right Reasoning inset. OpenAI uses Input, Cached
input, and Cache writes. Claude uses Base input, Cache hits & refreshes, and a
Cache writes total with 5m/1h details when that breakdown is reported. Once any
primary counter is available, missing categories remain visible as `—` and
explicit zero remains visible; a completely empty report retains its state
message. Metric labels and values sit together as centered inline pairs,
including Output and the lower-right Reasoning inset. Timing keeps its stage
timeline and metric order while using the same centered inline treatment for
First token, Duration, and Started. Diagnostics, the Record list, and the other
detail tabs do not use this presentation.
