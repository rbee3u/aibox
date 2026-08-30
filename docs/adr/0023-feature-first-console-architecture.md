# Layer the Console feature-first and keep it dependency-free

The Console uses an acyclic feature-first graph: `domain` depends on no other
layer; `api` and `shared` may depend on `domain` but not each other;
`features/common` may depend on all three but on no feature; features depend on
those four layers; and `app` composes everything. ESLint rejects reversed edges
and cross-feature imports. Routing, polling, latest-request cancellation,
batch selection, and failure notices remain local rather than moving into a
router, global store, or data-fetching framework because the embedded Console
ships as one bundle under a fixed gzip budget and the Control API has one
private client. A feature-local reducer may own only workflow state that spans
multiple actions, such as selection, dialogs, mutation phases, and typed
outcomes. Resource snapshots, loading, streaming reads, and AbortController
ownership remain in focused hooks.

A feature directory holds its controller, grouped view model, thin page view,
query codec, workflow reducer, and the modules more than one of its concerns
reads; `catalog/`, `detail/`, and `mutation/` own what a single concern uses.
The split is derived from usage rather than imposed: a module two concerns read
stays at the feature root. Overview keeps `topology/` and `components/` because
it has neither a catalog nor a detail pane, and naming its directories after
concerns it lacks would describe it falsely.

`features/common` exists for what several features share but `shared` cannot
hold, because it needs both an `api` wire type and a `shared/ui` type — a
Tenant option list is the motivating case. It is deliberately outside the
features-may-not-import-each-other rule so every feature may use it, and its own
boundary forbids importing a feature back so the shared layer cannot become a
dependency cycle. The batch-selection state machine lives here as one reducer:
four features had implemented it separately and had already diverged on action
names, field names, and whether a recovered selection resumes.

Feature controllers return grouped view models instead of flat bags of state,
setters, refs, and commands. All four catalog features group catalog, detail,
selection, mutation, dialog, and feedback ownership while leaving independent
editor or streaming lifecycles in focused hooks. A hook whose result spans more
than one group returns those groups itself, so a controller spreads them rather
than forwarding each field and a new field needs one edit instead of three.
Large pure models remain behind stable feature facades: Overview separates tree,
layout/path, query/filter, and health/attention logic, and one Config file
separates pure projection, session ownership, and rendering.

Configs, Tenants, and Sessions derive their Tenant and Agent selection directly
from the current `search` value on every render. Events write URL state only
through the `onLocationChange` port; local state is reserved for drafts, loaded
resources, mutations, dialogs, focus, and scroll position. Requests
intentionally keeps a route intent separate from the successfully loaded
snapshot because its detail and body polling can outlive a URL change.

## Considered Options

A layer-first split (`pages/`, `components/`, `hooks/`) was rejected because
this Console's domains barely interact, so grouping by technical role would
scatter each domain across four directories while hiding that Configs and
Sessions never touch each other.

Adopting React Router and TanStack Query was rejected on cost rather than
quality: together they would consume most of the remaining bundle allowance to
replace roughly two hundred lines of hooks, and neither addresses the Console's
actual complexity, which lives in Transcript projection, Component state
derivation, and raw Body decoding.

Injecting the Control API through React Context was rejected because passing
each page only its own domain interface as a prop is what lets tests substitute
strict, deterministic fakes with no sockets, no HTTP knowledge, and no provider
wrapper.

Asserting HTTP paths or wire field names through rendered pages was rejected
because it couples controller tests to the adapter below their feature port.
Page tests fake the feature-facing API and are split by lifecycle, routing,
inspection/editor, mutation/deletion, and failure behavior; adapter tests alone
own paths, queries, snake_case bodies, and wire conversion.

## Consequences

Wire types live in `api/<domain>.ts` rather than beside the feature that reads
them, because the composition root must import every domain and features must
not import each other. Cross-domain vocabulary such as Component rows inside the
Overview topology therefore has exactly one home. `api/core.ts` names the few
wire types more than one domain module shares, which is also where a Rust-side
rename lands once: ESLint keeps features and `features/common` out of
`api/generated/`, so nothing outside `api/` depends on a generated name.

Shared visual structure is a stylesheet (`shared/ui/layout/catalog.module.css`)
rather than a component library, because the catalogs share a visual rhythm
while their markup legitimately differs — an `aside` landmark here, extra grid
rows there. A domain module extends a shared class through `composes` only when
it adds rules of its own.

Complex pages expose controller hooks for URL synchronization, external
resource ownership, dialogs, and mutations. Feature-local workflow reducers do
not become resource caches: focused hooks continue to own Config drafts,
catalog snapshots, loading, Session/Request streaming inspection, and request
cancellation. Grouped controller outputs make that ownership visible to the
page without introducing a global store or another framework.
