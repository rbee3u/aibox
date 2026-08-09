# AGENTS.md

`aibox` is a Rust CLI that runs Claude Code or OpenAI Codex inside a Docker
container. The container is the Filesystem Sandbox boundary.
`CONTEXT.md` defines the canonical domain language; keep code, clap help, and
user documentation aligned with it. Architectural decisions live in
`docs/adr/`.

`README.md` is the concise entry point for evaluation, installation, first use,
and the core safety model. Advanced behavior has one canonical home in
`docs/tenants.md`, `docs/configs.md`, or `docs/sandbox.md`; keep examples and
clap help aligned without copying full references between them.

## Constraints

**Centralize Coding Agent contracts.** Put agent-specific paths, Config
files/templates, empty Current Config content, and invocation behavior in
`AgentKind` in `agent.rs`. Keep
transcript parsing in `session_claude.rs` and `session_codex.rs`. The Docker
image, container Home, and orchestration remain shared.

**Preserve the CLI boundary.** Split argv at the first `--` before clap parses
it, and pass the right side verbatim only to `run`. `run`, `config`, and
`session` own separately scoped `--agent`/`--tenant` options; `component` owns
`--tenant` and `--host` (Host supports statusline Components only); only
`config`, `session`, and `component` accept `--host`. `build`,
`completion`, and `tenant` accept none of them. `traffic` owns only `--listen`
and `--allow-remote`; it does not accept selectors or pass-through arguments.
`config propagate-auth` defaults to Host/Codex/Current and accepts only the
redundant compatible selectors `--host`, `--agent codex`, and `--current`.
Completion mirrors these scopes, stays read-only, runs on the host, and hides
`propagate-auth` after an incompatible source selector.

**Keep Tenants distinct.** A Managed Tenant is aibox-managed and runnable;
`host` is a valid Managed Tenant name. The Host Tenant is selected only with
`--host` by Tenant-scoped commands; global Credential Propagation defaults its
source to Host Current Config and may accept a redundant `--host`. The Host
Tenant cannot Run and never appears in `tenant list` or deletion. Only
`tenants/<name>` subtrees may be mounted from inside `$AIBOX_ROOT`.

**Keep the direct layout.** A Managed Tenant exists exactly when
`tenants/<name>` is a real directory. Named Config catalogs live under
`<agent>/<name>/`; the Host Tenant catalog uses `<agent>/__host/`. A Claude
Named Config contains only native `settings.json`; a Codex Named Config
contains only native `config.toml` and `auth.json`. Do not add scope/Config
metadata, layout versions, migration readers, management wrappers, or lock
directories. Ignore unknown collection entries during listing, but reject
explicitly selected unsafe paths.

**Keep names and local permissions narrow.** Managed Tenant and Named Config
names are lowercase DNS labels of 1–63 characters. Newly created aibox root,
collection, Named Config catalog, Named Config, and Tenant Home boundary
directories are `0700`. Named Config files and newly created Current Config
files are `0600`; applying or directly editing Current Config preserves
existing file modes, including for the Host Tenant. Existing Host Home
directory modes are never changed.

**Keep Config Application explicit and one-shot.** A Run consumes Current
Config and never reads or reapplies Named Config data. Each Named Config
belongs to one Tenant and Coding Agent and defines only the fixed
Config Fields centralized in `AgentKind`. `config apply` sets present fields,
deletes missing fields, preserves unrelated native configuration, and retains
no association with the Named Config afterward. Do not add activation, drift,
reconciliation, rollback, or transaction state.

**Keep Credential Propagation explicit and one-shot.** `config propagate-auth`
copies one Host Codex Current Config `auth.json` snapshot only to older existing
same-account ChatGPT Credentials in complete safe Named Configs and Managed
Tenant Current Configs. It creates nothing, retains no association, never runs
automatically, and is distinct from Config Application. It ignores non-ChatGPT
and different-account credentials, warns on malformed candidate content, and
fails before writing on an unsafe structural view.

**Keep Current Config direct and explicit.** `config get` and `config edit`
require either a Named Config name or `--current`; other Config commands operate
only on Named Configs except for global Credential Propagation. `get` prints
every native file in Agent-defined order
with file headings and without credential redaction. `edit` opens and commits
each file separately in that order. Named Config edits validate the selected
file before committing; Current Config edits preserve arbitrary bytes without
syntax validation and may initialize a missing Managed Tenant. A later editor
failure does not roll back an earlier committed file.

**Keep Agent permissions native.** Both built-in Named Config templates use
native Current Config for non-interactive, unrestricted operation. Do not add
permission or sandbox flags to invocation arguments, or enable the Claude
status line in its template. Claude stores `ANTHROPIC_AUTH_TOKEN` directly in
`settings.json.env`; Codex `auth.json` must be a JSON object and replaces the
native auth file as a whole. Every Named Config file is mode `0600`.

**Keep Components optional, native, and independently owned.** Tenant
initialization does not install status lines or toolchains. `component` derives
statusline state from native Managed or Host Tenant files without a registry;
Rust and Go remain Managed Tenant-only and install through the shared image with
only the Tenant Home mounted. Status-line Components directly manage their
script when applicable and their native configuration values. Status-line paths
are not Config Fields, so Config Application preserves them without ownership
or overlap machinery. Expose repairable partial state as `incomplete`. Remove
prompts before deleting any existing Component state and requires `--yes` in a
non-interactive shell; it does not require a separate discard flag. Preserve
Cargo and GOPATH user state across SDK replacement and removal.

**Use explicit destructive selection.** Tenant, Named Config, and Session
deletion require names/ids or `--all`; an empty list never means all. `--all`
and explicit selections are mutually exclusive. Named Config deletion may
remove safe invalid or incomplete Named Config directories, but rejects unsafe
entries.

**Treat container-writable paths as untrusted.** Host-side reads, writes, and
deletions reject symlinked or unexpected entries and validate relevant
ancestors. `session list` may return readable rows with traversal errors;
`session get` and `session delete` fail on a partial view. Transcripts without a
typed prompt remain visible and deletable. Malformed JSONL and unsupported
user-like records warn and make `session list/get` nonzero without hiding an
otherwise readable Transcript; deletion remains strict and format-independent.

**Keep missing scopes quiet.** `config list` and `session list` return empty for
a missing Managed Tenant, and `component list` reports its catalog as not
installed. Host Component listing reports the two supported statuslines as not
installed when the Host Home or Agent state is missing. Read-only commands and
completion create nothing. `run`, `config create`, `config edit --current`,
and Managed Tenant `component install` may initialize missing state;
Host statusline install may initialize an Agent state directory inside an
already existing Host Home.

**Do not imply cross-process coordination.** Tenant lifecycle can recover its
own interrupted filesystem work, but aibox provides no multi-process locking
guarantee. Config Application atomically replaces each changed file but is not
atomic across files; rerunning it converges. Sequential Config edits likewise
commit one file at a time without rollback. Credential Propagation uses its
preflight snapshots without write-time reconciliation, replaces targets
independently, continues after individual write failures, and never rolls back
successful targets. One aibox process supports only one active container
operation: a Run or toolchain installation.

**Keep Traffic host-side and raw.** The Traffic Proxy is global rather than
Tenant-owned, never starts Docker, and records raw application-visible header
values and body bytes under the flat `$AIBOX_ROOT/traffic/<record>/` layout.
Management routes remain loopback-only even when proxy traffic is allowed on a
non-loopback listener. Apart from the `198.18.0.0/15` host-side Fake-IP DNS
exception, do not add private-upstream access, redaction, body limits,
retention, WebSocket, CONNECT, or multi-process coordination.

**Keep routine Traffic tests socket-free.** Default tests must not bind TCP or
Unix sockets: exercise Axum routers as in-memory Tower services and drive body
streams with deterministic synchronization. Keep real-socket Reqwest transport
checks explicit and ignored, and run them only in a network-permitted host or
CI environment. Test the embedded UI in layers: Rust route/API/security tests,
`node --check assets/traffic.js` plus Node tests for extracted pure JavaScript,
then optional headless Chromium/Playwright interaction and screenshots in a
development image or CI. Desktop Browser access is never required for routine
changes. A headless browser must use the same container's loopback listener;
do not publish or weaken the loopback-only management interface for testing.

**Keep Run transient and the crate CLI-only.** Do not add Run History or a
Run-to-Session mapping. A validated Run attempt may initialize its Tenant before
Docker startup fails or the Coding Agent returns nonzero. Expose only the
application entry point from the library target, not embedding-oriented module
or dispatch APIs.

**Treat `AIBOX_ROOT` as dedicated but unmarked.** Do not add an ownership
marker. Keep deletion structurally scoped and document that users must not
point the root at a general-purpose directory.

**Keep Docker runs cleanup-aware.** Agent Runs and toolchain installers go
through `docker::run`; its child/cidfile registry supports one active container
operation per process. Register the cidfile before spawning Docker, register
the child immediately afterward, and keep cleanup armed until `finish_child`
checks for a container that outlived the Docker client.

**Keep the embedded Dockerfile context-free.** `docker.rs` passes it to
`docker build -f -` with an empty context, so dependencies must be fetched
during the build.

## Checks

For Rust changes, run all of these:

- `cargo fmt --check`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`

For embedded Traffic UI changes, also run the complete socket-free frontend
check:

- `make traffic-check`

Keep the real-browser Playwright checks explicit and optional because they bind
a loopback listener.
