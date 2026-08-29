# Layer the Console feature-first and keep it dependency-free

The Console uses an acyclic feature-first graph: `domain` depends on no other
layer; `api` and `shared` may depend on `domain` but not each other; features
depend on their own code plus `domain`, `api`, and `shared`; and `app` composes
all layers. One directory per domain owns a controller, thin page view, query
codec, React-free model, resource hooks, and components. ESLint rejects reversed
edges and cross-feature imports. Routing, polling, latest-request cancellation,
batch selection, and failure notices remain small local hooks rather than a
router, global store, or data-fetching framework because the embedded Console
ships as one bundle under a fixed gzip budget and the Control API has one
private client.

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

## Consequences

Wire types live in `api/<domain>.ts` rather than beside the feature that reads
them, because the composition root must import every domain and features must
not import each other. Cross-domain vocabulary such as Component rows inside the
Overview topology therefore has exactly one home.

Shared visual structure is a stylesheet (`shared/ui/layout/catalog.module.css`)
rather than a component library, because the catalogs share a visual rhythm
while their markup legitimately differs — an `aside` landmark here, extra grid
rows there. A domain module extends a shared class through `composes` only when
it adds rules of its own.

Complex pages expose controller hooks for URL synchronization, external
resource ownership, dialogs, and mutations. Focused hooks continue to own
independent Config drafts and Session/Request streaming inspection so the page
controller does not become a single reducer or cache.
