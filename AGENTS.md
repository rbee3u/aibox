# AGENTS.md

Guidance for AI agents working on this repo. Read this before editing.

## What this is

`aibox` is a single Rust CLI that runs a coding agent (Claude Code or OpenAI
Codex) inside a Docker container that **is** the sandbox boundary. One binary,
two agent subcommands:

```
aibox claude [options] [-- <args passed straight to claude>]
aibox codex  [options] [-- <args passed straight to codex>]
```

Both subcommands also carry `sync` (refresh config-file template docs) and
`session` (browse saved transcripts host-side). See `README.md` for user docs.

## Layout

```
src/
  main.rs        # thin bin: split argv at `--`, clap parse, call lib::run
  lib.rs         # orchestration (run / run_agent) + module wiring
  cli.rs         # clap types + split_passthrough
  agent.rs       # AgentKind enum + trait-like methods — THE divergence point
  profile.rs     # profile paths, home seeding, relay resolve/scaffold
  envfile.rs     # base+relay merge (IndexMap: order + last-wins)
  template.rs    # env-file templates + TEMPLATE_VERSION + stamp reader
  sync.rs        # sync merge engine + file dispatch
  session/       # transcript browsing: mod.rs (shared) + claude.rs + codex.rs
  docker.rs      # docker build/run child processes
  creds.rs       # 0600 temp creds + Drop + signal cleanup
  runspec.rs     # docker-run arg assembly + Invocation + home seeding
  platform.rs    # uid/gid, TTY, OS gate
assets/          # Dockerfiles + status.sh, embedded via include_str!
```

## Hard constraints

**The two agents diverge in exactly one place: `AgentKind` (`agent.rs`).**
Image name, config root, container home, Dockerfile, templates, the docker
invocation (endpoint wiring + agent command line), and the session backend all
hang off it. Shared logic (profile, envfile, sync, session surface, docker
hardening) is written once and takes an `AgentKind`. This is the type-enforced
replacement for the old "keep the two Bash scripts in parallel" rule: a
divergence that isn't expressed through `AgentKind` is a compile error, not a
copy-paste someone forgot. If you're about to special-case Claude vs Codex
outside `agent.rs` / `session/`, reconsider.

**Credentials never persist.** The API key is staged in a `0600` temp file
(the merged env-file for Claude, a key-only env-file or a throwaway `auth.json`
for Codex) and unlinked on every exit path. `creds.rs` covers both:
- normal / error path — `StagedFile`'s `Drop` unlinks;
- interrupt path — a SIGINT/SIGTERM handler unlinks every registered path and
  re-raises, because `Drop` does **not** run on a signal.

Docker runs as a **child** (`Command::status()`, never an exec-replace) so the
guards drop after it returns. Don't turn that into `exec`. If you add a new kind
of staged credential, register it through `creds.rs` so both paths cover it, and
manually test Ctrl-C mid-run.

**config.toml stays the user's (Codex).** The endpoint is injected only via
runtime `-c key=value` overrides (`agent.rs::build_codex`), never written to the
mounted `config.toml` — that file holds the user's `codex login`, trust levels,
MCP servers. The two auth modes are mutually exclusive: `env_key` (key via
`--env-file`) vs `requires_openai_auth` (key via a read-only `auth.json` mount);
setting both breaks Codex's own provider validation.

## Templates and `sync`

Env-file templates live in `template.rs` (`base_template` / `relay_template`) —
single source, shared by first-run scaffolding (`profile.rs`) and `sync`
(`sync.rs`). Rules:
- Every item is a `#` doc comment, then a commented `#KEY=example`. Nothing
  active — users append real lines under the examples.
- The first line is a `# aibox-template: vN` stamp. **Bump `TEMPLATE_VERSION`
  (in `agent.rs`) whenever you change a template**, so stale files get flagged
  and `sync` can refresh them.
- `sync` matches example lines by `^#[A-Za-z_][A-Za-z0-9_]*=` (see
  `sync::example_key`) and re-places the user's real lines under them. If you
  change example formatting, keep it matching that pattern, and update the
  `sync` tests.

## Dockerfiles

Neither Dockerfile has a `COPY` — they fetch everything via apt/curl/npm. That's
load-bearing: the build context is unused, so `docker.rs` feeds the embedded
Dockerfile to `docker build -f -` on stdin with an empty context. Keep them
`COPY`-free, or the stdin-build has to change.

## After editing

- `cargo build` and `cargo test` (unit tests live beside each module).
- `cargo clippy --all-targets` and `cargo fmt` — keep both clean.
- For run-path changes you can't unit-test, stub `docker` on `$PATH` with a
  script that echoes its args, and check the assembled `docker run` line.
- For `creds.rs` changes, manually verify a staged file is gone after both a
  normal exit and a Ctrl-C mid-run.
