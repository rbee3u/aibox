# AGENTS.md

`aibox` is a Rust CLI and foreground local Service that runs Claude Code or
OpenAI Codex inside a Docker container. The container is the Filesystem Sandbox
boundary. `CONTEXT.md`
defines the canonical domain language; keep code, clap help, and user
documentation aligned with it. Architectural decisions live in `docs/adr/`.

`README.md` is the concise entry point for evaluation, installation, first use,
and the core safety model. Advanced behavior has one canonical home in
`docs/tenants.md`, `docs/configs.md`, `docs/sandbox.md`, or
`docs/console-ui.md`; keep examples and clap help aligned without copying full
references between them.

## Implementation Map

- `src/cli.rs` defines the three-command Clap surface and Run pass-through
  boundary; `src/lib.rs` resolves command scope and orchestrates commands.
- `src/agent.rs` centralizes Coding Agent contracts. `src/tenant.rs` owns
  Tenant resolution, lifecycle, layout, permissions, and shared path safety.
- `src/config.rs`, `src/config_model.rs`, and `src/config_auth.rs` own Config
  catalog operations, Config Application, and Credential Propagation.
  `src/metadata.rs` owns the shared Tenant-and-Agent metadata document.
- `src/runspec.rs`, `src/docker.rs`, and `src/docker_image.rs` own mount
  validation, cleanup-aware container execution, and image construction.
  `src/component.rs` owns status-line and toolchain Components.
- `src/session.rs` owns shared Session discovery and dispatch;
  `src/session_claude.rs` and `src/session_codex.rs` parse native Transcripts.
- `src/request.rs` owns shared Request state. `src/request_proxy.rs`,
  `src/request_store.rs`, `src/request_sse.rs`,
  `src/request_interpretation.rs`, and `src/request_assessment.rs` own
  forwarding, persistence, SSE indexing, protocol facts, and assessment.
  `src/request_web.rs` owns the Request API and embedded Console assets.
- `src/service.rs`, `src/control_web.rs`, and `src/operation.rs` own the
  Root-local Service, Console Control API, and ephemeral Management Operations.
- `console/src/App.tsx` owns Console routing and navigation guards;
  `TenantPage.tsx`, `ConfigPage.tsx`, `SessionPage.tsx`, and
  `RequestsPage.tsx` own their domain pages. `controlApi.ts` and `api.ts` own
  the distinct Control and Request clients. `OverviewPage.tsx` orchestrates
  Overview, while `TopologyCanvas.tsx` and `overviewTopology.ts` own its
  rendering and pure topology model. `assets/console.*` is generated output;
  use `docs/console-ui.md`.

## Constraints

**Centralize Coding Agent contracts.** Put agent-specific paths, Config
files/templates, empty Current Config content, and invocation behavior in
`AgentKind` in `agent.rs`. Keep transcript parsing in `session_claude.rs` and
`session_codex.rs`. The Docker image, container Home, and orchestration remain
shared.

**Keep the CLI surface narrow.** Split argv at the first `--` before clap parses
it, and pass the right side verbatim only to `run`. The public commands are
`serve`, `run`, and `build`; `serve` owns only `--listen`, `build` owns only
`--force`, and all Tenant, Component, Config, and Session management is exposed
through the Console Control API. Removed management names must remain unknown
Clap subcommands; do not add aliases, tombstones, or a completion protocol.

**Keep Tenants distinct.** A Managed Tenant is aibox-managed and runnable;
`host` is a valid Managed Tenant name. The Host Tenant is selected only by
Console Tenant-scoped views. The Host Tenant cannot Run and never appears in the
Managed Tenant list or deletion. The Default Managed Tenant named `default` is
protected from deletion. Service startup creates or repairs its Tenant Home
baseline after acquiring the Service Lock and fails before listening when that
cannot be done safely. Only `tenants/<name>` subtrees may be mounted from inside
`$AIBOX_ROOT`.

**Keep the direct layout.** A Managed Tenant exists exactly when
`tenants/<name>` is a real directory. Named Config catalogs live under
`<agent>/<name>/`; the Host Tenant catalog uses `<agent>/__host/`. A Claude
Named Config contains only native `settings.json`; a Codex Named Config
contains only native `config.toml` and `auth.json`. Do not add scope/Config
metadata inside Named Config directories. One aibox-owned `metadata.json` at
the Tenant-and-Agent catalog root may contain typed observational sections;
preserve unknown top-level sections when updating a known section. Do not add
metadata elsewhere, layout versions, migration readers, management wrappers,
or lock directories. Ignore unknown collection entries during listing, but
reject explicitly selected unsafe paths.

**Keep names and local permissions narrow.** Managed Tenant and Named Config
names are lowercase DNS labels of 1–63 characters. Newly created aibox root,
collection, Named Config catalog, Named Config, and Tenant Home boundary
directories are `0700`. Named Config files and newly created Current Config
files are `0600`; `metadata.json` is `0600` and limited to 16 KiB. Applying or
directly editing Current Config preserves existing file modes, including for
the Host Tenant. Existing Host Home directory modes are never changed.

**Keep Config Application explicit and one-shot.** A Run consumes Current
Config and never reads or reapplies Named Config data. Each Named Config
belongs to one Tenant and Coding Agent and defines only the fixed
Config Fields centralized in `AgentKind`. The Console Configs module applies
present fields,
deletes missing fields and preserves unrelated native configuration. After all
files succeed, record Last Application and derive Config Drift for the Console.
Store it as the strict `last_application` section of the catalog-root
`metadata.json`; this observational record never activates or reapplies a
Named Config. Do not add reconciliation, rollback, or transaction state.

**Keep Credential Propagation explicit and one-shot.** The Console Configs
module copies one Host Codex Current Config `auth.json` snapshot only to older existing
same-account ChatGPT Credentials in complete safe Named Configs and Managed
Tenant Current Configs. It creates nothing, retains no association, never runs
automatically, and is distinct from Config Application. It ignores non-ChatGPT
and different-account credentials, warns on malformed candidate content, and
fails before writing on an unsafe structural view.

**Keep Current Config direct and explicit.** The Console Configs module reveals
or edits either a Named Config or Current Config. It presents every native file
in Agent-defined order without credential redaction. Named Config writes validate
the selected file before committing; Current Config writes preserve arbitrary
bytes without syntax validation and may initialize a missing Managed Tenant. A
later file failure does not roll back an earlier committed file.

**Keep Agent permissions native.** Both built-in Named Config templates use
native Current Config for non-interactive, unrestricted operation. Do not add
permission or sandbox flags to invocation arguments, or enable the Claude
status line in its template. Claude stores `ANTHROPIC_AUTH_TOKEN` directly in
`settings.json.env`; Codex `auth.json` must be a JSON object and replaces the
native auth file as a whole. Every Named Config file is mode `0600`.

**Keep Components optional, native, and independently owned.** Tenant
initialization does not install status lines or toolchains. The Console Components
module derives
statusline state from native Managed or Host Tenant files without a registry;
Rust and Go remain Managed Tenant-only and install through the shared image with
only the Tenant Home mounted. Status-line Components directly manage their
script when applicable and their native configuration values. Status-line paths
are not Config Fields, so Config Application preserves them without ownership
or overlap machinery. Expose repairable partial state as `incomplete`.
Component removal confirms before deleting any existing state. Preserve Cargo
and GOPATH user state across SDK replacement and removal.

**Use explicit destructive selection.** Tenant, Named Config, and Session
deletion require names/ids or `--all`; an empty list never means all. `--all`
and explicit selections are mutually exclusive. Named Config deletion may
remove safe invalid or incomplete Named Config directories, but rejects unsafe
entries.

**Treat container-writable paths as untrusted.** Host-side reads, writes, and
deletions reject symlinked or unexpected entries and validate relevant
ancestors. Console Session listing may return readable rows with traversal
errors; Session detail and deletion fail on a partial view. Transcripts without a
typed prompt remain visible and deletable. Malformed JSONL and unsupported
user-like records warn and make Session listing/detail nonzero without hiding an
otherwise readable Transcript; deletion remains strict and format-independent.

**Keep missing scopes quiet.** Console Config and Session views return empty for
a missing Managed Tenant, and the Components view reports its catalog as not
installed. Host Component listing reports the two supported statuslines as not
installed when the Host Home or Agent state is missing. Read-only views create
nothing. Service startup initializes the Default Managed Tenant; Run, Config
creation, Current Config editing, and Managed Tenant Component installation may
initialize other missing state. Host statusline install may initialize an Agent
state directory inside an already existing Host Home.

**Do not imply cross-process coordination.** Tenant lifecycle can recover its
own interrupted filesystem work, but aibox provides no multi-process locking
guarantee. Config Application atomically replaces each changed file but is not
atomic across files; rerunning it converges. Sequential Config edits likewise
commit one file at a time without rollback. Credential Propagation uses its
preflight snapshots without write-time reconciliation, replaces targets
independently, continues after individual write failures, and never rolls back
successful targets. One aibox process supports only one active container
operation: a Run or toolchain installation.

**Keep the Request Proxy host-side and raw.** The Request Proxy is an always-on part of
the aibox Service, global rather than Tenant-owned, never starts Docker, and records raw application-visible header
values and body bytes under the flat `$AIBOX_ROOT/requests/<record>/` layout.
One explicit `--listen` socket serves both the Request Proxy and Console;
the surrounding network is trusted, so do not add authentication, TLS, request
admission checks, or network-exposure confirmations. Apart from the
`198.18.0.0/15` host-side Fake-IP DNS exception, do not add private-upstream
access, redaction, body limits, retention, WebSocket, CONNECT, or multi-process
coordination.

**Keep routine Request Proxy tests socket-free.** Default tests must not bind TCP or
Unix sockets: exercise Axum routers as in-memory Tower services and drive body
streams with deterministic synchronization. Keep real-socket Reqwest transport
checks explicit and ignored, and run them only in a network-permitted host or
CI environment. Test the embedded UI in layers: Rust route/API tests,
then Vitest module and component tests for the React and TypeScript source in
`console/`, then optional headless Chromium/Playwright interaction and
screenshots in a development image or CI. Edit that source rather than the
generated `assets/console.*` bundle, as `docs/console-ui.md` describes. Desktop
Browser access is never required for routine changes. A headless browser uses
the same container's loopback listener.

**Keep Run transient and the crate application-only.** Do not add Run History or a
Run-to-Session mapping. The Control API is Console-internal, not a public Rust
or HTTP embedding surface. A validated Run attempt may initialize its Tenant before
Docker startup fails or the Coding Agent returns nonzero. Expose only the
application entry point from the library target, not embedding-oriented module
or dispatch APIs.

**Treat `AIBOX_ROOT` as dedicated but unmarked.** Do not add an ownership
marker. Keep deletion structurally scoped and document that users must not
point the root at a general-purpose directory.

**Keep Docker runs cleanup-aware.** Runs and toolchain installers go
through `docker::run`; its child/cidfile registry supports one active container
operation per process. Register the cidfile before spawning Docker, register
the child immediately afterward, and keep cleanup armed until `finish_child`
checks for a container that outlived the Docker client.

**Keep the embedded Dockerfile context-free.** `docker_image.rs` passes it to
`docker build -f -` with an empty context, so dependencies must be fetched
during the build.

## Checks

For Rust changes, run all of these:

- `cargo fmt --check`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`

For embedded Requests UI changes, also run the complete socket-free frontend
check:

- `make console-check`

Keep the real-browser Playwright checks explicit and optional because they bind
a loopback listener.
