# AGENTS.md

`aibox` is a Rust CLI that runs Claude Code or OpenAI Codex inside a Docker
container. The container is the Filesystem Sandbox boundary.
`CONTEXT.md` defines the canonical domain language; keep code, clap help, and
user documentation aligned with it. Architectural decisions live in
`docs/adr/`.

`README.md` is the concise entry point for evaluation, installation, first use,
and the core safety model. Advanced behavior has one canonical home in
`docs/tenants.md`, `docs/profiles.md`, or `docs/sandbox.md`; keep examples and
clap help aligned without copying full references between them.

## Constraints

**Centralize Coding Agent contracts.** Put agent-specific paths, Agent Profile
files/templates, and invocation behavior in `AgentKind` in `agent.rs`. Keep
transcript parsing in `session_claude.rs` and `session_codex.rs`. The Docker
image, container Home, and orchestration remain shared.

**Preserve the CLI boundary.** Split argv at the first `--` before clap parses
it, and pass the right side verbatim only to `run`. `run`, `profile`, and
`session` own separately scoped `--agent`/`--tenant` options; `component` owns
only `--tenant`; only `profile` and `session` accept `--host`. `build`,
`completion`, and `tenant` accept none of them. Completion mirrors these
scopes, stays read-only, and runs on the host.

**Keep Tenants distinct.** A Managed Tenant is aibox-managed and runnable;
`host` is a valid Managed Tenant name. The Host Tenant is selected only with
`--host`, cannot Run, and never appears in `tenant list` or deletion. Only
`tenants/<name>` subtrees may be mounted from inside `$AIBOX_ROOT`.

**Keep the direct layout.** A Managed Tenant exists exactly when
`tenants/<name>` is a real directory. Agent/Tenant metadata lives under
`<agent>/<name>/`; Host Tenant metadata uses `<agent>/__host/`. Do not add layout
versions, migration readers, management wrappers, or lock directories. Ignore
unknown collection entries during listing, but reject explicitly selected
unsafe paths.

**Keep names and local permissions narrow.** Managed Tenant and Agent Profile
names are lowercase DNS labels of 1–63 characters. Newly created aibox root,
collection, metadata, Agent Profile, and Tenant Home boundary directories are
`0700`. Agent Profile files, scope metadata, and Agent Profile-materialized
Agent Configuration are `0600`; deactivation restores original native modes
and existing Host Home directory modes are never changed.

**Keep Agent Profile activation explicit and persistent.** A Run consumes
native Agent Configuration and never injects or reapplies Agent Profile data.
Agent Profile catalogs and Active Agent Profile state are local to one Tenant
and Coding Agent. Activation snapshots the base and applied Agent Profile.
Reconciliation is a three-way merge of applied, source, and working state;
conflicts require explicit JSON Pointer choices.

**Keep Agent permissions native.** Both built-in Agent Profile templates use
native Agent Configuration for non-interactive, unrestricted operation. Do not
add permission or sandbox flags to invocation arguments, or enable the Claude
status line in its template. Claude Agent Profile `auth.json` is a string map
projected into `settings.env`; Codex `auth.json` is whole-file ownership. Every
Agent Profile auth file is mode `0600`.

**Keep Components optional, native, and independently owned.** Tenant
initialization does not install status lines or toolchains. `component`
operates only on Managed Tenants and derives state from native Tenant Home
files without a registry. Status-line Components own only their script when
applicable and their protected native configuration keys. Claude owns
`/config/statusLine`; Codex owns `/config/tui/status_line` and
`/config/tui/status_line_use_colors`. Agent Profiles exclude those paths and
overlapping ownership is rejected.
Expose repairable partial state as `incomplete`. Component removal requires
explicit discard for modified/unmanaged state. Rust and Go install through the
shared image with only the Tenant Home mounted; preserve Cargo and GOPATH user
state across SDK replacement and removal.

**Roll Agent Profile transactions forward.** State-changing Agent Profile
commands first persist typed pending changes in the Agent/Tenant
`.metadata.json`, then apply them idempotently and clear the pending record.
Agent Profile commands, Runs, and status-line Component operations resume
interrupted transactions. Pending records may identify only known Agent and
Agent Profile files, never arbitrary paths. Do not add backup, restore,
rollback, or filesystem-lock machinery.

**Use explicit destructive selection.** Tenant, Agent Profile, and Session
deletion require names/ids or `--all`; an empty list never means all. `--all`
and explicit selections are mutually exclusive. Explicit deletion rejects an
Active Agent Profile; Agent Profile `--all` keeps it and deletes the inactive
Agent Profiles.

**Treat container-writable paths as untrusted.** Host-side reads, writes, and
deletions reject symlinked or unexpected entries and validate relevant
ancestors. `session list` may return readable rows with traversal errors;
`session get` and `session delete` fail on a partial view. Transcripts without a
typed prompt remain visible and deletable. Malformed JSONL and unsupported
user-like records warn and make `session list/get` nonzero without hiding an
otherwise readable Transcript; deletion remains strict and format-independent.

**Keep missing scopes quiet.** `profile list` and `session list` return empty for
a missing Managed Tenant, `profile status` reports inactive, and `component list`
reports every Component as not installed. Read-only commands and completion
create nothing. `run`, `profile create`, and `component install` may initialize
a missing Managed Tenant.

**Do not imply cross-process coordination.** Tenant lifecycle and Agent Profile
transactions recover their own interrupted filesystem work, but aibox provides
no multi-process locking guarantee. One aibox process supports only one active
container operation: a Run or toolchain installation.

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
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`
