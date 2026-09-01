# Console UI Development

The Console is a React and TypeScript application under `console/`. It
contains Overview, Tenants/Components, Configs, Sessions, and the complete
Requests module. Build it with the Node and npm installation in the current
development environment: host Node on macOS, or the selected Managed Tenant's
Node Component when developing inside AIBox. The Rust binary continues to
embed the generated files in `assets/console.html`, `assets/console.css`, and
`assets/console.js`.

## Requirements

Use Node 24 (`v24.19.0`). With
`nvm`, install and select it with `nvm install 24.19.0` and `nvm use 24.19.0`.
The repository commits `package-lock.json`, so install the exact dependency
tree with:

```sh
make console-ci
```

Rolldown and Lightning CSS resolve a platform-specific native binding, so a
`node_modules` installed on one platform cannot serve another. Separate macOS
and AIBox development clones naturally isolate both `node_modules` and Cargo's
`target/`; run `make console-ci` once in each clone. When sharing one Workspace
instead, give the container its own dependency tree with
`-m <host-dir>:/workspace/console/node_modules`.

## Common Commands

```sh
make format             # Format Rust and Console sources
make build              # Build embedded Console assets, then the Rust CLI
make test               # Run Rust and Console tests
make lint               # Lint Rust and Console sources
make check              # Run every socket-free project check
make rust-check         # Run only Rust format, test, and lint checks
make console-format     # Format frontend source files
make console-build      # Generate the three embedded assets
make console-test       # Vitest module and React interaction tests
make console-lint       # ESLint frontend source files
make console-check      # Format, type, Vitest, lint, contract, and asset checks
make console-contract   # Update committed Rust-owned wire bindings and samples
make console-contract-check # Compare committed contracts with a temporary export
make console-assets-check   # Compare committed assets with a temporary build
```

Routine Rust Request Proxy tests are part of `make check` and do not bind sockets.
Run the ignored Reqwest/TCP bridge smoke test only in a network-permitted host
or CI environment:

```sh
cargo test \
  request::tests::reqwest_tcp_smoke_preserves_bytes_headers_query_and_redirect_policy \
  -- --ignored --exact
```

That test binds loopback listeners and is intentionally excluded from the
default suite.

Frontend tests are layered. Query codecs, reducers, derivations, and formatting
are unit tested beside the module they cover, so a rule such as "an equal
version offers no Update" is stated once in a fast test rather than inferred
from a rendered page. A unit test follows its module into `catalog/`, `detail/`,
or `mutation/`; a page-level interaction test stays at the feature root. Each
domain keeps interaction tests that render its real page against a strict fake
of its own API interface, split by theme rather than collected into one suite.
Test doubles live with what they double. Sessions and Tenants keep one
`testSupport.tsx`; Requests and Configs have enough fixture data to separate it
from the harness that renders the page, so both name that pair
`testFixtures.ts` and `testHarness.tsx`.
`features/common/testFixtures.ts` holds the Tenant rows several features assume.
The Tenants fixture stays local because its Tenant Homes are load-bearing: one
sits under the Host Home to exercise `~/...` abbreviation and one outside it to
stay absolute. HTTP paths, queries, snake_case request bodies, and wire
normalization are asserted only by the `api/` adapter tests.

Because the tokens test asserts the density and stacking contracts directly,
browser specs do not restate exact pixel values. They assert behavior and
relative geometry instead — that names share a leading edge, that a split action
matches the width of its plain counterpart, that a menu stays right-aligned with
its trigger, and that nothing overflows horizontally.

Two optional Chromium smoke tests use a loopback-only Vite development
listener. They cover the responsive Requests workflow and Config editor modes,
avoid visual baselines and pixel-level geometry assertions, and remain separate
from the routine socket-free checks:

```sh
npm --prefix console run test:chromium
```

Download Playwright's browser once in each development environment:

```sh
npm --prefix console exec playwright install chromium
```

The project deliberately uses bundled Chromium rather than an installed Chrome
channel, so it runs the same way on macOS and Linux and inside the Runtime
Image; Google publishes no Linux arm64 Chrome. The Runtime Image carries the
shared fonts and ABI libraries that Chromium links against, so a Debug Shell
needs only the browser download.

There is intentionally no required Vite development server. Generate the
assets, then rebuild and launch the Rust binary so its `include_str!` inputs
are current:

```sh
make console-build
cargo run -- console
```

Open the embedded Console to check management modules and the real Request API.
To test the installed `aibox` command instead, run `make install` after
rebuilding the assets.

Do not edit the generated files in `assets/console.*` directly. Change the
source under `console/`—application code in `src/` or the HTML shell in
`index.html`—and rebuild before committing. The publish step rewrites the asset
references, so `assets/console.css` and `assets/console.js` are served as
`/_aibox/ui/app.css` and `/_aibox/ui/app.js`.

## Code Boundaries

`src/` has an explicit acyclic dependency graph. `domain/` depends on no other
Console layer. `api/` and `shared/` may depend on `domain/` but not on each
other. `features/common/` may depend on `api/`, `shared/`, and `domain/`, but on
no feature. A feature may depend on its own files plus those four layers; `app/`
composes every layer. ESLint's `no-restricted-imports` rules reject reversed
edges and cross-feature imports, and one further rule per concern subdirectory
rejects a `catalog/`, `detail/`, `mutation/`, `topology/`, or `components/`
module importing a sibling concern. Those subdirectory rules restate their
feature's patterns because `no-restricted-imports` options are replaced rather
than merged for a file two configs match. The concern list is read from
`src/features/<feature>/` at config load, so a newly added subdirectory is
governed the day it appears. `src/test` is exempt because its harnesses
compose whole pages. Files are imported through the `@/` alias; there are no
barrel files, so every import names the module it uses.

`features/common/` is for what several features share when `shared/` cannot hold
it, because the module needs both an `api/` wire type and a `shared/ui` type.
`tenantOptions.tsx` is the motivating case: it turns Control API Tenant rows
into Selection Menu options. It also holds `catalogSelection.ts`, the one
batch-selection reducer all four catalog pages compose, including its optional
per-row context for a catalog whose selection spans pages; `useElementRegistry.ts`,
the keyed focus registry pages use to move focus to a row; and
`testFixtures.ts`, the Tenant rows several features' tests assume. It sits
outside the features-may-not-import-each-other rule so every feature may use it,
and its own ESLint boundary forbids importing a feature back.

`app/App.tsx` owns the persistent AIBox shell. `app/routing/useConsoleRouter.ts`
owns the sole `history` and `popstate` integration, URL-backed module
navigation, and protection for unsaved Config edits across in-app, history, and
browser navigation. `app/useMobileNavigation.ts` owns the narrow-layout drawer
and `app/useOperationFeed.ts` the latest Management Operation. Pages receive an
immutable `search` snapshot plus one `onLocationChange(query, replace)` writer
for their own module; they keep only pure query codecs in
`features/<domain>/route.ts` and never subscribe to browser history themselves.
`app/SidebarUtilities.tsx` owns the sidebar resource catalog and theme control.
The Claude, OpenAI, GitHub, Node.js, Python, Rust, and Go brand SVGs are
committed under `shared/icons/brand/`; their Lobe Icons or Simple Icons source
versions and licenses are recorded beside the assets. All Console brand
rendering goes through the typed `shared/icons/brandIcons.tsx` registry, so
callers select a registered brand and explicit size without importing SVGs
directly. Runtime and build output do not depend on an icon package.

Each `features/<domain>/` holds a page controller, a thin page view, its query
codec, its workflow reducer, and the React-free modules or components that more
than one of its concerns reads. `catalog/`, `detail/`, and `mutation/`
subdirectories hold what exactly one concern uses, so a module's location states
who depends on it: `sessions/sessionSource.ts` stays at the root because the
catalog, detail, mutation, and route code all read it, while
`sessions/detail/sessionFormat.ts` moved down because only detail does.
The rule runs the other way too. `configs/viewTypes.ts` holds the catalog load
kind at the feature root because both `mutation/` hooks and the controller read
it, and `overview/viewTypes.ts` holds the `Tone` union because both `topology/`
and `components/` render from it. One concern reaching sideways for another's
module is the signal that the module belongs to the feature instead, so ESLint
forbids that edge and the diagnostic names the fix. A single reader moves the
other way: `overview/components/runtimeImage.ts` holds the Runtime Image tone
and short-id projections because only that concern reads them.
`features/overview/` keeps `topology/` and `components/` instead — it has
neither a catalog nor a detail pane.

Controllers own URL synchronization, latest-request ownership, dialogs, and
mutation orchestration. Every controller exposes a grouped view model rather
than a flat collection of setters, refs, mutable maps, or inferred controller
return types. The four catalog controllers group by `catalog`, `detail`,
`selection`, `mutations`, `dialogs`, and `feedback`, with feature-specific
`editor` or `components` groups. Overview has none of those panes, so it groups
by its own three concerns instead: `service`, `topology`, and `attention`.
A hook whose result spans more than one group returns those groups itself:
`useComponentActions` returns `components` and `dialogs`, and
`useSessionDeletion` returns `mutations` and `dialogs`. So the controller
spreads them instead of forwarding each field, and adding a field is one edit
rather than three. Their feature-local reducers own only cross-action workflow
state such as selection, dialogs, mutation phases, and typed outcomes, and
delegate the selection part to `features/common/catalogSelection.ts`. Resource
snapshots, loading, streaming reads, and AbortController ownership remain with
focused hooks.

`features/overview/` orchestrates
Overview and keeps rendering in `topology/TopologyCanvas.tsx` and
`topology/TopologyCanvasNode.tsx`. `topology/topologyModel.ts` is the stable
facade over the React-free core tree, layout/path, query/filter, and
health/attention modules; `topology/useTopologyInteraction.ts` owns focus,
keyboard navigation, expansion, and zoom while `useOverviewData.ts` owns its
two read-only resource lifecycles.
`features/tenants/` holds the Component
vocabulary, version comparison, Latest Release observation, and row derivation
in `componentCatalog.ts`, with `useTenantController.ts` composing separate
Tenant and Component catalog hooks plus actions and dialogs.
Its three Component hooks sit under `mutation/` because
`useComponentActions.ts` is their only reader: `useComponentCatalog.ts` owns the
selected Tenant's Component snapshot, `useComponentLatest.ts` owns the
Service-wide Latest Release snapshot, and `useComponentMenu.ts` owns anchored
menu focus and positioning.
`features/configs/` keeps the file editor, visual option editor, and CodeMirror
integration under `detail/`, since editing a Config is what its detail pane
does; `useConfigEditorSession.ts` coordinates file controllers and ordered
saves. Each file pane separates pure projection in
`configFileModel.ts`, draft/reveal/save ownership in
`useConfigFileSession.ts`, and rendering in `ConfigFilePane.tsx`;
`useConfigController.tsx` composes route selection, catalog, mutations,
dialogs, and the editor session. Named Config and Request Proxy rules remain in
`configCatalog.ts`. `features/sessions/` keeps the Transcript timeline reducer
in `sessionDetail.ts`, the Tenant-and-Agent source vocabulary in
`sessionSource.ts`, and detail streaming in `useSessionInspection.ts`;
`useSessionController.tsx` composes catalog, inspection, deletion, conversation
navigation, and routing, while `SessionCatalogPane.tsx`,
`SessionDetailPane.tsx`, and `SessionDialogs.tsx` keep the page view split by
stable interaction responsibility. `features/requests/` remains the reference
split: body decoding, Summary derivation, and list bookkeeping live in pure
modules; `useRequestsController.ts` owns list/deletion, and
`useRequestInspection.ts` composes focused detail, Body/timing, and download
resource hooks through one explicit Request identity and generation.

Domain CSS Modules own domain and responsive rules.
`shared/ui/layout/catalog.module.css` owns the shared catalog page frame,
split, toolbar, list row, status pill, and dialog structure; a domain module
extends one of those classes through `composes` only when it adds rules of its
own. Desktop layouts support 1024px and wider with a collapsible sidebar;
narrow layouts use one-panel catalog/detail navigation.

`src/domain/` contains only cross-feature identities and invariants such as
Tenant selection, Coding Agent identity, validated names, and stable key
codecs. It does not mirror every wire DTO or become a general application
model. Rust-generated, read-only wire types live under `src/api/generated/`.

`connectControlApi()` in `src/api/connect.ts` composes narrow Overview, Tenant,
Config, Session, Requests, and Operation interfaces over the single
`api/transport.ts` client. That client owns fetch, CSRF, NDJSON reading, and
binary Bodies; each `api/<domain>.ts` owns its paths, query strings, wire
bodies, conversion, and feature-facing port, and `api/operations.ts` owns the
SSE subscription. Generated TypeScript interfaces mirror the Rust JSON
responses, including raw Summary timing, Request Outcome, and the top-level
`Coding Agent Session ID`, the persisted Model Protocol Summary and Request Assessment,
and normalized Diagnostics groups. Pages receive only their domain API
interface so tests can use strict, deterministic fakes without sockets, HTTP
paths, snake_case, or wire-body knowledge. Adapter tests alone own those HTTP
details. The Rust-owned test manifest in
`console/src/api/generated/routes.ts` records the semantic route keys, methods,
and path templates used by adapter tests; those tests add path parameters and
queries through the shared manifest helper. Production clients remain
handwritten. `make console-contract-check`
exports bindings, routes, and samples to a temporary directory and compares
them byte-for-byte. `make console-assets-check` builds to a temporary directory,
runs the bundle budget,
and compares embedded HTML, CSS, and JavaScript. Both gates run under `make
console-check`.

Shared cross-domain behavior lives in `shared/hooks/`: `usePolling` runs an
interval that never overlaps its own request, `useNarrowDetailFocus` moves focus
into a one-pane detail, `useFailureNotifications` collects per-source failure
notices, and `useAsyncResource` loads values through a caller-supplied adapter
function. Batch selection is not here: it needs an `api/` wire key type, so it
lives in `features/common/catalogSelection.ts` as one reducer. `shared/lib/latestRequest.ts` owns abort and stale-response
ownership; shared code never imports API DTOs. `shared/lib/errors.ts` owns
message extraction and cancellation detection.

## Control API

Console pages and generated resources live below `/_aibox/ui/`; every
Console-internal data, Body, and event endpoint lives below `/_aibox/api/`.
Top-level resources use plural names. Reads use GET, collection creation may
use POST, and existing commands or destructive operations use
`POST /<resource>/<action>`. A simple global identifier belongs in the path;
compound Tenant-and-Agent selectors remain query or JSON body fields. The
Control API is internal to the Console and has no compatibility guarantee for
third-party clients.

The Requests interface is:

- `GET /_aibox/api/requests?page=N`
- `POST /_aibox/api/requests/delete`
- `GET /_aibox/api/requests/{id}`
- `GET /_aibox/api/requests/{id}/request-body?offset=N`
- `GET /_aibox/api/requests/{id}/response-body?offset=N`
- `GET /_aibox/api/requests/{id}/request-body-decoded`
- `GET /_aibox/api/requests/{id}/response-body-decoded`
- `GET /_aibox/api/requests/{id}/response-event-timings?after_sequence=N`

The list response uses `requests`; persisted Request metadata uses
`request_id`. Deletion accepts `{ "ids": [...] }` and returns
`{ "deleted": N }`.

Vite uses its built-in React and TypeScript transform without the React Vite
plugin. The development server therefore does not provide Fast Refresh; this
keeps the production and development dependency surface smaller without
changing the embedded Console behavior.

## Visual System

AIBox semantic CSS variables in `src/shared/styles/tokens.css` are the single
source of truth for Console color, surface, border, status, focus, code, shadow,
density, and stacking roles. Surfaces that escape their own layout use the
`--layer-*` tokens so their order stays centralized and testable. Both
light and dark palettes are complete and tested for parity and primary text
contrast. The default `system` theme follows `prefers-color-scheme`; explicit
light or dark choices are persisted, and `data-resolved-theme` selects the
palette before React renders.

Small AIBox-owned UI primitives in `src/shared/ui/` provide ordinary actions,
text inputs, text areas, checkboxes, native selects, and icon buttons. Shared
section headers and alert banners provide the same narrow presentation contract
for ordinary Console surfaces. They use native HTML semantics and CSS Modules
rather than a general visual or headless UI framework. Their props remain native
except for narrow AIBox contracts such as action tone and a checkbox's boolean
change callback. Use these primitives for ordinary controls; keep specialized
domain interaction with the structure that owns it.

Ordinary Console chrome shares a soft-surface ladder with catalog rows and
sidebar module navigation. Resting fills stay role-specific (toolbar actions and
filter triggers use a quiet `--control-rest` chip; sidebar Theme and collapse
controls rest transparent on the shell). Catalog rows, sidebar module links,
SelectionMenu options, and Theme menu options share the lightest wash,
`--surface-row-hover`, for both hover and selected/current states—shallow
enough that an in-row Apply chip on `--control-rest` still reads as a separate
control. Selection itself is marked by the left accent bar, a trailing check,
or a checkbox rather than a deeper fill. Soft action hover or open states use
the deeper `--surface-selected` wash with accent ink so a hovered control floats
above a selected row. Sidebar resource links and other chrome such as Overview
tiles keep the intermediate `--surface-hover` tier so a hovered GitHub link
never matches a list wash or a hovered action. Pressed (`:active`) soft actions
keep the hover colors and only sink slightly. Segmented view toggles
(Visual/Raw, Pretty/Source) rest transparent with muted ink and use the same
`--surface-selected` + accent pair when hovered or pressed. The danger tone
rests on a diluted danger chip (`--control-danger-rest`) with danger ink, and
hover uses `--danger-soft` with deeper `--danger-strong` ink—never a solid
danger fill with light ink. Only `focus-visible` draws a focus ring. Prefer an
icon followed by a short label. Keep icon-only controls for navigation chrome,
compact tools, and destructive Remove/Delete trash actions; keep text-only
controls for Cancel, Select all / Clear variants, Details, pagination, and
SelectionMenu footer actions. Those text-only controls still use the same
default soft-surface pair.
Self-explanatory actions do not show a visual tooltip; icon-only controls rely
on `aria-label`.

CSS Modules continue to own domain layout and presentation. Do not replace the
Overview topology, resource catalogs, Session conversation, CodeMirror,
diagnostics, custom filter/listbox behavior, or other domain structures with a
generic Card, Table, Tree, or Select solely for visual consistency. The native
`Dialog` also remains the modal container because its focus trap, Escape,
focus-restoration behavior, and tests are established. Lucide remains the
interface icon system. Focused libraries may own specialized behavior such as
code editing or Markdown rendering, but do not add a general visual framework,
external fonts, CDN assets, or a second competing token source.

All shared-control and tooltip styles are static. The request-scoped nonce from
the `aibox-csp-nonce` meta element remains required for CodeMirror-generated
styles under the embedded Console Content Security Policy. Frontend tests cover
semantic tokens, system-theme changes, CSP propagation, native-control
contracts, tooltips, and axe accessibility without committed screenshot
baselines.

The production JavaScript bundle is checked after every Console build. Its gzip
size may grow by at most 64 KiB (65,536 bytes) from the current architecture
baseline of 378,629 bytes; `scripts/check-bundle-budget.mjs` enforces the
444,165-byte maximum before generated assets are published.

## Overview and Management Navigation

Overview is an operational resource map. Key facts combine Service health,
Managed Tenant count, Host Tenant availability, Config and Component health,
version, listen address, and the AIBox Root. The Host Tenant is
reported separately as a console-only view and is never included in the
Managed Tenant count. Needs attention appears immediately below the key facts;
the complete structural Resource topology follows it, with Runtime below the
map. Runtime reports Docker
availability and exact local Runtime Image status (`built`, `missing`, or
`unknown`) with its reference, short ID, creation time, and size. Its explicit
actions are **Build** and **Build without cache**; Overview is the only Runtime
Image build entry point.

Resource topology is Tenant-centered: each Managed or Host Tenant contains its
Codex and Claude resources, while Components are Tenant-owned siblings of the
Coding Agents. Coding Agent branches expose Current Config presence, every
Named Config, and a Sessions branch whose Transcript discovery remains on
demand. Requests and Runtime are global
and therefore never appear in the Tenant tree.

The topology is a left-to-right node-and-edge canvas with separate node-body
navigation and output-side disclosure controls. It grows to its complete scaled
height so Overview remains the only vertical scroll container; native
horizontal scrolling appears only when a narrow layout or manual zoom exceeds
the available width. The topology never converts vertical wheel input into
horizontal movement.

Desktop initializes with the whole graph fitted to the viewport, while narrow
layouts retain 100% scale. Zoom is bounded to 65%-150% in 10% steps, with Fit
and 100% reset controls in the sticky topology toolbar. Fit mode follows layout
and viewport width changes; manual zoom remains stable through later topology
changes. Expansion and zoom compensate the relevant scroll positions to keep
the operated or active node anchored. Tenant roots are ordered with the Host
Tenant first, `default` second, and remaining Managed Tenants by display name.
Service startup normally guarantees that the protected Default Managed Tenant
exists; Config and Session selectors never synthesize missing Tenant rows.
Every Tenant, Coding Agent, Named Config, and installed Component branch opens
initially so the map explains the complete resource structure. Expanding a
Sessions branch is still the explicit trigger for Transcript discovery. Search
preserves the graph layout, expands
matching paths, highlights matches, and dims unrelated nodes. Needs attention
instead prunes and reflows the graph to warning and error paths. Hover or
keyboard focus traces a path back to the Service root, and diagnostic details
use lightweight popovers. The topology retains ARIA tree semantics, roving
focus, and arrow-key navigation.

Overview status polls every 15 seconds while the document is visible; topology
changes only on its explicit refresh. Expanding Sessions performs discovery
only and does not parse Transcript content. Topology expansion, filters, zoom,
and viewport position are deliberately not persisted.

Management selections are shareable URL state. Tenants use only `tenant`;
historical `component` parameters are ignored and removed from the URL when the
Tenant page loads. Component navigation therefore opens the Tenant-level
diagnostics list, where an individual row can be expanded with **Details**.
Configs use `tenant`, `agent`, either `current=1` or `config`, and optional
`file`; Sessions use repeated `tenant` and `agent`, plus
`session_tenant`, `session_agent`, and `session` for the selected Session. Dirty
Config file edits require confirmation before in-app navigation, history
navigation, or page unload can discard them.

The Managed Tenant Components catalog contains Codex, Claude, their two
statuslines, Node.js, Python, Rust, and Go; the Host Tenant contains only the
two statuslines. A fixed 64-pixel detail toolbar presents `Components` as its
primary task and keeps the selected Tenant, abbreviated Home, installed ratio,
nonzero issue count, Latest Release check time, and the Check for updates
action as compact context. Container-width breakpoints progressively hide Home,
the installed ratio, and the visible check time while retaining the Tenant,
nonzero issues, and check action. Managed catalogs use three presentation-only
groups: Coding Agents, Statuslines, and Runtimes & Toolchains. They are
unframed sections with short labels and separators rather than cards; the Host
catalog omits the two empty groups.

`Check for updates` refreshes local state and the Service-wide Latest Release
snapshot and appears as an ordinary icon-plus-label default action. The page
header owns the not-checked or last-checked observation; rows do not repeat a
Service-wide `Latest not checked` placeholder. Component rows are quiet,
non-selectable 52-pixel list items with a fixed two-line information block. The
first line contains a bare Component brand icon and compact name; statuslines
use the shared waveform icon. The second line keeps the local Installed State
and only the necessary observed Latest Release. Equal Installed and Latest
versions do not repeat Latest, and missing observations have no placeholder.
Normal confirmation labels such as `Definition current`, version `Current`, and
`Update available` stay silent; an available Update is expressed by its action,
while exceptional inspection states retain their badges. Diagnostic text is
available from a visible text-only **Details** control. Group and Component
order remains stable across state changes.

The horizontal action group sits independently at the row end and is centered
against both information lines rather than aligned with either line. Install,
Update, Repair, Restore, and Retry inspection all use the shared default action
pair. Remove is an icon-only danger action with destructive confirmation.
Version menus fit their action phrase and remain right-aligned with the split
trigger. A split Install or Update action shares one default pair across both
halves so hover fills the whole control. Default installation is the primary
half of a split action whose menu accepts an exact `X.Y.Z` version. A checked
versioned Component with a higher Latest Release uses the same split treatment
for Update: the primary action selects Latest, while the menu accepts any exact
version newer than Installed. The installer remains responsible for determining
whether that version exists. Equal versions create no Operation; downgrades
retain the explicit Remove-then-install workflow. Statusline `modified` state
shows a `Modified` badge and an unversioned Update action. Remove remains a
visible, separate destructive action with confirmation. Unmanaged state is
diagnostic only and exposes neither Install nor Remove, because the Console
cannot claim foreign launchers safely.

The Tenants, Configs, Sessions, and Requests catalog shells share one visual rhythm:
44-pixel ordinary toolbars, 30-36 pixel controls, aligned leading icons,
14-pixel semibold primary text,
12-pixel secondary text, the shared soft-surface default and danger action
pairs, and the same hover, selection, and focus treatment for their navigable
rows. On Configs and Sessions, the Tenant and Coding Agent filters are two
independent selectors with an 8-pixel gap between them and a 14-pixel gap before
the toolbar actions; each is capped near 112 pixels wide and shares the quiet
resting fill plus soft-surface hover and open states of other toolbar controls.
The Tenant
Component list is the deliberate exception: its rows are static and use
unframed sections, bare Component icons, a 52-pixel desktop rhythm, and
row-local progress in the second information line for the active Component
Operation.
Its detail pane uses container-width breakpoints to move the intact horizontal
action group below the information block before allowing it to wrap, without
changing the Console's master/detail navigation. Each selected detail starts
with the same pale shell surface and divider while retaining its
module-specific structure.
Single-line catalog rows use a 44-48 pixel rhythm. Rows containing a second
metadata or Conversation Message preview line use a 52-56 pixel rhythm, while
coarse-pointer targets remain at least 44 pixels. The complete Session title
remains available from the row title. Selecting a Session updates its shareable detail URL without
restarting the unchanged catalog lifecycle, so the catalog keeps its scroll
position and the selected row remains in context. Session source labels place
the Tenant and Coding Agent together with a space; detail adds the Session ID
after the same compact source. Compact list-empty states and larger detail-empty
states use shared typography and spacing without changing each module's
domain-specific copy.

The Sessions detail is a compact header with a sticky `Conversation`/`Details`
tab bar over independently scrolling content. The header contains the Session
title, Tenant and Coding Agent source, started time, compact message/tool
counts, observed duration, exceptional reading state, back, and refresh. Session
deletion remains in the Sessions catalog and batch-selection flow; the detail
view has no duplicate destructive action. The selected tab is shareable URL
state through `tab=conversation|details`, defaulting to Conversation.

Conversation is a centered reading stream with a compact user-message navigator.
On desktop the navigator is a vertical rail beside the stream; each point maps
to one user Conversation Message, follows the current scroll position, and uses
that message's first readable line as its accessible label. Narrow screens use
the same anchors in a horizontally scrollable strip above the conversation.

User Conversation Messages remain separate, render as right-aligned plain-text
bubbles, and preserve line breaks. Agent Conversation Messages form the wider
left reading stream and render safe GFM Markdown with raw HTML disabled, secure
absolute HTTP(S) links, and copy controls for fenced code. Root-relative,
relative, anchor, and non-HTTP(S) destinations render as inert inline code so
they cannot enter the same-listener Request Proxy. Adjacent Agent messages may
merge only when no secondary Transcript record separates them. Message
timestamps stay compact and expose the complete timestamp through their title.

Consecutive Tool Activity and Transcript Evidence entries remain in native order
but appear as one collapsed `Transcript activity` disclosure. Its summary reports the
item count, bounded activity labels, and whether diagnostics are present;
expansion reveals the individual safe summaries and on-demand evidence controls.
Reasoning and thinking show a hidden-internal diagnostic state but never expose
their raw text. Activity disclosures reset to collapsed whenever the Session is
reloaded.

Details contains separate Session and Diagnostics sections. Session facts include
Tenant, Coding Agent, Session ID, relative Transcript path, started/last-event
times, duration, file size, and only the available working directory, model
provider, and CLI version. Diagnostics show non-zero parser counts and warnings;
normal Sessions say `No transcript diagnostics.`. A warning in Conversation is a
single link to Details rather than an inline list. The NDJSON detail stream
renders frames as they arrive; manual refresh retains the old content until a
new stream succeeds. Missing-readable-message, tool-only, evidence-only, and
partial Transcript states have explicit copy. Long Conversations open at the
beginning and provide a jump-to-latest control after scrolling away from the
end. On narrow screens the Sessions module keeps its list/detail single-page
switch, the detail view becomes full width with its back control retained, and
the desktop message rail becomes the horizontal message navigator.

Console typography follows interface role before raw data type. Navigation,
headings, controls, explanatory copy, catalog titles, and catalog metadata use
the shared system sans-serif stack. Technical facts use the shared system
monospace stack: HTTP methods, detail URLs, paths, identifiers, Config file
names, timestamps, durations, code, raw Bodies, Transcripts, and logs. Catalog
metadata has a 12-pixel sans-serif baseline; only its timestamp and duration
fragments switch to monospace with tabular numerals. Page-level section and
dialog headings use 16-pixel semibold text, while nested Request detail section
headings remain deliberately smaller and bolder. Non-code interface text does
not fall below 12 pixels. Specialized editor, JSON tree, Body, Transcript, and
log line heights remain local because their reading modes differ.

Named Config files open in the Visual editor when the Control API supplies a
Visual Config Option model; Raw remains the explicit advanced view. Visual uses
compact desktop label-and-control rows that stack on narrow screens. Native
paths stay in Raw, descriptions use hover-and-focus help tooltips, and required
Options use an accessible `*`. Closed enum controls use declared native values;
optional enums and booleans expose **Default** to omit their Config Field.
Current Config files always open in Raw; Visual is available only for supported
Named Config main files. The editor header keeps Tenant, Coding Agent,
Config, and File visible as separate context fields. **Apply to Current Config**
is a one-shot projection of fixed Config Fields, never an Active Config
association. Confirmation, success feedback, Last applied, and Config Drift use
that same language and retain the existing per-file commit and no-rollback
semantics.

Requests uses `page` for its one-based page number, `request` for the selected
Request ID, and `tab` for `summary`, `request`, or `response`. Invalid values
are replaced with the canonical default URL. If a selected Request no longer
exists, the Console returns to the list, removes `request` and
`tab`, and leaves a dismissible failure notice.

Request pages contain 50 rows and intentionally have no filter query.
Rows use the shared 16-pixel Requests icon before method, target, HTTP status,
and an optional Request Assessment icon. The target is the primary text. Compact
metadata starts under the method with `Model reasoning effort`, omitting the
suffix when reasoning effort is unavailable. This flexible label elides from
the end before the fixed-width `First Token / total timing` and timestamp group
on the right; a wider gap separates timing from the timestamp. At 430 pixels
and below, the right-hand group moves intact to a second metadata line and stays
right-aligned. Pagination shows the current and total page counts and restores
a valid page and focus target after deletion. In selection mode the toolbar
places Cancel on the left, the selected count and Select page in the center,
and Delete on the right, then copies the pagination bar underneath so a
consecutive page walk can stay at the top of the list.

`requestsWorkflow.ts` owns batch selection, the confirmation dialog, and the
delete in flight, recording the page each Request was selected on so a batch
spanning pages returns to the earliest one. React hooks own pagination, body
offsets, request cancellation, and the 5-second list / 3-second active-Request
polling. The Summary is
the default detail tab, and request/response bodies load only for the visible
body tab. Formatting and binary decoding stay in pure functions covered by
Vitest.

Request and action failures use one top-centered notification stack keyed by
list, inspection, and action source. A notice remains for eight visible
seconds, pausing while hovered, focused, covered by a confirmation dialog, or
while the page is hidden or unfocused. Repeated polling failures notify once
until that source succeeds. List, detail, Body, and download failures offer a
scoped retry; destructive actions require confirmation again. Decoding and SSE
timing degradation remain local to the affected Body view.

The latest Management Operation remains visible across modules in the bottom
task dock. Starting a new Operation or receiving a new failure expands it;
polling does not reopen a dock the user collapsed. Expanded output reserves
workspace space and scrolls independently. Runtime Component installer
Operations run their shell installers with command tracing enabled, so the
dock receives each installer command together with the live stdout and stderr
that it produces.

## Request Assessment and Diagnostics

Request Outcome, HTTP response status, Provider Error, and diagnostic warnings
remain independent evidence. The AIBox Service derives one Request Assessment for
consistent display; the browser never reclassifies a Request from `outcome`,
status, or Body content. [Request Proxy](sandbox.md#request-proxy) is the
canonical reference for which evidence produces Active, OK, Warning, or Error.

Active takes visual precedence until the Request terminates. Every
terminal warning elevates OK to Warning. Duplicate findings with the same
source, kind, and message collapse to their earliest observed time. The compact
primary cause uses this precedence: recording integrity, Provider Error,
proxy/transport, HTTP, then warnings. All findings remain visible regardless of
which one is primary.

The Request list leaves OK quiet and adds only one accessible Warning or Error
icon beside the independent HTTP status. The detail header keeps `HTTP 200`
green even when a red Provider Error tag is present; the tag names the primary
cause, exposes its message as a tooltip, and adds `+N` for further findings.
Missing response metadata is neutral rather than an error by itself.
Diagnostics renders the normalized `Proxy / transport`, `HTTP response`,
`Model API`, and `Warnings` groups supplied by Rust.

## Body Views

The Request and Response tabs open in `Pretty` when the complete decoded Body
has a renderer. The Requests module keeps three deliberately separate
representations.
The Body routes named below are suffixes of `/_aibox/api/requests/{id}/`:

- The Request and the existing `request-body` / `response-body` routes
  contain the exact original application-visible bytes. Download always uses
  these routes and preserves those bytes.
- `Source` is the unformatted content after applying the supported HTTP
  `Content-Encoding`. The top-level Copy action copies this text regardless of
  the selected view.
- `Pretty` is derived in the browser from Source. It never changes or persists
  a Request.

The read-only `request-body-decoded` and `response-body-decoded` Requests module
routes accept no coding, an empty coding, `identity`, or one case-insensitive
`zstd` or `gzip` coding. Rust streams encoded decoding from a blocking worker;
unsupported or combined codings do not alter the raw Body. An active encoded
Body must be complete before it can be decoded. Source can show a partially
received identity Body, while an incomplete encoded Body is explicitly shown as
encoded hex until decoding is possible.

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
than being treated as JSON. Event cards prefer a JSON payload's top-level
`type`, then its top-level `object`, before falling back to the SSE event type;
standard Chat Completions chunks therefore appear as `chat.completion.chunk`.
The cards form a labeled semantic list so assistive technology can identify
the SSE Event collection and its boundaries.

The `response-event-timings` route reads the existing best-effort
`response.events.jsonl` index on demand and returns only each sequence and its
complete-receipt offset. The browser joins those offsets to independently
parsed SSE Events by sequence. A missing, truncated, or partly malformed index
shows `Time unavailable` plus one warning and never suppresses Event data.
Active views request later sequences during their normal poll. Event time is
shown relative to Request start at millisecond precision, with the absolute
timestamp in a tooltip using the Requests module's existing timezone convention.
Content-encoded event streams retain their exact encoded bytes but do not get
an event index, because decoded Event boundaries cannot be mapped to exact raw
byte offsets; the decoded Pretty view remains available after a supported zstd
or gzip Body is complete. A content-coded event stream is interpreted only
after complete EOF and does not synthesize First Token or per-Event timing.

## Protocol Summary

For recognized model requests, the Request Proxy also records an optional
top-level `summary.coding_agent_session_id`. OpenAI Responses and OpenAI Chat
Completions prefer the first nonempty UTF-8 `session-id` request-header value
and fall back to `x-claude-code-session-id`; Claude Messages uses the reverse
precedence.
Only those header names are considered, matched case-insensitively. Unknown
protocols do not derive this value, bodies are never searched for it, and
missing values are not backfilled.

The Request Proxy derives model, reasoning effort, response mode, First Token,
final Token Usage, and Provider Errors from native OpenAI Responses, OpenAI Chat
Completions, or Claude Messages data while it records the exchange. Stable facts
are atomically checkpointed in the optional `summary.protocol` object. List and
detail APIs return that same object without parsing request/response bodies.
Request format v4 has no format v3 compatibility reader, migration, lazy
backfill, or read-time repair. Stop the old Service, optionally back up the
collection, and manually remove `$AIBOX_ROOT/requests` before the first start
of an upgraded Service. The new Service recreates an empty collection.

For a recognized streaming response, First Token is the offset at which the
first trim-nonempty SSE `data:` line not beginning with `[DONE]` is completely
received. It is deliberately compatible with common relay accounting rather
than a claim that tokenizer or semantic output has arrived: ping, error,
malformed JSON, empty-delta JSON, Claude `message_start`, OpenAI
`response.created`, and role-only Chat Completions data all qualify. Comments,
other SSE fields, blank data, and `[DONE]` prefixes do not. A line split across
body chunks uses the arrival time of its terminator; an unterminated final line
uses the last body arrival time at EOF. Unknown protocols and non-streaming
responses have no First Token.

Chat Completions holds streamed usage in memory until an exact trim-equal
`[DONE]` data value or a structured stream error makes the protocol terminal.
It maps prompt and completion usage into the existing OpenAI billing categories
and warns about inconsistent totals or requested final usage that never arrives.
The browser displays each raw chunk independently; it does not concatenate
choice content or tool-call arguments into a reconstructed conversation.

The browser never parses model bodies for Summary. It receives decimal
nanosecond offsets, uses `BigInt` to build Timing Stages on a shared axis, and
falls back to a single Response body stage when a protocol has no observable
First Token. Unknown protocols retain generic Timing and diagnostics while
Token Usage states that the protocol is unsupported. A recognized active
protocol without final usage says it is still waiting for the provider. A
successfully completed Request with a terminal protocol response says that the
completed response reported no usage; a failed, interrupted, or
protocol-incomplete Request instead says that usage was not reported before the
request ended.
For streaming responses with First Token, the interval after that checkpoint
is named `Response stream` rather than implying that every byte is model
output.

The detail Summary presents Model and Token Usage in one pale hierarchy card.
The effective-or-requested model is the primary value, followed after a space by
a weaker reasoning effort. A `Streaming` or `Non-streaming` badge follows when
that fact is available. Coding Agent Session ID remains on its own secondary row
with an inline copy control. A missing model says `Not reported`, or `Detecting…` while
active; missing optional qualifiers are omitted.

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
First token, Duration, and Ended. The list and Timing summary display Request
End Time for terminal Requests and `—` for active or interrupted Requests; list
ordering follows descending canonical directory basename order. Active and
interrupted Requests therefore appear first by start time, followed by terminal
Requests by End Time; host and UUID break same-millisecond ties. Diagnostics and
the other detail tabs do not use this presentation.

When an older or partial Request lacks one Timing boundary but retains later
boundaries, the timeline combines the adjacent phases around that unknown
boundary and marks the combined interval `incomplete`. It continues with later
independently measurable stages instead of inventing a missing duration or
hiding the remaining checkpoints.

The Request list uses one-based pages of 50 from the current complete ordering.
Each poll opens only each Request's `summary.json`, which contains the complete
list projection and persisted Assessment. Detail reads remain strict over raw
request/response metadata, Body entries, and relevant ancestors, so an unsafe
or malformed raw entry can fail detail without hiding a valid list row.
Polling refreshes the page the Requests module is already on, even when
terminalization moves Requests between pages. An empty page falls back through
earlier pages to the closest non-empty page or page 1. Multi-page selection
pauses polling; after deletion the Requests module returns to the lowest page
containing a selected Request and applies the same empty-page fallback.
Selection mode keeps Cancel on the left, the selected count and Select page
centered, and Delete on the right, then duplicates the footer pagination under
that toolbar so a consecutive page walk can stay at the top of the list.
Single-Request deletion returns to its originating page. A confirmation dialog
pauses and cancels list, detail, and Body polling, then refreshes the applicable
views when it closes.
Active Requests cannot be selected or deleted.
