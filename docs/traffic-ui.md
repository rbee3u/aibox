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

The browser never parses model bodies for Summary. It receives decimal
nanosecond offsets, uses `BigInt` to build Timing Stages on a shared axis, and
falls back to a single Response body stage when a protocol has no observable
First Token. Unknown protocols retain generic Timing and diagnostics while
Token Usage reports unsupported. Recognized active protocols without final
usage report waiting; terminal records without final usage report not reported.

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
