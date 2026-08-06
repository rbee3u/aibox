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
interfaces mirror the existing Rust JSON responses; the Rust routes, CSRF
rules, CSP, and loopback checks are unchanged. Components receive an API
interface so tests can use deterministic fakes without sockets.

React hooks own pagination, selection, body offsets, request cancellation, and
the existing 2.5-second list / 1-second active-record polling. Formatting and
binary decoding stay in pure functions covered by Vitest.
