# Console UI

Console interaction and feature contracts. Domain behavior belongs in
[Configs](configs.md), [Tenants](tenants.md), [Sandbox](sandbox.md), and
[Requests](requests.md); implementation boundaries belong in
[Console Architecture](console-architecture.md).

## Shared Interaction Contracts

### Navigation and Layout

Use semantic tokens from `shared/styles/tokens.css` in both themes. Keep
feature interactions local and general controls in shared primitives.

- Cap and left-align detail content using the measure tokens. Backgrounds and
  dividers span the container; row actions align to the content measure and
  metadata stays beside its value. Components detail, code editors, JSON trees,
  and raw bodies use the available width.
- Use the type scale without raw or inline sizes. Section labels are smaller
  and dimmer than row titles; bold is reserved for page titles. Comparable
  values share a size. Agent-output headings have a separate content scale.
- Keep the fill order `canvas < shell < inset < surface`. Light-theme elevation
  uses contact and ambient shadows; dark-theme raised surfaces also change fill.
  Hover, selection, and press use progressively stronger ink overlays on both
  shell and content surfaces. Persistent states also need a non-color cue.
- Use the two-pixel spacing scale and shared section gaps. Reserve accent for
  current navigation, primary actions, hovered links, and focus. Dense linked
  cells use arrows or underlines at rest. Text prominence decreases from ink
  through secondary, muted, and faint.
- Use the line-icon family, shared stroke weight, and `shared/icons/iconSizes.ts`
  scale. Brand marks are the exception; selection marks may override stroke
  weight. Shared icon slots keep a consistent size.
- Controls with hover feedback also show press feedback; fields use focus
  instead. Buttons layer press feedback over their fill. Use the shared focus
  ring and offset for keyboard focus, drawing a replacement if the focusable
  element cannot carry it. The skip link always shows its ring when focused.
- Use the two transition tokens and global reduced-motion rule. Fill changes
  ease; press arrives instantly and eases on release. Chrome defines its target
  sizes explicitly; control height comes from either padding or a height token.
- Icon controls share one string for accessible name and tooltip. Tooltips open
  after hover delay or immediately on keyboard focus, but stay closed during
  pointer-driven focus restoration. The shared anchored tooltip uses dark labels
  for names and light bordered panels for explanations.

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

Read failures with an inline error surface offer local retry without a duplicate
notification. Other failures use the shared notification stack, keyed by resource
or action; repeated identical failures stay quiet until recovery. Destructive
failures require confirmation again.

The latest Management Operation remains available across modules. Polling does
not overlap itself or reopen a collapsed operation panel. Route changes and
dialogs cancel obsolete work; late responses cannot replace a newer generation.
Degraded decoding or evidence stays local to the affected view.

### Accessibility and Content

Use system sans-serif for interface prose and system monospace for identifiers,
paths, URLs, methods, timestamps, Configs, code, bodies, Transcripts, and logs.

Render Agent Conversation Messages as safe GFM Markdown with raw HTML disabled.
Only absolute HTTP(S) links are active; relative links remain inert so they
cannot enter the same-listener Request Proxy. Images render as references without
fetching. User messages and raw evidence preserve plain text and line breaks.

## Feature Contracts

### Overview and Tenants

Overview lists every Tenant with Codex, Claude, and Component status. Each
Agent cell shows Current Config, Named Config count, and discovered Session
count. Missing inspection differs from zero or healthy state; Component totals
do not imply Agent readiness. Links open the relevant management scope, and
returning restores scroll position.

Session counts come from the same Overview read without parsing Transcripts.
They link to no individual Session; failed discovery reports an error, not zero.

The Service, Docker, and Runtime Image strip shares one refresh with resource
inspection while preserving independent failures and previous data. The
attention list is the single explanatory warning/error summary. Normal status
stays quiet; build failures are not Service outages, and unavailable Docker is
reported once for dependent inspection.

Tenants combines Tenant lifecycle with Component status and actions. Overview
links may target a Tenant, Agent, Config scope, or Component row without
creating a parallel selection model. The frontend may present update
comparisons, but does not invent installed state, desired versions, or automatic
updates. Follow the [Tenant Component contract](tenants.md#tenant-components).

### Configs

Show one persistent editor reminder that native files may contain credentials;
Visual mode masks them without removing them. For the Host Tenant, also state
that edits write to the real Host Home. Reads stay within the selected Config.

Named Config main files use Visual mode only when the API supplies a Visual
Config Option model; Raw remains available, and Current Config is Raw-only.
Required, omitted, sensitive, enum, and provider behavior comes from that model,
not frontend-maintained lists. Optional Visual fields use an Optional checkbox in
the same column as the required-field marker and Required caption; omitted
controls stay visible and disabled, except fields whose model-declared visibility
condition is unmet. Conditional omission follows [Config semantics](configs.md).

Visual fields use one Agent-ordered list with native-path captions and no group
headings or help icons. Codex places Custom provider first; including it reveals
its name on the same row and Base URL below. Routed fields show the saved
Request Proxy prefix below their controls.

Files share one scrolling region with sticky headers and independent Save
actions; editors do not scroll internally. Raw editor chrome uses Console tokens
in both themes. Headers show `Unsaved changes` or `New file` when applicable;
Save all appears when at least two files are dirty.

A refused Visual save shows a strip below the file header and marks the invalid
field. Checks match the Service: required values and HTTP(S) URLs. Server refusals
use the strip without a field mark. Editing clears both; read failures use the
page-level error.

Leaving a dirty Config through in-app or history navigation uses one
`Unsaved changes` dialog with Cancel, Discard, and Save; unload uses the native
prompt. Switching Visual/Raw with a draft uses `Switch to Raw?`/`Switch to Visual?`
and warns that drafts belong to the mode being left.

Credential Propagation is a Host Codex Current Config action: a quiet `Propagate`
row action and a `Propagate credentials` header action. Its preview and result
dialogs state the source once, summarize in a toned banner, and list targets
as target · status · reason with sentence-case statuses; a target newer than
the source is Skipped, a failed write leads the result.

Apply is offered twice for one Named Config: a quiet row action in the catalog
and the primary action of its detail header, both through the same confirmation.
The Last Application source shows `Applied` on its row and header while drift
is clean, otherwise the drift label. Clean applications hide Apply.

For the Last Application source and Current Config, file headers expose difference
counts and expandable Named/Current value comparisons. Visual fields carry
persistent difference indicators; Raw uses source-aware line markers. Missing or
hidden fields remain discoverable in the file summary, without synthetic editor
lines. Credential differences expose internal paths while retaining one file-level
Config Field count. Sensitive value comparisons retain explicit reveal controls.
The Differs badge does not use the Last Application timestamp as its tooltip.

Comparison follows visible drafts after a 250 ms debounce, labels unsaved
content, and rereads on opening, Refresh, Save, and Apply without idle polling.
Invalid input or failed reads clear only that file's markers and explain the
failure. Stale responses cannot replace newer edits or scopes. Comparison is
read-only.

Routes distinguish the Named Config catalog, Current Config, and a named Config.
Desktop may default to Current Config detail; a one-panel layout does not mark
that default inspected until opened. Follow [Config semantics](configs.md).

### Sessions

Session detail has shareable Conversation and Details tabs. Conversation keeps
native order and groups Tool Activity separately from Transcript Evidence.
Reasoning remains hidden.

Projection quirks remain quiet. Warnings appear only when reading is impaired,
such as malformed records or an incomplete stream; a failed Tool Activity marks
its own activity group. Unanswered calls show neutral `No result` without a
result section. Streaming renders frames as they arrive; manual refresh keeps
old content until replacement succeeds and preserves the reader's position,
open disclosures, and the navigator's current stop. Stale evidence triggers one
in-place Session reread before reporting failure.
Missing-message, tool-only, evidence-only, and partial states remain explicit.

Catalog summaries prefer meaningful human text over review boilerplate, raw
approval JSON, markup, or skill paths. Each row states its start time and its
message and tool counts on one left-aligned line under the title, led by the
source when several are listed. Single-delete confirmation identifies the
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
[Request diagnostics contract](requests.md#diagnostics).

The catalog status cell adds an Assessment glyph only when the primary finding
adds to the HTTP status. Errors override the HTTP tone; warnings retain it.
Without an HTTP status, the finding supplies the label. Hover explains the
finding; the second line holds model, timing, and timestamp.

Requests detail shows retry count and first/last HTTP 429 times when retry
rules apply. Its Request upload stage includes local recording before the first
upstream send; its Response wait stage includes retry delays, and recovered
requests carry a Warning assessment.

Body views provide Raw download, decoded Source, and browser-only Pretty
representations. Raw preserves application-visible bytes; Pretty never changes
or persists a Request. Values remain unredacted. Lossless JSON preserves large
number spelling and rejects duplicate keys; decoding failures fall back without
hiding Raw.

SSE presentation derives only complete events and keeps partial tails visible.
It does not reconstruct a reply from fragments. Optional timings join by
sequence and degrade locally; encoded streams and incomplete timings never
invent raw offsets or durations.
