# AGENTS.md

Use [CONTEXT.md](CONTEXT.md) for domain language. Before changing a domain,
read its canonical reference and relevant [ADRs](docs/adr/README.md):

| Area | Canonical reference |
| --- | --- |
| Tenants, Sessions, Components, Tenant Environment | `docs/tenants.md` |
| Named and Current Configs | `docs/configs.md` |
| Mounts, Runtime Image, cleanup | `docs/sandbox.md` |
| Requests and Request Proxy | `docs/requests.md` |
| Development environment, builds, checks, contract generation | `docs/development.md` |
| Backend ownership, Management use cases, and Service composition | `docs/backend-architecture.md` |
| Console architecture, Control API ownership, and test organization | `docs/console-architecture.md` |
| Console interaction and feature contracts | `docs/console-ui.md` |

Keep code, Clap help, examples, and canonical references aligned. Update the
owning document; link to contracts instead of repeating them. Add concepts,
files, or abstractions only when necessary. ADRs explain decisions and trade-offs;
comments explain intent, constraints, or surprising behavior. Delete narration,
common knowledge, and obsolete history; preserve useful existing guidance.

## Guardrails

### Agents and CLI

- Reach shared Agent paths, Config files and templates, empty Current Config,
  and invocation behavior only through `AgentKind`. Shared contract matches
  stay in `agent/mod.rs`; Agent-specific fields stay in `agent/<agent>.rs`.
- Split argv at the first `--` before Clap parses it. Forward the right side
  verbatim only to `run`.
- Keep the public CLI to `console` (`--listen`), `run`, and `debug` (`--tenant`).
  Expose only the application entry point; the Control API is Console-internal.

### Filesystem and Destructive Operations

- Host-side operations treat container-writable paths as untrusted: validate
  relevant ancestors and reject symlinks or unexpected entries. Listing may
  ignore unknown entries; unsafe selections fail.
- A Managed Tenant exists only when `tenants/<name>` is a real directory. Only
  its subtree may be mounted from inside `$AIBOX_ROOT`.
- `$AIBOX_ROOT` is dedicated but unmarked; scope deletion structurally.
- Tenant, Named Config, and Session deletion requires explicit names or ids, or
  an explicit select-all request. Empty never means all. Preserve the Default
  Tenant and documented irreversible-operation guards.
- Missing read-only scopes stay quiet and create nothing. Initialize Tenant or
  Agent state only through documented lifecycle operations.
- Preserve documented modes and atomic replacement. Multi-file operations are
  sequential and never roll back earlier successes.

### Components, Docker, and the Runtime Image

- A Run requires the selected Tenant-local Agent Component and invokes its
  absolute launcher. Never install on first Run or fall back to the Runtime
  Image. A Debug Shell requires no Component.
- Runs, Debug Shells, and Component installers go through `docker::run`. One
  process supports only one active container operation.
- Register the cidfile before spawning Docker and the child immediately after;
  keep cleanup armed through `finish_child`.
- Build the embedded Dockerfile with an empty context and fetch dependencies
  during the build. Keep mutable Agents, runtimes, toolchains, and browsers out
  of the fixed Runtime Image.
- Tenant lifecycle may recover interrupted filesystem work; separate processes
  are not serialized.

### Service and Architecture

- Give Management coordinators only required capabilities. Route mutations
  through them; Request reads may use its facade. Keep transport out of domains
  and Management.
- SSE observation has no Store or file I/O dependency. Store owns persistence;
  Proxy records raw bytes before observation.
- Register Control routes once in `service/control/routes.rs`. Handlers remain
  `pub(super)`, return `ControlResult`, and use the shared error envelope.
- Keep module graphs acyclic and prohibit sibling reach-through. Declare each
  non-structural Rust depth-one/depth-two edge exactly once in
  `allowed_dependencies`; remove stale entries. Console dependencies point
  inward; `app` owns composition and browser history.

### Tests and Generated Assets

- Put Rust suites in sibling `<module>_tests.rs` files under `#[cfg(test)]`,
  except for documented architecture and contract seams.
- Test through production facades without widening visibility. Keep suite-local
  doubles local and shared doubles in established test support modules.
- List test-only surfaces exactly in `TEST_ONLY_SURFACE`. Gate test re-exports
  with `cfg(test)`; never hide dead exports with blanket unused-import allows.
- Routine tests must not bind sockets. Use in-memory Tower services and
  deterministic streams; keep real-socket Reqwest and Chromium checks explicit
  and optional.
- Edit Console source, never generated `web/dist/` or wire artifacts. Follow
  [Development](docs/development.md) for setup and contract generation.

## Checks

Run the complete socket-free check before handoff:

```sh
make web && make style test lint
```

Build Console assets before compiling Rust; `make help` lists targets.
