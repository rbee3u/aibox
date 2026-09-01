# AGENTS.md

AIBox is a Rust CLI and foreground local Service that runs Claude Code or
OpenAI Codex, or opens a Managed Tenant Debug Shell, inside a Docker container.
The container is the Filesystem Sandbox boundary.

Use the canonical domain language in [CONTEXT.md](CONTEXT.md). Architectural
decisions live in [docs/adr/](docs/adr/README.md). Before changing a domain,
read its canonical reference and the relevant ADRs:

| Area | Canonical reference |
| --- | --- |
| Tenants, Sessions, Components, Tenant Environment | `docs/tenants.md` |
| Named and Current Configs | `docs/configs.md` |
| Mounts, Runtime Image, cleanup, Request Proxy | `docs/sandbox.md` |
| Console development and UI contracts | `docs/console-ui.md` |

Keep examples, Clap help, code, and these references aligned. Do not copy a
complete behavioral contract into another document.

## Implementation Map

- `src/cli.rs` owns the three-command Clap surface and Run pass-through
  boundary. `src/lib.rs` converts parsed DTOs into execution-owned commands and
  dispatches them.
- `src/foundation/` owns policy-free no-follow filesystem, platform, and
  synchronization mechanics. `src/docker/` owns cleanup-aware container
  execution and Runtime Image construction.
- `src/sandbox/` validates `RunSpec` and mounts, then builds Docker arguments.
  `src/execution/` orchestrates Run and Debug without depending on Clap.
- `src/agent/` centralizes Coding Agent contracts. `src/session/` owns Session
  discovery; Agent-specific Transcript parsing stays in `session/claude.rs`
  and `session/codex.rs`.
- `src/tenant/`, `src/config/`, and `src/component/` own their domain
  lifecycles and facades. `src/metadata.rs` owns shared Tenant-and-Agent
  observational metadata.
- `src/request/` owns Request observation, persistence, reporting, and proxy
  forwarding. Storage and proxy lifecycles stay private behind its facades.
- `src/service/` is the Root-local composition root. Coordinators own
  Management Operations; `control/` owns routes, wire DTOs, response helpers,
  Console assets, contract export, and adapters.
- `console/src/` is feature-first and acyclic: `domain` is independent; `api`
  and `shared` depend only on it; `features/common` may use both; features use
  those inner layers; `app` composes the application.

## Guardrails

### Coding Agents and CLI

- Reach shared Agent paths, Config files and templates, empty Current Config,
  and invocation behavior only through `AgentKind`. Shared contract matches
  stay in `agent/mod.rs`; Agent-specific fields stay in `agent/<agent>.rs`.
- Split argv at the first `--` before Clap parses it. Forward the right side
  verbatim only to `run`.
- The public commands are `console`, `run`, and `debug`. `console` owns only
  `--listen`; `debug` owns only `--tenant`. Do not restore removed commands,
  aliases, tombstones, or a completion protocol.
- The crate is application-only. Expose only its entry point. The Control API
  is Console-internal and has no public compatibility contract.

### Filesystem and Destructive Operations

- Treat container-writable paths as untrusted. Host-side operations validate
  relevant ancestors and reject symlinks or unexpected entries; listing may
  ignore unknown collection entries, but selecting an unsafe path must fail.
- A Managed Tenant exists only when `tenants/<name>` is a real directory. Only
  its subtree may be mounted from inside `$AIBOX_ROOT`.
- Treat `$AIBOX_ROOT` as dedicated but unmarked. Keep deletion structurally
  scoped and never imply that a general-purpose directory is safe.
- Tenant, Named Config, and Session deletion requires explicit names or ids, or
  an explicit select-all request. An empty selection never means all. Preserve
  the protected Default Tenant and every documented irreversible-operation
  guard.
- Missing read-only scopes stay quiet and create nothing. Only documented
  lifecycle operations may initialize missing Tenant or Agent state.
- Preserve required modes and atomic-file replacement behavior from the
  canonical domain documents. Multi-file operations remain sequential and do
  not roll back earlier successes.

### Components, Docker, and the Runtime Image

- A Run requires the selected Tenant-local Agent Component and invokes its
  absolute launcher. Never install on first Run or fall back to the Runtime
  Image. A Debug Shell requires no Component.
- Runs, Debug Shells, and Component installers go through `docker::run`. One
  process supports only one active container operation.
- Register the cidfile before spawning Docker and the child immediately after.
  Keep cleanup armed until `finish_child` proves no container outlived the
  Docker client.
- Build the embedded Dockerfile with an empty context and fetch every dependency
  during the build. Keep mutable Agents, runtimes, toolchains, and browsers out
  of the fixed Runtime Image.
- Do not claim cross-process coordination. Tenant lifecycle may recover its
  own interrupted filesystem work, but separate processes are not serialized.

### Service and Architecture

- Management mutations reach domains through coordinators. Request reads may
  use the Request facade directly; keep HTTP and Console presentation out of
  the Request aggregate.
- Register Control routes once in `service/control/routes.rs`. Handlers remain
  `pub(super)`, return `ControlResult`, and use the shared error envelope.
- Keep the Rust module graph acyclic. Declare every non-structural depth-one to
  depth-two dependency exactly once in `allowed_dependencies`; stale and
  undeclared entries must fail.
- Do not create sibling reach-through edges. Move shared ownership to an
  appropriate parent or inner module.

### Tests and Generated Assets

- Keep Rust suites in sibling `<module>_tests.rs` files reached through the
  module's `#[cfg(test)]` declaration, except for documented architecture and
  contract seams.
- Tests must not widen production visibility. Test through production facades;
  keep suite-local doubles local and shared doubles in the established test
  support modules.
- List every remaining test-only surface exactly in `TEST_ONLY_SURFACE`. Keep
  re-exports under `cfg(test)` and do not mask dead exports with blanket
  unused-import allows.
- Routine tests must not bind sockets. Use in-memory Tower services and
  deterministic streams; keep real-socket Reqwest and Chromium checks explicit
  and optional.
- Follow the feature-local Console test-support conventions documented in
  `docs/console-ui.md`. Do not share one `node_modules` between host and
  container platforms.
- Edit Console source, never generated `assets/console.*`. Update Rust-owned
  wire artifacts only through the documented contract command.

## Checks

Run the complete socket-free check before handoff:

```sh
make check
```

Use `make rust-check`, `make rust-doc-check`, or `make console-check` during
focused iteration. Use `make help` for the authoritative target list.
