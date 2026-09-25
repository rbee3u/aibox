# Development

This document owns project environment setup, build outputs, check commands,
and contract generation. Read [AGENTS.md](../AGENTS.md) for change constraints,
[CONTEXT.md](../CONTEXT.md) for domain language, and the [ADR index](adr/README.md)
for architectural decisions. Console architecture and test organization belong
in [Console Architecture](console-architecture.md); interaction and feature
contracts belong in [Console UI](console-ui.md).

## Environment and Dependencies

Use Rust meeting `rust-version` in [Cargo.toml](../Cargo.toml), Make, and
Node/npm matching [web/package.json](../web/package.json). Cargo and npm use
the committed lockfiles.

Install the frontend dependencies once per environment and again after
dependency changes:

```sh
make web-ci
```

Native build bindings are platform-specific. Do not share one `node_modules`
between host and container platforms. When a Workspace is shared, mount a
separate directory over `/workspace/<directory name>/web/node_modules`.

## Build and Install

Use `make help` for the authoritative target list.

```sh
make build
make install
```

For local development, run `make dev`. It rebuilds the Console and starts
`aibox console` in the foreground at `http://127.0.0.1:9923/`. The Console is
embedded in the Rust binary, so restart the command after frontend changes.

`make build` generates Console assets and builds the CLI. `make install`
always runs `npm ci`, builds the Console, and installs the CLI with
`cargo install --locked --path .`. Other Make tasks reuse installed dependencies.
The installed binary embeds the Console and needs no Node runtime.

Every Make task that compiles Rust first builds the Console, including focused
Rust checks and contract generation. Aggregate targets such as `make check`
share that prerequisite, so they build the assets once. Build failures stop the
dependent Rust commands.

Direct Cargo commands do not build the Console. Generate assets first, and
regenerate after changing frontend source or switching branches:

```sh
make web-build
cargo run --locked -- console
```

Edit `web/index.html` and `web/src/`, never generated `web/dist/` files.
The dedicated `web/dist/` output directory is cleaned by the frontend build
and ignored by Git, ESLint, and Prettier. Rust embeds its HTML, CSS, and
JavaScript; the hand-maintained Dockerfile and runtime scripts remain in
`assets/`. Vite writes `index.html` and the Console bundles under `dist/assets/`
with URLs below `/_aibox/ui/`.

## Checks and Focused Iteration

```sh
make check
```

The complete check is socket-free. `make rust-check` runs Rust formatting,
tests, Clippy, and private-item documentation checks. `make web-check` runs
frontend formatting, type checking, tests, lint, and Rust-owned contract
verification. Both use the shared Console build prerequisite; the latter
requires Rust as well as Node.

`make format`, `make test`, and `make lint` cover both Rust and Console.
For individual checks, use the native tools from the repository root:

```sh
# Rust commands require the generated Console assets described above.
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --document-private-items

npm --prefix web run format:check
npm --prefix web run typecheck
npm --prefix web run test
npm --prefix web run lint
```

Use `cargo fmt` or `npm --prefix web run format` to format only one language.

## Contract Generation

Rust owns the wire types, route manifest, and samples committed under
`web/src/api/generated/`. For intentional contract or exporter changes, run:

```sh
make web-contract
```

Review and commit the resulting artifacts alongside the Rust changes. Do not
edit generated files manually. Verify them without updating committed files:

```sh
make web-contract-check
```

Verification regenerates into a temporary directory and compares all three
artifacts byte-for-byte. It is included in `make web-check` and `make check`.
Unlike these reviewable contracts, compiled Console bundles are rebuilt locally
and are not committed to Git.

## Optional Socket Checks

Routine checks do not bind sockets. Run the following checks explicitly in an
environment that permits loopback listeners.

Playwright uses bundled Chromium and starts a loopback-only Vite listener:

```sh
npm --prefix web exec playwright install chromium
npm --prefix web run test:chromium
```

The Runtime Image contains Chromium ABI libraries and fonts, but not a browser.
The optional Reqwest TCP smoke test uses real loopback connections:

```sh
make web-build
cargo test --locked request::tests::reqwest_tcp_smoke_preserves_bytes_headers_query_and_redirect_policy -- --ignored --exact
```
