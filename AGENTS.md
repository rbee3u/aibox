# AGENTS.md

`aibox` is a Rust CLI that runs Claude Code or OpenAI Codex inside a Docker
container. The container, not the agent process, is the Filesystem Sandbox.
`CONTEXT.md` defines the canonical domain language; keep code, clap help, and
user documentation aligned with it. Architectural decisions live in
`docs/adr/`.

`README.md` is the concise entry point for evaluation, installation, first use,
and the core safety model. Advanced behavior has one canonical home in
`docs/tenants.md`, `docs/providers.md`, or `docs/sandbox.md`; keep examples and
clap help aligned without copying full references between them.

## Constraints

**Centralize Coding Agent contracts.** Put agent-specific paths, Provider
files, and invocation behavior in `AgentKind` in `agent.rs`. Keep transcript
parsing in `session_claude.rs` and `session_codex.rs`. The Docker image,
container Home, and orchestration remain shared.

**Preserve the CLI boundary.** Split argv at the first `--` before clap parses
it, and pass the right side verbatim only to `run`. `run`, `provider`, and
`session` own separately scoped `--agent`/`--tenant` options; `component` owns
only `--tenant`; only `provider` and `session` accept `--host`. `build`,
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

**Keep Provider activation explicit and persistent.** A Run consumes native
Agent Configuration and never injects or reapplies Provider data. Provider
catalogs and Active Provider state are local to one Tenant and Coding Agent.
Activation snapshots the base and applied Provider. Reconciliation is a
three-way merge of applied, source, and working state; conflicts require
explicit JSON Pointer choices.

**Keep Agent permissions native.** Both built-in Provider templates use native
Agent Configuration for non-interactive, unrestricted operation. Do not add
permission or sandbox flags to invocation arguments, or enable the Claude status
line in its template. Claude Provider `auth.json` is a string map projected into
`settings.env`; Codex `auth.json` is whole-file ownership. Every Provider auth
file is mode `0600`.

**Keep Components optional and native.** Tenant initialization does not install
status lines or toolchains. `component` operates only on Managed Tenants and
derives state from native Tenant Home files without a registry. Status-line
installation owns only its script and native configuration keys. Rust and Go
install through the shared image with only the Tenant Home mounted; preserve
Cargo and GOPATH user state across SDK replacement.

**Roll Provider transactions forward.** State-changing Provider commands first
persist typed pending changes in the Agent/Tenant `.metadata.json`, then apply
them idempotently and clear the pending record. Provider commands, Runs, and
status-line installation resume interrupted transactions. Pending records may
identify only known Agent and Provider files, never arbitrary paths. Do not add
backup, restore, rollback, or filesystem-lock machinery.

**Use explicit destructive selection.** Tenant, Provider, and Session deletion
require names/ids or `--all`; an empty list never means all. `--all` and
explicit selections are mutually exclusive. Explicit deletion rejects an
Active Provider; Provider `--all` keeps it and deletes the inactive Providers.

**Treat container-writable paths as untrusted.** Host-side reads, writes, and
deletions reject symlinked or unexpected entries and validate relevant
ancestors. `session list` may return readable rows with traversal errors;
`session get` and `session delete` fail on a partial view. Transcripts without a
typed prompt remain visible and deletable.

**Keep missing scopes quiet.** Provider list and Session list return empty for
a missing Managed Tenant, Provider status reports inactive, and Component list
reports every Component as not installed. Read-only commands and completion
create nothing. `run`, `provider create`, and `component install` may initialize
a missing Managed Tenant.

**Do not imply cross-process coordination.** Tenant lifecycle and Provider
transactions recover their own interrupted filesystem work, but aibox provides
no multi-process locking guarantee. One aibox process still supports only one
active Run.

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
