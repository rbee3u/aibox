# Console Architecture

The Console is the React and TypeScript application under `web/`.
This document owns frontend architecture, dependency boundaries, Control API
ownership, and test organization. See [Console UI](console-ui.md) for shared
interaction and feature contracts, and [Development](development.md) for
environment setup, build outputs, checks, and contract generation.

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

## Control API and Generated Contracts

Console pages and assets live below `/_aibox/ui/`; Console-internal APIs live
below `/_aibox/api/`. The Control API is not a public integration surface.

The shared transport owns fetch, CSRF, NDJSON, and binary bodies. Domain
adapters own paths, queries, wire conversion, and feature-facing ports. Features
never import transport or generated wire types.

Rust owns the wire types, route manifest, and contract samples under
`web/src/api/generated/`. Declare each route once in
`service/control/routes.rs`; production clients remain handwritten. See
[Contract Generation](development.md#contract-generation) for the update and
verification workflow.

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
