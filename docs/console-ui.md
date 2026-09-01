# Console UI Development

The Console is a React and TypeScript application under `console/`. It contains
Overview, Tenants/Components, Configs, Sessions, and Requests. The Rust binary
embeds generated HTML, CSS, and JavaScript from `assets/console.*`.

This document owns frontend development, architecture, testing, and interaction
contracts. Domain behavior belongs in [Configs](configs.md),
[Tenants](tenants.md), or [Filesystem Sandbox and Mounts](sandbox.md).

## Requirements and Workflow

Use a Node version satisfying the range in `console/package.json`. With `nvm`:

```sh
nvm install 24
nvm use 24
make console-ci
```

`make console-ci` installs the committed lockfile. Build tooling uses
platform-specific native bindings, so do not share one `node_modules` between
macOS and Linux or between host and container. Separate clones isolate both
dependencies and Cargo's `target/`; when sharing a Workspace, mount a separate
host directory over `/workspace/console/node_modules`.

Use `make help` as the authoritative target list. The normal workflow is:

| Task | Command |
| --- | --- |
| Full socket-free project check | `make check` |
| Console-only check | `make console-check` |
| Build embedded assets | `make console-build` |
| Update Rust-owned wire artifacts | `make console-contract` |

`make console-check` covers formatting, types, Vitest, ESLint, the Rust-owned
contract, bundle budget, and embedded assets.

For embedded development, build assets before starting the Rust Service:

```sh
make console-build
cargo run -- console
```

Use `make install` after rebuilding when testing the installed command. Edit
`console/index.html` or `console/src/`, never generated `assets/console.html`,
`assets/console.css`, or `assets/console.js`. Publishing rewrites asset URLs to
`/_aibox/ui/app.css` and `/_aibox/ui/app.js`.

## Testing

Keep each rule in the narrowest useful layer:

1. Pure tests cover query codecs, reducers, derivations, formatting, and state
   transitions.
2. Feature interaction tests render the real page against a strict fake of its
   domain API.
3. API adapter tests alone assert HTTP paths, queries, wire bodies,
   normalization, streaming, and binary transport.
4. Optional Chromium tests cover interactions requiring real layout or browser
   behavior.

Tests follow the modules they cover; page interactions stay at the feature
root. Keep one-suite doubles local and shared feature support in that feature's
existing fixture or harness modules. Cross-feature fixtures belong in
`features/common` only when the production concept is also shared.

Do not duplicate pure state rules in browser tests. Geometry checks assert
behavior and relative layout rather than token values or pixel-perfect
baselines. Routine Rust and Console tests remain socket-free.

The ignored real-socket Request Proxy bridge test is explicit and optional:

```sh
cargo test \
  request::tests::reqwest_tcp_smoke_preserves_bytes_headers_query_and_redirect_policy \
  -- --ignored --exact
```

Run it only in a network-permitted host or CI environment.

Playwright uses bundled Chromium, not an installed browser channel. Install it
once per environment and run:

```sh
npm --prefix console exec playwright install chromium
npm --prefix console run test:chromium
```

These checks start a loopback-only Vite listener and remain optional. The
Runtime Image supplies fonts and Chromium ABI libraries but no browser. Do not
add host-only Firefox or WebKit projects.

## Architecture and Ownership

Console dependencies point inward. ESLint enforces layer and feature
boundaries; source imports use the `@/` alias so edges remain visible.

| Layer | May depend on | Responsibility |
| --- | --- | --- |
| `domain/` | itself | Cross-feature identities and invariants |
| `api/` | `domain/` | HTTP, wire conversion, domain API ports |
| `shared/` | `domain/` | API-independent UI, hooks, and libraries |
| `features/common/` | `domain/`, `api/`, `shared/` | Shared feature machinery needing wire and UI types |
| `features/<feature>/` | inner layers and itself | One product feature |
| `app/` | every layer | Shell, routing, theme, and composition |

`api/` and `shared/` never depend on each other. Features never import another
feature or the app shell. `features/common/` may not import a feature back.
`src/test/` is exempt because its harnesses compose complete pages. Do not add
barrel files; every import names the owning module.

Each feature owns its controller, grouped view model, view, route codec, and
cross-action workflow state. Remote loading, polling, streaming reads, and
`AbortController` ownership remain in focused hooks. Controllers expose stable
responsibility groups rather than flat setters, refs, maps, or inferred return
types.

Concern directories express single ownership and must not reach into siblings.
Move a value needed by two concerns to the feature root. Move a value shared by
features inward to `features/common`, `shared`, `api`, or `domain` according to
its dependencies. Keep `shared/` API-independent; reusable hooks accept loaders
or adapters from callers.

Only the app layer integrates browser history and composes the persistent
shell. Pages receive a location snapshot and writer, keep parsing in their
feature route codec, and do not subscribe to history themselves. Dirty Config
edits must be protected across in-app, history, and browser navigation.

## Control API and Generated Assets

Console pages and assets live below `/_aibox/ui/`; Console-internal APIs live
below `/_aibox/api/`. The Control API is not a public integration surface.

The API layer composes narrow domain interfaces over the shared transport,
which owns fetch, CSRF, NDJSON, and binary bodies. Domain adapters own paths,
queries, wire bodies, conversion, and feature-facing ports. Features receive
only their port and never import transport or generated wire types.

Rust owns three generated artifacts under `console/src/api/generated/`: wire
types, a test-facing route manifest, and contract samples. Declare every route
once in `service/control/routes.rs`. Adapter tests use the route manifest and
shared helpers; production clients remain handwritten.

Run `make console-contract` only for an intentional wire update. The check
exports to a temporary directory and compares every artifact byte-for-byte.
The asset check similarly builds to a temporary directory, enforces the bundle
budget, and compares the embedded files. Keep executable checks, not copied
values in documentation, as the source of truth.

## Interaction System

### Visual Structure and Navigation

Use the semantic roles in `shared/styles/tokens.css` for color, surfaces,
status, focus, code, shadow, density, and stacking. Keep light and dark palettes
complete. Shared primitives wrap ordinary native controls and layout; keep
specialized domain interaction with its owning feature instead of introducing
a general UI framework.

The desktop shell uses a persistent, collapsible sidebar; narrow layouts use a
drawer and one-panel catalog/detail navigation. The same route identifies the
active module and detail on both layouts. Invalid query values canonicalize to
a safe default, and responsive changes must not create a second navigation
state machine.

Catalog features share master/detail structure, selection mode, focus recovery,
empty states, dialogs, and pagination where needed. Selecting detail must not
restart an unchanged catalog lifecycle or discard its scroll context. Narrow
detail views retain an explicit back action.

### Selection, Feedback, and Async Work

Batch selection is explicit and never interprets an empty selection as all.
Select-page affects only the visible page; selection may span pages where
supported. Destructive actions require confirmation and restore focus to the
closest valid row, page, or initiating control.

Menus and split actions support keyboard navigation, Escape and outside-click
dismissal, anchored positioning, and focus return. Avoid duplicate destructive
actions when the catalog selection flow already owns deletion.

Failures use the shared notification stack, keyed by resource or action.
Repeated polling failures notify once until recovery. Notices pause while the
user interacts, the page is hidden, or a dialog covers them. Resource failures
offer scoped retry; destructive failures require confirmation again.

The latest Management Operation remains available across modules. Polling must
not overlap itself or reopen a task dock the user collapsed. Dialogs and route
changes cancel obsolete work; late responses never replace a newer generation.
Keep decoding or evidence degradation local to the affected view.

### Accessibility and Content

Use native roles, labels, focus order, and keyboard behavior before ARIA. Every
icon-only control needs an accessible name; tooltips work on focus and hover.
Coarse-pointer targets and narrow layouts must avoid horizontal page overflow.

Use system sans-serif for interface prose and system monospace for paths,
identifiers, URLs, methods, timestamps, Configs, code, raw bodies, Transcripts,
and logs.

Render Agent Conversation Messages as safe GFM Markdown with raw HTML disabled.
Only secure absolute HTTP(S) links are active; relative, root-relative, anchor,
and other schemes stay inert so they cannot enter the same-listener Request
Proxy. User messages and raw evidence preserve plain text and line breaks.

## Feature UI Contracts

### Overview and Tenants

Overview owns Service health, Tenant topology, Runtime Image state, and
actionable attention. Tenants combines its catalog with Component status and
actions. Frontend comparisons may expose an Update action, but never invent
installed state, desired versions, or automatic update behavior. Follow the
[Tenant Component contract](tenants.md#tenant-components).

### Configs

Named Config main files use Visual mode only when the API supplies a Visual
Config Option model; Raw remains available, and Current Config is Raw-only.
Required, omitted, sensitive, enum, and provider fields follow that supplied
model rather than frontend-maintained lists. The editor tracks drafts and
results per file, and ordered saves do not imply rollback. Dirty guards cover
every navigation path. Apply, Last Application, and Drift copy describe a
one-shot projection, never an Active Config. Follow the authoritative
[Config semantics](configs.md).

### Sessions

Session detail has shareable `conversation` and `details` tabs. Conversation
keeps native order, safe Markdown for Agent text, plain-text user messages, and
grouped Tool Activity and Transcript Evidence; reasoning remains hidden.
Streaming renders frames as they arrive, while manual refresh keeps old content
until replacement succeeds. Missing-message, tool-only, evidence-only, and
partial states remain explicit. Follow the [Session contract](tenants.md#sessions).

### Requests

Requests owns page, selection, detail, and tab URL state. Summary is the default
tab; bodies load only for the visible body tab. Selection may span pages and
keeps row context for focus after deletion. Active Requests are unselectable.

Rust supplies Request Assessment and normalized diagnostics. The browser does
not reclassify outcomes or parse bodies to backfill model, usage, First Token,
Session ID, or diagnostics. It presents HTTP status, Provider Error,
proxy/transport findings, and warnings as independent evidence. Follow the
[Request diagnostics contract](sandbox.md#diagnostics).

Body views provide Raw download, decoded Source, and browser-only Pretty
representations. Raw preserves application-visible bytes. Source owns the
top-level Copy value. Pretty never changes or persists a Request. Lossless JSON
handling preserves large-number spelling and rejects duplicate keys. Invalid
UTF-8, JSON, or supported content decoding falls back to Source or encoded hex
without hiding Raw; large decoded bodies default to Source until Pretty is
requested.

SSE cards derive only complete events from decoded Source and keep partial
tails visible. Optional timings join by sequence and degrade locally when
missing or malformed. Content-encoded streams may be shown after complete
decoding but do not invent raw-offset timing. Nanosecond offsets remain decimal
wire values and use `BigInt`; incomplete timing combines only measurable
adjacent stages and never invents a duration.
