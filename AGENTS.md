# AGENTS.md

`aibox` is a Rust CLI that runs Claude Code or OpenAI Codex inside a Docker
container. The container, not the agent process, is the sandbox boundary.
`README.md` is the concise entry point for evaluation, installation, first use,
and the core safety model. Advanced user behavior has one canonical home in
`docs/profiles.md`, `docs/providers.md`, or `docs/sandbox.md`; keep README
examples, those guides, and clap help aligned without copying full references
between them.

Keep this file limited to project-specific constraints that are hard to infer
from the code and costly to violate. Prefer existing modules and data flows;
add an abstraction, configuration layer, or compatibility path only for a
demonstrated requirement, such as a user request, test, published behavior, or
observed failure.

## Constraints

**Centralize shared agent contracts.** Put agent-specific paths, managed files,
and invocation behavior in `AgentKind` in `agent.rs`. Keep transcript-format
parsing in `session_claude.rs` and `session_codex.rs`. The Docker image and
container home remain shared so agent support does not fork shared
orchestration.

**Preserve the CLI boundary.** Split argv at the first `--` before clap parses
it, and pass the right side verbatim only to `run`. The `run`, `provider`, and
`session` commands own separately scoped `--agent`/`--profile` options;
`build`, `completion`, and `profile` accept neither. Completion must
mirror these scopes and the `--` boundary; candidate discovery stays host-side
and must not initialize profiles or start Docker. Keep clap help, README
examples, and scope-rejection tests aligned.

**Keep provider application explicit and persistent.** A run consumes the
active agent files left by `provider apply`; it must not inject or reapply
provider data. `provider apply` deep-merges `config.toml` or `settings.json` into
the current active config; Codex `auth.json` is validated and replaced as a
whole. Changing providers is not an implicit rollback or reset, so previously
applied keys may persist.

**Keep management data on the host.** Within `$AIBOX_ROOT`, only an ordinary
profile's `home` may be used as a bind source; all other data is host-only. The
special `host` profile lets `provider` and `session` operate on the real host
agent dirs while metadata stays under `$AIBOX_ROOT`; it has no managed home and
must be rejected by Docker runs and profile deletion. This prevents management
state and real host agent data from crossing the container boundary.

**Validate every bind mount before Docker sees it.** Resolve host sources so
they cannot become named volumes or escape path checks. Extra mounts may nest
beneath managed targets, but must not replace `/work` or the shared container
home.

**Treat container-writable profile paths as untrusted.** Host-side reads,
writes, and deletions must reject symlinked or unexpected path entries and
validate every relevant ancestor. `session list` may return readable rows with
traversal errors; `session get` and `session delete` must fail on a partial
view. Transcripts without a typed prompt must still be listed and included in
delete-all operations.

**Keep Docker runs single-active and cleanup-aware.** Agent runs must go through
`docker::run`; its child/cidfile registry supports one active run per process.
Register the cidfile before spawning Docker, register the child immediately
afterward, and keep cleanup armed until `finish_child` checks for a container
that outlived the Docker client, or a signal race can orphan the container.

**Keep the embedded Dockerfile context-free.** It must not depend on local
build-context files: `docker.rs` passes it to `docker build -f -` with an empty
context, so dependencies must be fetched during the build.

## Checks

Run checks relevant to the change before handing it off and report any check
the environment prevents. For Rust changes, run all of these:

- `cargo fmt --check`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`
