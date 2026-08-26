# Layer the Console feature-first and keep it dependency-free

The Console is organized as `app` -> `features` -> `shared` -> `api`, with one
directory per domain owning its page, query codec, React-free domain modules,
and components, and with ESLint's `no-restricted-imports` rejecting any import
that reverses a layer or lets two features depend on each other. Routing,
polling, cancellation, batch selection, and failure notices are hand-written
hooks under `shared/`, not a router or data-fetching library, because the
embedded Console ships as one bundle inside the Rust binary under a fixed gzip
budget and the Control API is a private, Console-only surface that gains nothing
from a general client.

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
