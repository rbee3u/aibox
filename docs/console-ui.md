# Console UI Development

The Console is the React and TypeScript application under `console/`. The Rust
Service embeds its generated HTML, CSS, and JavaScript from `assets/console.*`.
These three build outputs are ignored by Git and generated locally.

This document owns frontend development, architecture, testing, and interaction
contracts. Domain behavior belongs in [Configs](configs.md),
[Tenants](tenants.md), or [Filesystem Sandbox and Mounts](sandbox.md).

## Development

Use a Node version accepted by `console/package.json`; `make console-ci`
installs the committed lockfile. Run it once per environment and again after
frontend dependency changes. Native build bindings are platform-specific,
so do not share one `node_modules` between host and container platforms. When a
Workspace is shared, mount a separate directory over
`/workspace/console/node_modules`.

Use `make help` for the authoritative target list. The main targets are:

| Task                             | Command                 |
| -------------------------------- | ----------------------- |
| Install dependencies and AIBox    | `make install`          |
| Full socket-free check           | `make check`            |
| Console-only check               | `make console-check`    |
| Build embedded assets            | `make console-build`    |
| Update Rust-owned wire artifacts | `make console-contract` |

`make install` always runs `npm ci` before building and installing AIBox.
Other Make targets reuse the installed dependencies. Every Make invocation
that compiles Rust first builds the Console, including focused Rust checks
and contract generation. One shared prerequisite builds the assets once per
invocation, even when several targets need them. Build failures stop the
dependent Rust commands. The build retains the gzip bundle-size budget and
published HTML validation.

Direct Cargo commands do not build the Console. Generate assets first, and
regenerate after changing frontend source or switching branches:

```sh
make console-build
cargo run -- console
```

Edit `console/index.html` and `console/src/`, never generated
`assets/console.*`. Publishing rewrites the asset URLs below `/_aibox/ui/`.

## Architecture

Console dependencies point inward. ESLint enforces these boundaries, and source
imports use the `@/` alias so dependency edges remain visible.

| Layer                 | May depend on           | Ownership                                         |
| --------------------- | ----------------------- | ------------------------------------------------- |
| `domain/`             | itself                  | Cross-feature identities and invariants           |
| `api/`                | `domain/`               | HTTP, wire conversion, and domain API ports       |
| `shared/`             | `domain/`               | API-independent UI, hooks, and libraries          |
| `features/common/`    | inner layers            | Shared feature machinery needing API and UI types |
| `features/<feature>/` | inner layers and itself | One product feature                               |
| `app/`                | every layer             | Shell, routing, theme, and composition            |

`api/` and `shared/` do not depend on each other. Features do not import other
features or `app/`; `features/common/` cannot import a feature back. `src/test/`
may compose complete pages. Do not add barrel files.

Concern directories have single ownership and do not import siblings. Move
values shared by concerns to the feature root, and values shared by features to
the narrowest valid inner layer.

Each feature owns its route codec, controller, grouped view model, view, and
workflow state. Focused hooks own loading, polling, streaming, and cancellation.
Only `app/` integrates browser history and composes the persistent shell; pages
receive a location snapshot and navigation writer. Config dirty state must be
guarded across in-app, history, and browser navigation.

## Control API and Generated Assets

Console pages and assets live below `/_aibox/ui/`; Console-internal APIs live
below `/_aibox/api/`. The Control API is not a public integration surface.

The shared transport owns fetch, CSRF, NDJSON, and binary bodies. Domain
adapters own paths, queries, wire conversion, and feature-facing ports. Features
never import transport or generated wire types.

Rust owns the wire types, route manifest, and contract samples under
`console/src/api/generated/`. Declare each route once in
`service/control/routes.rs`; production clients remain handwritten. Run
`make console-contract` only for intentional wire changes. Contract checks
regenerate into temporary directories and compare byte-for-byte with the
committed wire artifacts. Compiled Console assets are rebuilt during checks
and are not compared against Git-tracked bundles.

## Testing

Keep a rule in the narrowest useful layer:

1. Pure tests cover codecs, reducers, derivations, formatting, and state.
2. Feature tests render the real page against a strict domain API fake.
3. Adapter tests cover HTTP and wire behavior.
4. Optional Chromium tests cover real layout or browser behavior.

Tests follow their modules; page interactions stay at the feature root. Keep
suite-only doubles local and feature-wide support inside that feature. Share
cross-feature fixtures only when the production concept is also shared.

Do not repeat pure rules in browser tests. Geometry tests assert behavior and
relative layout, not design-token values or pixel snapshots. Routine Rust and
Console tests remain socket-free.

Playwright uses bundled Chromium. Install and run it explicitly:

```sh
npm --prefix console exec playwright install chromium
npm --prefix console run test:chromium
```

These optional tests start a loopback-only Vite listener. The Runtime Image
contains Chromium ABI libraries and fonts, but not a browser.

## Shared Interaction Contracts

### Navigation and Layout

Use semantic roles from `shared/styles/tokens.css`; keep both palettes complete.
Shared primitives wrap general controls and layout, while domain interactions
stay with their feature.

Detail content takes a measure from the centralized ladder rather than the
available width, so a wider viewport gains whitespace instead of spreading the
same content thinner. Measured content is left-aligned, and a section that owns
a background or divider stays full-bleed while the wrapper inside it takes the
measure. Trailing metadata belongs beside the value it describes; only
container-level headers and toolbars separate a title from independent actions.
Row actions in measured content align to the measure edge. The Components
detail pane uses the full available width for its header, grouped rows, and
loading placeholders, with consistent horizontal padding; its actions align
to their container's trailing edge. Code editors, JSON trees, and raw bodies
stay unmeasured.

Every type role takes a step from the centralized size scale rather than a raw
size. A section title is a quiet label for the group below it, so it stays
smaller and dimmer than the rows it groups and a row title remains the primary
scanning target. Bold is reserved for the page title, which is an accessible
name on desktop and a visible top bar heading when narrow. Headings inside
Agent output are content, not chrome: they carry their own scale and never
borrow the section or panel roles.

Chrome and content are different materials. The fill ladder runs canvas, then
shell, then surface, and every step is perceptible on its own, so a sidebar,
toolbar, detail header, or catalog panel reads as a region without a hairline
having to say so. An inset recess sits below the content plane but never as
deep as chrome. A light theme cannot go brighter than its content plane, so
elevation there is carried by a shadow with a contact layer rather than a
lighter fill, and only the dark theme gives a raised surface its own step.
Hover and selection are ink overlays rather than fixed fills, because one token
has to darken a content row and a chrome toolbar by the same amount; selection
always reads stronger than hover.

Spacing steps run on a two-pixel base, and the steps that separate structure
carry role names rather than numbers, because a numbered step says nothing
about when to reach for it. A section title takes the same gap to its content
in every module, and the gap to the next section is several times larger, so
one distance can never mean "these belong together" in one module and "a new
group starts here" in another.

The accent answers "where am I" and "what am I engaging with": the current
module, a primary action, a hovered link, a focus ring. It does not mark every
navigable thing, because a table whose cells all navigate would spend the
entire colour budget saying nothing. Links in such a table carry an arrow or an
underline and take the accent only on hover. The focus ring is a variant of the
accent rather than an indigo of its own. Each text role is quieter than the one
above it, so a role named faint can never outrank one named muted.

Icons come from one line family, at one stroke weight, in five sizes that live
in a TypeScript scale rather than a stylesheet: the icon library takes a numeric
prop, so a CSS token can never reach an icon and a call site left to invent its
own number will. Third-party brand marks are the one legitimate second family.
The steps carry sizes rather than role names because the same step serves a row
identity, an operation status, and a topology node; which step each role takes
is documented at the scale. A shared component's icon slot takes one size
wherever it appears, and explicit stroke weight is reserved for the mark that
signals selection.

Anything that answers a pointer at rest also answers a press, with one ink
overlay that runs deeper than every resting state so the press still reads on a
selected row. Buttons carry it as a layer over their own fill rather than a
replacement, which is how one token serves a Ghost and a Danger button alike.
A field is the exception: the pointer going down is what focuses it, and the
ring says so, so a separate press would announce the same event twice.

Focus is one ring at one distance, both held as tokens, and it belongs to
keyboard focus rather than to every click; the skip link is the one deliberate
exception. A control that suppresses the ring owes a replacement, drawn on
whichever box the user is actually on rather than on whatever element happens
to hold the tab stop.

Motion is timed from two duration tokens and nothing invents a time of its own.
Anything that changes fill under the pointer eases it, so how an interaction
feels does not depend on which tag the component happened to reach for. A press
is the exception in the other direction: it arrives instantly and eases back on
release, which the fast token achieves by dropping to zero while an element is
active rather than by every rule restating it. The reduced-motion opt-out lives
in one place; a second copy beside a component reads as local control over
something that is already settled globally.

Type sizes come only from the ladder, and no rule writes one inline. Within a
strip of related values every value takes the same step: these are numbers the
reader compares by reading them, and tabular figures line up only at a shared
size, so promoting one of them spends the alignment that makes the row
scannable. Emphasis among siblings comes from position, weight, or a label
instead. The page and panel title steps belong to titles; a field value that
borrows one outranks the thing it sits inside, and the console is dense enough
that a single promoted value reads as the page's subject.

A state that lasts draws its fill from the same ladder a passing one does, so
the two can be ranked against each other. Marking the current module with an
accent tint instead put it below the hover an idle module gets from a passing
pointer, and the tint was never measured against the shell it was painted on.
The ladder is checked on both the content surface and the shell for that reason.
A lasting state also says so more than one way: colour alone leaves the answer
to a reader who can separate indigo from slate. Chrome states its own target
sizes rather than inheriting them from whichever token a rule reached for, and a
control's height is decided by one thing — either its padding or a height token,
not both.

A control whose only content is an icon owes the reader its name, and gives it
to a pointer and a keyboard alike: after a hover delay, and immediately on
focus, since focus has no other way to read the glyph. The name is one string
serving as both the accessible name and the tooltip text, so the two cannot
drift. Withholding it is not a neutral choice — a trash icon in a list of fifty
rows carries no clue which row it ends, and the answer is already written.

One anchored-tooltip mechanism serves two roles, and the surface says which.
A dark label names a control in a few words, matching what a pointer expects
from a tooltip anywhere else. A light bordered panel explains a condition at
length, and belongs with the Console's other floating surfaces because that is
what it is. The distinction is content, not caller: reach for the dark label
whenever the tooltip is a name, however the control is built.

The desktop shell has one persistent, collapsible sidebar and no repeated module
title bar. Module changes update the document title and move focus to the page
heading; initial load and query-only changes preserve focus. A skip link targets
the main landmark.

Narrow layouts use a drawer and one-panel catalog/detail navigation without a
second route model. The drawer has an explicit close control and supports Escape
and pointer dismissal. Catalog details retain a back action. Invalid queries
canonicalize to a safe state.

Catalogs share selection, pagination, focus recovery, empty states, and dialogs.
Opening a detail does not restart an unchanged catalog or discard its scroll
context. Responsive controls must remain named, keyboard accessible, and usable
with coarse pointers without horizontal page overflow.

### Selection and Async Work

Batch selection is explicit: an empty selection never means all, and select-page
affects only the visible page. Destructive actions require confirmation and
restore focus to the closest surviving target. Protected resources and active
Requests remain unavailable for deletion.

Menus and split actions support keyboard navigation, Escape, outside-click
dismissal, anchored positioning, and focus return. Avoid duplicate destructive
entry points when selection mode already owns deletion.

Failures use the shared notification stack, keyed by resource or action.
Repeated polling failures notify once until recovery, and scoped read failures
offer scoped retry. Destructive failures require confirmation again.

The latest Management Operation remains available across modules. Polling does
not overlap itself or reopen a collapsed operation panel. Route changes and
dialogs cancel obsolete work; late responses cannot replace a newer generation.
Degraded decoding or evidence stays local to the affected view.

### Accessibility and Content

Prefer native roles, labels, focus order, and keyboard behavior over ARIA. Every
icon-only control has an accessible name, and supplemental help works with both
focus and hover.

Use system sans-serif for interface prose and system monospace for identifiers,
paths, URLs, methods, timestamps, Configs, code, bodies, Transcripts, and logs.

Render Agent Conversation Messages as safe GFM Markdown with raw HTML disabled.
Only secure absolute HTTP(S) links are active; other links remain inert so they
cannot enter the same-listener Request Proxy. User messages and raw evidence
preserve plain text and line breaks.

## Feature Contracts

### Overview and Tenants

Overview is one Tenant status table showing every Tenant and the summary state
of Codex, Claude, and Components, with each Coding Agent's Current Config, Named
Config count, and discovered Session count stated in its own cell. Missing
inspection is distinct from zero or healthy state, and Component totals never
imply Coding Agent readiness. Links open the relevant management scope, and
returning to the module restores the scroll position.

Session counts are discovery counts from the same read that builds the table:
they name no Session and parse no Transcript, so Overview opens no Session
route. A failed walk reports its own error rather than a zero count.

The Service, Docker, and Runtime Image strip shares one refresh with resource
inspection while preserving independent failures and previous data. The
attention list is the single explanatory warning/error summary. Normal status
stays quiet; build failures are not Service outages, and unavailable Docker is
reported once for dependent inspection.

Tenants combines Tenant lifecycle with Component status and actions. Overview
links may target a Tenant, Coding Agent, Config scope, or Component row without
creating a parallel selection model. The frontend may present update
comparisons, but does not invent installed state, desired versions, or automatic
updates. Follow the [Tenant Component contract](tenants.md#tenant-components).

### Configs

Every selected Config displays its native files. Content may contain credentials
and is shown without redaction; keep that reminder in the editor context, stated
once and stable across editor modes — Visual masks credentials without removing
them. The Host Tenant's reminder also says that edits write to the real Host
Home. Reads remain scoped to the selected Tenant, Coding Agent, and Config.

Named Config main files use Visual mode only when the API supplies a Visual
Config Option model; Raw remains available, and Current Config is Raw-only.
Required, omitted, sensitive, enum, and provider behavior comes from that model,
not frontend-maintained lists. Optional Visual fields use an Optional checkbox in
the same column as the required-field marker and Required caption; omitted
controls stay visible and disabled, except fields whose model-declared visibility
condition is unmet. Conditional omission follows [Config semantics](configs.md).

Visual fields form one ordered list without group headings or field-help icons.
Codex places the Custom provider aggregate first. Including it shows a name
field on that same row and reveals Base URL below. Other fields follow
Agent-defined order. Native file headers and independent Save actions remain.
Files stack at their content height in one scrolling region with each file's
header sticky while its file is in view; editors do not scroll inside the
pane. The Raw editor's own chrome — selection, search, panels, tooltips —
takes the Console's tokens in both themes. A file header's second line speaks only when there is something to say —
`Unsaved changes` or `New file` — and Save all appears only once two files are
dirty, since one dirty file already has its own Save. Field captions are not
control labels. Checkbox labels still toggle inclusion.

Drafts and results are tracked per file. Ordered saves do not imply rollback,
and dirty guards cover every navigation path. Use Last Application and Config
Drift language; never describe an Active Config. Credential Propagation remains
a Host Codex Current Config action.

Apply is offered twice for one Named Config: a quiet row action in the catalog
and the primary action of its detail header, both through the same confirmation.
The Last Application source is marked where it is read — on its row and in its
header — as `Applied` while drift is clean, otherwise with the drift label; a
clean application offers no Apply at all, since rerunning it changes nothing.

For the Last Application source and Current Config, file headers expose difference
counts and expandable Named/Current value comparisons. Visual fields carry
persistent difference indicators; Raw uses source-aware line markers. Missing or
hidden fields remain discoverable in the file summary, without synthetic editor
lines. Credential differences expose internal paths while retaining one file-level
Config Field count. Sensitive value comparisons retain explicit reveal controls.
The Differs badge does not use the Last Application timestamp as its tooltip.

Comparison follows the visible drafts after a 250 ms debounce, labels unsaved
content, and rereads on opening, Refresh, Save, and Apply without idle polling.
Invalid input or failed reads clear that file's old markers and explain why
comparison is unavailable; other files remain inspectable. Obsolete responses
cannot replace a newer edit or scope. Comparison only inspects and locates;
existing Save and Apply remain the write operations.

Routes distinguish the Named Config catalog, Current Config, and a named Config.
Desktop may default to Current Config detail; a one-panel layout does not mark
that default inspected until opened. Follow [Config semantics](configs.md).

### Sessions

Session detail has shareable Conversation and Details tabs. Conversation keeps
native order, safe Markdown for Agent text, plain text for user messages, and
groups Tool Activity separately from Transcript Evidence. Reasoning remains
hidden.

Projection quirks remain quiet. Warnings appear only when reading is impaired,
such as malformed records, an incomplete stream, or failed Tool Activity.
Streaming renders frames as they arrive; manual refresh preserves old content
until replacement succeeds. Missing-message, tool-only, evidence-only, and
partial states remain explicit.

Catalog summaries prefer meaningful human text over review boilerplate, raw
approval JSON, markup, or skill paths. Single-delete confirmation identifies the
selected Session; batch confirmation summarizes the selection without listing
ids. Follow the [Session contract](tenants.md#sessions).

### Requests

Requests owns page, selection, detail, and tab URL state. Summary is the default
tab; body data loads only for the visible body tab. Selection may span pages,
while active Requests remain unselectable.

Rust supplies Request Assessment and diagnostics. The browser does not
reclassify outcomes or parse bodies to invent model, usage, First Token, Session
ID, or diagnostics. HTTP status, Provider Error, transport findings, and
warnings remain independent evidence. Follow the
[Request diagnostics contract](sandbox.md#diagnostics).

Body views provide Raw download, decoded Source, and browser-only Pretty
representations. Raw preserves application-visible bytes; Pretty never changes
or persists a Request. Values remain unredacted. Lossless JSON preserves large
number spelling and rejects duplicate keys; decoding failures fall back without
hiding Raw.

SSE presentation derives only complete events and keeps partial tails visible.
It does not reconstruct a reply from fragments. Optional timings join by
sequence and degrade locally; encoded streams and incomplete timings never
invent raw offsets or durations.
