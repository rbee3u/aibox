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

The desktop shell uses a persistent, collapsible sidebar; orientation is the
selected sidebar item, and the workspace has no repeating module title bar.
External resource links sit in the sidebar footer as an icon row, not as
module-sized destinations.
Narrow layouts use a compact bar with the menu control and module name, plus a
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
closest valid row, page, or initiating control. Confirm-dialog fact
labels stay sentence case. Typed confirmation inputs keep an accessible
name; the copy control stays outside that label. Create dialogs keep Create
disabled when the typed name already appears in that catalog.

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
actionable attention. The attention list is the only verbal “needs work”
summary: it names the first target and why it needs work, then jumps
there, without a visible section title. A healthy summary is one quiet
line. Key facts, listen/root, and Runtime share one quiet status row.
Health facts stay heavier than Version, listen, and Root.
On narrow viewports that row collapses to a wrapping health summary so
every health token stays visible; Version, listen, and Root stay on the
summary title. Attention items wrap as a
short list without filled chips. The topology toolbar keeps the title
and iconifies collapse, expand, and refresh so it does not nest a
horizontal scroll.
Named Config and Component inventory counts stay in topology, not the
status row. Build is primary only while the Runtime Image is missing or
unknown; those states also keep Build without cache inline. When the
image is Built, Build stays secondary and the cacheless rebuild moves
into the Build overflow. Topology marks the same state spatially and does
not repeat the page-level attention count. Agent cards show the
last-applied name and drift without a Last applied or Config Drift
prefix; Sessions say Load count until a summary is loaded. Shortened or
truncated node copy is disclosed on the whole node for pointer hover and
keyboard focus. Opening a node docks its details to the right of the
topology workspace so the tree stays clickable; the hover disclosure
closes. That pane stays pinned under the topology toolbar while the
tree scrolls, so its facts and Open in action stay reachable.
That pane shows facts the card shortened — last-applied time,
file counts, full Tenant home, on-demand Session load. When those extra
facts are absent it still names the card metric the click asked for
(Session count, leaf status) so the pane is not an empty tone line.
It does not shout an uppercase eyebrow or repeat Dirty when last-applied
time and file counts are already listed. On narrow viewports that pane
stacks under the canvas instead of overlaying it. Topology nodes share
one quiet card chrome; identity
nodes stay taller and bolder, catalog and leaf nodes stay shorter.
Do not mix cards with underline rows. Collapsed branches keep a plus
control and do not show a filled child-count badge. Topology Component
and Named Config leaves use the same width as Tenant cards so their
labels stay readable. Current Config leaves use Application Drift for dirty,
source-missing, and comparison-error the same way Attention names them;
healthy leaves keep the file-count detail. First expansion follows the first attention path; when nothing
needs work it still opens the protected
default Tenant. Overview Component attention and topology Component leaf
nodes open Tenants with `tenant=<selection>` and `component=<kind>`. Tenants
scrolls that row into view, highlights it once, then drops `component=` so
the row stays a non-selectable list item. A Components group node, or a
catalog-level Component error, still opens the Tenant only. Tenants
combines its catalog with Component status and actions. The Component
header keeps inventory and issue count as the only verbal summary.
Check for updates is a quiet icon control; last-checked time is not a
header fact. Catalog create is the same quiet tone as Refresh and Select,
not a filled Primary. On narrow
viewports the catalog and detail take turns filling the page; the
detail pane stays hidden until a Tenant is opened. Frontend
comparisons may expose an Update action, but never invent installed state,
desired versions, or automatic update behavior. Follow the
[Tenant Component contract](tenants.md#tenant-components).

### Configs

Named Config main files use Visual mode only when the API supplies a Visual
Config Option model; Raw remains available, and Current Config is Raw-only.
Required, omitted, sensitive, enum, and provider fields follow that supplied
model rather than frontend-maintained lists. The editor tracks drafts and
results per file, and ordered saves do not imply rollback. Dirty guards cover
every navigation path. In-app leaves, including the sidebar, use the same
Unsaved changes dialog with Save and continue. History and unload stay on
the native confirmation. Last Application is quiet Current Config metadata.
When Config Drift is dirty, that line also says it differs, without a
Dirty badge. Apply success feedback states the one-shot projection;
Drift stays on the
recorded Named Config. None of that copy describes an Active Config. The
editor heading is the selected Config name. Host risk and the unredacted
content warning share one quiet line. Tenant, Agent, Config, and File
facts appear only on narrow viewports, when the catalog is hidden.
Credential Propagation stays on the Host Codex Current Config row as a
quiet control, not the same emphasis as Apply. Catalog create uses that
same quiet tone; Apply stays the row action. Catalog Tenant and Coding
Agent filters share leftover toolbar width so Host Tenant is not clipped;
longer names still ellipsize.
Overview Named Configs opens
Configs with `named=1`: the Named Configs catalog for that Tenant and Coding
Agent, without selecting Current Config or opening the editor. `current=1`
inspects Current Config; `config=<name>` inspects a Named Config. Absent both,
Configs still defaults to Current Config. Follow the authoritative
[Config semantics](configs.md).

### Sessions

Session detail has shareable `conversation` and `details` tabs. Conversation
keeps native order, safe Markdown for Agent text, plain-text user messages, and
grouped Tool Activity and Transcript Evidence; reasoning remains hidden.
A leading Codex skill file link `[$name](path)` displays as `$name` with the
path on the title; the Conversation navigator uses the same label.
A Codex request-review prompt collapses to the first embedded user line, or
Review continuation; the full prompt stays in a disclosure. The navigator uses
the same label. A whole-message approval JSON with outcome or risk_level
shows those present fields and keeps the raw object in a disclosure.
Groups that include tools are labeled as tools and show the tool input on the
row; evidence-only groups stay Transcript activity, collapsed, without native
type names or a tool icon. Evidence-only groups before the first message stay
off the Conversation reading stream; their counts remain on Details.
Routine filtered, hidden-internal, and unsupported projections stay in those
groups without warning chrome. Header warning, Conversation banner, and the
Details attention mark appear only when reading is impaired: malformed
records, an incomplete stream, or failed Tool Activity. Details keeps
unsupported and hidden-internal as quiet counts and does not banner the
unsupported-projection warning.
Streaming renders frames as they arrive, while manual refresh keeps old content
until replacement succeeds. Missing-message, tool-only, evidence-only, and
partial states remain explicit. The Conversation header shows source, start
time, and message/tool counts; the first-to-last event interval stays on Details
as Observed span, not Duration. Details omits Tenant, Coding Agent, and start
time already shown in that header; field labels stay sentence case. The catalog primary line prefers human-readable
copy: it skips review boilerplate, JSON-only previews, and skill file paths,
and demotes the native title or skill name to the secondary line. That
secondary line stays plain text: it drops `**`, backticks,
blockquote `>`, and a leading `- ` / `* ` list marker, and does not
render GFM. A promoted
latest message uses its first paragraph or first CJK sentence, not a
collapsed severity list or trailing file path. When a
single Tenant and Coding Agent are selected, catalog rows omit the source
and keep the time. Codex request review rows do not show an empty-preview
line. Single-session
delete confirmation keeps `display_id` in the title and restates the catalog
headline, source, and start time. Batch confirmation keeps the selected count
and sources and does not enumerate ids. Follow the
[Session contract](tenants.md#sessions).

### Requests

Requests owns page, selection, detail, and tab URL state. Summary is the default
tab; bodies load only for the visible body tab. Selection may span pages and
keeps row context for focus after deletion. Active Requests are unselectable.
Single-request delete confirmation restates method, target, status, time, and
id from the catalog row. Batch confirmation keeps the selected count and does
not enumerate ids.

Rust supplies Request Assessment and normalized diagnostics. The browser does
not reclassify outcomes or parse bodies to backfill model, usage, First Token,
Session ID, or diagnostics. It presents HTTP status, Provider Error,
proxy/transport findings, and warnings as independent evidence. The catalog
row shows a short Assessment label (`Warning:` / `Error:` plus the finding
kind) in place of the model on the secondary line; the model stays in the
row title. The full diagnostic sentence stays on the detail record. The catalog
row and the detail headline both omit an Assessment label when that primary
only restates the HTTP status already shown. The Model summary shows protocol
response mode as `Stream` or `Non-stream`; that label is not the live
`Streaming` phase used on active catalog rows. Follow the
[Request diagnostics contract](sandbox.md#diagnostics).

Body views provide Raw download, decoded Source, and browser-only Pretty
representations. Raw preserves application-visible bytes. Source owns the
top-level Copy value. Pretty never changes or persists a Request. Request and
Response Headers start collapsed as a count and content-type summary;
expanding reveals the table. Values stay unredacted. The
no-redaction reminder is quiet metadata, not a warning Alert. Lossless JSON
handling preserves large-number spelling and rejects duplicate keys. Invalid
UTF-8, JSON, or supported content decoding falls back to Source or encoded hex
without hiding Raw; large decoded bodies default to Source until Pretty is
requested.

SSE cards derive only complete events from decoded Source and keep partial
tails visible. Completed records start at the first Event; only an active
stream pins to the newest Event. Collapsed cards show text already present
on that Event and do not assemble a reconstructed reply. Consecutive
same-type Events without a useful card preview (missing, or only a short
fragment) collapse into a summary row; empty and short-fragment runs stay
separate so a later short text Event does not join a preview-less run.
Expanding the row reveals those Events. Optional timings
join by sequence and degrade locally when missing or malformed.
Content-encoded streams may be shown after complete decoding but do not
invent raw-offset timing. Nanosecond offsets remain decimal
wire values and use `BigInt`; incomplete timing combines only measurable
adjacent stages and never invents a duration.
