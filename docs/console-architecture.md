# Console Architecture

Architecture and tests for the React/TypeScript Console under `web/`. See
[Console UI](console-ui.md) for interactions and [Development](development.md)
for builds, checks, and contract generation.

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

Each feature owns its controller, views, workflow, and resource hooks.
Feature-root `viewTypes` supply shared contracts; views do not import controllers.
Controllers compose concern hooks and coordinate their policies. Cross-feature
URL codecs live in `features/common/routes/`; feature-only routes, navigation
synchronization, and workflow actions stay local.

Configs' `editor/` concern owns a `ConfigEditorSession` per Tenant, Agent, and
Current/Named Config identity. React manages its lifetime and subscribes to
immutable snapshots. Drafts, mode, saves, dirty state, and comparison inputs
share that session; file focus and URLs do not replace it. Comparison revisions
invalidate old evidence even when edits return to earlier content. CodeMirror
and difference location remain view bindings. See
[ADR 0012](adr/0012-config-editing-scope.md).

`LatestRequest` owns replaceable request leases. Its single-response runner
commits only the current request's callbacks and releases its lease; resource
hooks keep their own loading, refresh, error, and recovery policies. Streaming
readers use leases directly rather than being forced into that runner.

Styles stay beside their concern; cross-concern styles stay at the feature root.
Page styles contain page layout only.
Shared catalog card rows use explicit `data-inspected`, `data-selected`, and
`data-selection-mode` state; deletion actions use `data-row-action`. Never infer
state from CSS Modules class-name fragments.

ESLint discovers feature and concern directories from disk, so a new feature
receives the same dependency restrictions immediately.

Only `app/` integrates browser history and composes the persistent shell; pages
receive a location snapshot and navigation writer. Config dirty state must be
guarded across in-app, history, and browser navigation.

## Control API and Generated Contracts

Console pages and assets live below `/_aibox/ui/`; Console-internal APIs live
below `/_aibox/api/`. The Control API is not a public integration surface.

The shared transport owns fetch, CSRF, NDJSON, and binary bodies. Domain
adapters own paths, queries, wire conversion, and feature-facing ports. Features
never import transport or generated wire types.

Rust owns the wire types, route manifest, and contract samples under
`web/src/api/generated/`. Declare each route once in
`service/control/routes.rs`: one declaration of path, method, and handler
produces both the Axum registration and the generated route manifest.
Production clients remain handwritten. See
[Contract Generation](development.md#contract-generation) for the update and
verification workflow.

The exporter registers endpoint root DTOs and recursively discovers their
TypeScript dependencies, rejecting conflicting names and ordering declarations
deterministically. Nested types need no facade re-export merely for export;
fixtures may still name types they construct. Type overrides must retain their
Rust dependency information so recursive collection remains complete.

## Testing

Keep a rule in the narrowest useful layer:

1. Pure tests cover codecs, reducers, derivations, formatting, and state.
2. Feature tests render the real page against a strict domain API fake.
3. Adapter tests cover HTTP and wire behavior.
4. Optional Chromium tests cover real layout or browser behavior.

Tests follow their modules; page interactions stay at the feature root. Keep
suite-only doubles local and feature-wide support inside that feature. Share
cross-feature fixtures only when the production concept is also shared.

Vitest runs pure `.test.ts` files in a shared Node environment so codecs,
reducers, formatting, and source-contract checks do not pay for jsdom. Tests
that use browser globals are listed with the isolated DOM project in
`web/vite.config.ts`; `.test.tsx` files use that DOM project by default.
The shared reset restores mocks, stubbed globals, and real timers after every
test. Keep Node-project tests free of mutable module-global state so file order
cannot affect their results.

Do not repeat pure rules in browser tests. Geometry tests assert behavior and
relative layout, not design-token values or pixel snapshots. Routine Rust and
Console tests remain socket-free.

Run the real-browser checks explicitly as described in
[Optional Socket Checks](development.md#optional-socket-checks).
