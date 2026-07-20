# AGENTS.md

`aibox` is a Rust CLI that runs a coding agent (Claude Code or OpenAI Codex)
inside a Docker container that **is** the sandbox boundary: `aibox claude|codex
[options] [-- <args passed straight to the agent>]`. Both subcommands also
carry `refresh` (refresh config-file template docs) and `session` (browse saved
transcripts host-side). User docs in `README.md`.

## Layout

```
src/
  main.rs        # thin bin: split argv at `--`, clap parse, call lib::run
  lib.rs         # orchestration (run / run_agent) + module wiring
  cli.rs         # clap types + split_passthrough
  agent.rs       # AgentKind enum + trait-like methods — divergence point
  profile.rs     # profile paths, config root, relay resolve/scaffold
  envfile.rs     # base+relay merge (IndexMap: order + last-wins)
  template.rs    # env-file templates + TEMPLATE_VERSION + stamp reader
  refresh.rs     # refresh merge engine + file dispatch
  session/       # transcript browsing: mod.rs (shared) + claude.rs + codex.rs
  docker.rs      # docker build/run child processes
  creds.rs       # 0600 temp creds + Drop + signal cleanup
  runspec.rs     # docker-run arg assembly + Invocation + home seeding
  platform.rs    # uid/gid, TTY, OS gate
assets/          # Dockerfiles + status script, embedded via include_str!
```

## Hard constraints

**Agent divergence is centralized in `AgentKind` (`agent.rs`).** Everything
per-agent — image name, config root, container home, Dockerfile, templates,
Docker invocation, session backend — hangs off it; shared logic is written once
and takes an `AgentKind`. Sole exception: transcript parsing, behind `session/`
backends. Don't special-case Claude vs Codex anywhere else.

**Credentials never persist across handled exits.** API keys are staged in
`0600` temp files and unlinked after the run — `StagedFile::drop` on the
normal/error path, a SIGINT/SIGTERM handler on the interrupt path (`Drop` does
**not** run on a signal); both live in `creds.rs`. Docker runs as a child
(`Command::status()`), never an exec-replace, so the guards get to drop.
Register any new staged credential through `creds.rs`, and never write secrets
into profile homes — SIGKILL skips all cleanup.

**config.toml stays the user's (Codex).** The endpoint is injected only via
runtime `-c key=value` overrides (`agent.rs::build_codex`), never written to
the mounted `config.toml` — that file holds the user's `codex login`, trust
levels, MCP servers. The two auth modes are mutually exclusive: `env_key` vs
`requires_openai_auth`; setting both breaks Codex's provider validation.

## Templates and `refresh`

Env-file templates live in `template.rs`, shared by first-run scaffolding
(`profile.rs`) and `refresh` (`refresh.rs`):
- Every item is a `#` doc comment plus a commented `#KEY=example`; nothing
  active — users append real lines under the examples.
- Bump `TEMPLATE_VERSION` (`agent.rs`) whenever you change a template; the
  `# aibox-template: vN` stamp is how stale files get flagged.
- `refresh` matches example lines by `^#[A-Za-z_][A-Za-z0-9_]*=`
  (`refresh::example_key`) and re-inserts the user's real lines under them; keep
  example formatting matching that pattern.

## Dockerfiles

Embedded Dockerfiles must stay `COPY`-free (fetch via apt/curl/npm): the build
context is unused, so `docker.rs` pipes each one to `docker build -f -` with an
empty context.

## Checks

- `cargo clippy --all-targets` and `cargo fmt`, in addition to build/test.
- Run-path changes you can't unit-test: stub `docker` on `$PATH` with a script
  that echoes its args, and inspect the assembled `docker run` line.
- `creds.rs` changes: manually verify staged files are gone after a normal exit
  **and** after Ctrl-C mid-run.
