# Console UI Development

The Console is a React and TypeScript application under `console/`. It
contains Overview, Tenants/Components, Configs, Sessions, and the complete
Requests module. Node and npm are development tools only. The Rust binary
continues to embed the generated files in `assets/console.html`,
`assets/console.css`, and `assets/console.js`.

## Requirements

Use Node 24, matching the bundled aibox development image (`v24.19.0`). With
`nvm`, install and select it with `nvm install 24.19.0` and `nvm use 24.19.0`.
The repository commits `package-lock.json`, so install the exact dependency
tree with:

```sh
make console-ci
```

## Common Commands

```sh
make console-format     # Format frontend source files
make console-build      # Generate the three embedded assets
make console-test       # Vitest module and React interaction tests
make console-lint       # ESLint frontend source files
make console-check      # Format check, typecheck, build, node check, test, lint
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

Optional desktop browser smoke tests use a loopback-only Vite development
listener. They deliberately avoid committed screenshot baselines and remain
separate from the routine socket-free checks:

```sh
npm --prefix console run test:chrome        # Installed stable Chrome
npm --prefix console run test:browsers      # Firefox and WebKit behavior smoke
```

The browser smoke projects require the matching optional Playwright browsers:

```sh
npm --prefix console exec playwright install firefox webkit
```

There is intentionally no required Vite development server. Generate the
assets, then rebuild and launch the Rust binary so its `include_str!` inputs
are current:

```sh
make console-build
cargo run -- serve
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

`src/App.tsx` owns the persistent AIBox shell, URL-backed module navigation,
sidebar preferences, latest Management Operation surface, and protection for
unsaved Config edits across in-app, history, and browser navigation.
`src/SidebarUtilities.tsx` owns the sidebar resource catalog, brand icons, and
theme control.
`src/OverviewPage.tsx` owns Overview. `src/ManagementPages.tsx` owns
Tenants/Components, Configs, and Sessions. Desktop layouts support 1024px and
wider with a collapsible sidebar; narrow layouts use one-panel catalog/detail
navigation.

`src/controlApi.ts` owns the Console-internal Control API and startup CSRF
token; `src/api.ts` remains the Request API client. Their TypeScript
interfaces mirror the Rust JSON responses, including raw Summary timing,
Request Outcome, the top-level Coding Agent Session ID, the persisted Model
Protocol Summary and Record Assessment, and normalized Diagnostics groups.
Components receive an API interface so tests can use deterministic fakes
without sockets.

## Overview and Management Navigation

Overview is an operational resource map. Key facts combine Service health,
Managed Tenant count, Host Tenant availability, Config and Component health,
Requests, version, listen address, and the aibox Root. The Host Tenant is
reported separately as a console-only view and is never included in the
Managed Tenant count. Needs attention appears immediately below the key facts;
the complete structural Resource topology follows it, with Runtime below the
map. Runtime reports Docker
availability and exact local Runtime Image status (`built`, `missing`, or
`unknown`) with its reference, short ID, creation time, and size. Its explicit
actions are **Build** and **Build without cache**.

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

Management selections are shareable URL state. Tenants use `scope` and optional
`component`; Configs use `scope`, `agent`, either `current=1` or `config`, and
optional `file`; Sessions use repeated `scope` and `agent`, plus
`session_scope`, `session_agent`, and `session` for the selected Session. Dirty
Config file edits require confirmation before in-app navigation, history
navigation, or page unload can discard them.

The Tenants, Configs, Sessions, and Requests catalogs share one visual rhythm:
48-pixel toolbars, aligned leading icons, 14-pixel semibold primary text,
12-pixel secondary text, quiet destructive actions, and the same hover,
selection, and focus treatment. Request and Tenant rows have a 64-pixel minimum
height, while the single-line Config rows use 56 pixels. Session rows also start
at 64 pixels, but a prompt title may occupy two lines before it is truncated;
only those rows grow to accommodate the second line, and the complete title
remains available from the row title. Compact list-empty states and larger
detail-empty states use shared typography and spacing without changing each
module's domain-specific copy.

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
visual field model; Raw remains the explicit advanced view. Current Config
files always open in Raw, with Visual available only as an optional view when
supported. The editor header keeps Scope, Coding Agent, Config, and File visible
as separate context fields. **Apply to Current Config** is a one-shot projection
of fixed Config Fields, never an Active Config association. Confirmation,
success feedback, Last applied, and Config Drift use that same language and
retain the existing per-file commit and no-rollback semantics.

Requests uses `page` for its one-based page number, `record` for the selected
Request Record ID, and `tab` for `summary`, `request`, or `response`. Invalid
values are replaced with the canonical default URL. If a selected Request
Record no longer exists, the Console returns to the list, removes `record` and
`tab`, and leaves a dismissible failure notice.

Request Record pages contain 50 rows and intentionally have no filter query.
Rows use the shared 16-pixel Requests icon before method, target, HTTP status,
and an optional Record Assessment icon. The target is the primary text. Compact
metadata starts under the method with `Model·reasoning effort`, omitting the
suffix when reasoning effort is unavailable. This flexible label elides from
the end before the fixed-width `First Token/total timing` and timestamp group
on the right; a wider gap separates timing from the timestamp. At 430 pixels
and below, the right-hand group moves intact to a second metadata line and stays
right-aligned. Pagination shows the current and total page counts and restores
a valid page and focus target after deletion.

React hooks own pagination, selection, body offsets, request cancellation, and
the 5-second list / 3-second active-record polling. The Summary is
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
workspace space and scrolls independently.

## Record Assessment and Diagnostics

Request Outcome, HTTP response status, Provider Error, and diagnostic warnings
remain independent evidence. The aibox Service derives one Record Assessment for
consistent display; the browser never reclassifies a Record from `outcome`,
status, or Body content. [Request Proxy](sandbox.md#request-proxy) is the
canonical reference for which evidence produces Active, OK, Warning, or Error.

Active takes visual precedence until the Request Record terminates. Every
terminal warning elevates OK to Warning. Duplicate findings with the same
source, kind, and message collapse to their earliest observed time. The compact
primary cause uses this precedence: recording integrity, Provider Error,
proxy/transport, HTTP, then warnings. All findings remain visible regardless of
which one is primary.

The Record list leaves OK quiet and adds only one accessible Warning or Error
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
The Body routes named below are suffixes of `/_aibox/requests/api/records/{id}/`:

- The Request Record and the existing `request-body` / `response-body` routes
  contain the exact original application-visible bytes. Download always uses
  these routes and preserves those bytes.
- `Source` is the unformatted content after applying the supported HTTP
  `Content-Encoding`. The top-level Copy action copies this text regardless of
  the selected view.
- `Pretty` is derived in the browser from Source. It never changes or persists
  a Request Record.

The read-only `request-body-decoded` and `response-body-decoded` Requests module
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
than being treated as JSON. Event cards prefer a JSON payload's top-level
`type`, then its top-level `object`, before falling back to the SSE event type;
standard Chat Completions chunks therefore appear as `chat.completion.chunk`.

The `response-event-timings` route reads the existing best-effort
`response.events.jsonl` index on demand and returns only each sequence and its
complete-receipt offset. The browser joins those offsets to independently
parsed SSE Events by sequence. A missing, truncated, or partly malformed index
shows `Time unavailable` plus one warning and never suppresses Event data.
Active views request later sequences during their normal poll. Event time is
shown relative to Record start at millisecond precision, with the absolute
timestamp in a tooltip using the Requests module's existing timezone convention.
Content-encoded event streams retain their exact encoded bytes but do not get
an event index, because decoded Event boundaries cannot be mapped to exact raw
byte offsets; the decoded Pretty view remains available after a supported zstd
Body is complete. A zstd event stream is interpreted only after complete EOF
and does not synthesize First Token or per-Event timing.

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
Request Record format v3 has no v2 compatibility reader, migration, lazy backfill, or
read-time repair. Raw bodies and the best-effort SSE index remain available for
diagnosis.

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
protocol without final usage says it is still waiting for the provider, and a
terminal Record without usage says the completed response reported none.
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
First token, Duration, and Ended. The list and Timing summary display Request
End Time for terminal Records and `—` for active or interrupted Records; list
ordering follows descending canonical directory basename order. Active and
interrupted Records therefore appear first by start time, followed by terminal
Records by End Time; host and UUID break same-millisecond ties. Diagnostics and
the other detail tabs do not use this presentation.

When an older or partial Record lacks one Timing boundary but retains later
boundaries, the timeline combines the adjacent phases around that unknown
boundary and marks the combined interval `incomplete`. It continues with later
independently measurable stages instead of inventing a missing duration or
hiding the remaining checkpoints.

The Record list uses one-based pages of 50 from the current complete ordering.
Each poll opens only each Record's `summary.json`, which contains the complete
list projection and persisted Assessment. Detail reads remain strict over raw
request/response metadata, Body entries, and relevant ancestors, so an unsafe
or malformed raw entry can fail detail without hiding a valid list row.
Polling refreshes the page the Requests module is already on, even when
terminalization moves Records between pages. An empty page falls back through
earlier pages to the closest non-empty page or page 1. Multi-page selection
pauses polling; after deletion the Requests module returns to the lowest page
containing a selected Record and applies the same empty-page fallback.
Single-record deletion returns to its originating page. A confirmation dialog
pauses and cancels list, detail, and Body polling, then refreshes the applicable
views when it closes.
Active Records cannot be selected or deleted.
