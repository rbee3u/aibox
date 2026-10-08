# Development

Contribution constraints live in [AGENTS.md](../AGENTS.md).

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

Use `make help` for the target list. Targets run independently; only `make deps`
installs dependencies. Build Console assets with `make web` before compiling
Rust. Rebuild after frontend changes or switching branches, then restart the
Service to load the embedded assets.

```sh
make web && make build
make deps && make web && cargo install --locked --path .
```

Start the foreground Service at `http://127.0.0.1:9923/`:

```sh
make web && make console
```

Pass Console arguments through `ARGS`, for example
`make console ARGS="--listen 127.0.0.1:9924"`.

`make build` writes `target/debug/aibox`. The installed binary embeds the
Console and needs no Node runtime.

Edit `web/index.html` and `web/src/`; the build replaces `web/dist/`.
The repository's `assets/` holds the hand-maintained Dockerfile and runtime scripts.

## Checks and Focused Iteration

```sh
make web && make style test lint
```

The complete check is socket-free. `make style` checks Rust and Console
formatting. `make test` runs Rust and Console tests. `make lint` runs Clippy,
TypeScript type checking, ESLint, private-item documentation checks with warnings
treated as errors, and Rust-owned contract verification.

For focused checks, run individual tools from the repository root:

```sh
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

Verification regenerates all three artifacts in a temporary directory and
compares them byte-for-byte; any failure fails `make lint`.

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
