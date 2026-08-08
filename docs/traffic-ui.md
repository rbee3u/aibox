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
record-outcome fields, and the persisted Model Protocol Summary; the Rust
routes, CSRF rules, CSP, and loopback checks remain unchanged. Components
receive an API interface so tests can use deterministic fakes without sockets.

React hooks own pagination, selection, body offsets, request cancellation, and
the 5-second list / 3-second active-record polling. The Summary is
the default detail tab, and request/response bodies load only for the visible
body tab. Formatting and binary decoding stay in pure functions covered by
Vitest.

## Protocol Summary

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
