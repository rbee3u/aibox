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
the committed lockfiles. Rust formatting and lint checks require rustfmt and
Clippy in the selected toolchain.

Install the frontend dependencies once per environment and again after
dependency changes:

```sh
make deps
```

Native build bindings are platform-specific. Do not share one `node_modules`
between host and container platforms. When a Workspace is shared, mount a
separate directory over `/workspace/<directory name>/web/node_modules`.

## Build and Install

Use `make help` for the authoritative target list.

```sh
make web && make build
make deps && make web && cargo install --locked --path .
```

For local development, build the Console assets and start the foreground
Service at `http://127.0.0.1:9923/`:

```sh
make web && make console
```

Pass Console arguments through `ARGS`, for example
`make console ARGS="--listen 127.0.0.1:9924"`.

`make build` uses the development profile and writes `target/debug/aibox`.
Install the CLI with `cargo install --locked --path .` after building the
Console assets. The installed binary embeds the Console and needs no Node
runtime.

Make targets run independently. Only `make deps` installs frontend dependencies;
other targets reuse them. Build Console assets with `make web` before commands
that compile Rust, including `make build`, `make test`, `make lint`,
`make generate`, `make console`, and direct Cargo commands. Rebuild after
frontend changes or switching branches, and restart the Service to use the new
embedded assets:

```sh
make web
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
make web && make style test lint
```

The complete check is socket-free. `make style` checks Rust and Console
formatting. `make test` runs Rust and Console tests. `make lint` runs Clippy,
TypeScript type checking, ESLint, private-item documentation checks with warnings
treated as errors, and Rust-owned contract verification.

Use these targets for focused iteration, or run individual native tools from
the repository root:

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

Use `make format` to format both languages, or `cargo fmt` or
`npm --prefix web run format` to format only one language.

## Contract Generation

Rust owns the wire types, route manifest, and samples committed under
`web/src/api/generated/`. For intentional contract or exporter changes, run:

```sh
make web && make generate
```

Review and commit the resulting artifacts alongside the Rust changes. Do not
edit generated files manually. Rebuild Console assets after generation, then
verify the contracts without updating committed files:

```sh
make web && make lint
```

Verification regenerates into a temporary directory and compares all three
artifacts byte-for-byte. Any export or comparison failure fails `make lint`,
and the temporary directory is removed on exit. Unlike these reviewable
contracts, compiled Console bundles are rebuilt locally and are not committed
to Git.

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
make web
cargo test --locked request::tests::reqwest_tcp_smoke_preserves_bytes_headers_query_and_redirect_policy -- --ignored --exact
```
