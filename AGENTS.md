# AGENTS.md

`aibox` is a Rust CLI that runs Claude Code or OpenAI Codex inside a Docker
container that **is** the sandbox boundary:
`aibox claude|codex [options] [-- <args passed straight to the agent>]`.
Subcommands also carry `config` for provider overlays and `session` for
host-side transcript browsing. User docs live in `README.md`.

## Layout

```
src/
  lib.rs               # orchestration (run / run_agent) + module wiring
  main.rs              # thin bin: split argv at `--`, clap parse, call lib::run
  agent.rs             # AgentKind enum + trait-like methods; divergence point
  cli.rs               # clap types + split_passthrough
  config.rs            # provider create/list/get/apply/edit/delete
  merge.rs             # TOML/JSON deep-merge helpers
  creds.rs             # docker run pid/cidfile signal cleanup
  docker.rs            # docker build/run child processes
  platform.rs          # uid/gid, TTY, OS gate
  profile.rs           # profile/root/host layout and path boundary checks
  runspec.rs           # docker-run arg assembly + minimal home seeding
  session.rs           # transcript browsing shared dispatch + backend trait
  session_claude.rs    # Claude transcript backend
  session_codex.rs     # Codex transcript backend
assets/                # Dockerfiles, embedded via include_str!
```

## Hard Constraints

**Agent divergence is centralized in `AgentKind` (`agent.rs`).** Everything
per-agent: image name, container home, active state dir (`.codex`/`.claude`),
managed config files, Dockerfile, permissions invocation, and session backend.
Shared logic takes an `AgentKind`; transcript parsing is the only split backend.

**Provider metadata never enters the container.** Normal profiles use
`$AIBOX_ROOT/{profile}` as the mounted home for both agents. Provider snapshots,
`.backup`, and `.state.json` live under
`$AIBOX_ROOT/.config/{profile}/{agent}/`; provider directories are direct
children of that directory. This management tree must not be mounted as part of
a normal run. `$AIBOX_ROOT` is `$AIBOX_CONFIG_ROOT` or `$HOME/.aibox`.

**`host` is a management-only profile.** `-p host` is valid for `config` and
`session` only. It targets the real host `$HOME/.codex` or `$HOME/.claude` while
keeping provider metadata under `$AIBOX_ROOT/.config/host/{agent}/`. Docker
runs must reject `host`.

**Config is persistent and applied before runs.** Do not reintroduce `-e`,
`base`, `envs`, runtime endpoint injection, or refresh templates. Codex providers
own `config.toml` plus `auth.json`; Claude providers own `settings.json`.
`config apply` deep-merges TOML/JSON config into the active agent dir. Codex
`auth.json` is validated and replaced as a whole file.

**Managed Docker mounts define the boundary.** `runspec.rs` owns `/work`, the
per-agent container home, and every extra bind mount. Always resolve and
validate host bind sources before passing them to Docker: relative sources can
become named volumes, and `:` breaks `-v` parsing. User `-m` mounts may be
nested under managed targets, but must not replace `/work` or
`AgentKind::container_home()`.

**Docker runs remain child processes.** `docker::run` registers the child pid
and cidfile through `creds.rs`, so wrapper-only signals do not leave a container
running. Do not bypass `docker::run` for agent runs.

## Config Safety

- Profile and provider names are restricted to `[A-Za-z0-9_-]+`.
- Host-side writes to active config, provider files, backups, and state use
  atomic replacement.
- Codex auth files and auth backups are private on Unix.
- Existing symlinked active dirs/files must be rejected rather than followed.
- `config delete` must ask before removing a provider unless `-y/--yes` is set.

## Dockerfiles

Embedded Dockerfiles must stay `COPY`-free (fetch via apt/curl/npm): the build
context is unused, so `docker.rs` pipes each one to `docker build -f -` with an
empty context.

## Checks

- `cargo fmt`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- Run-path changes you cannot unit-test: stub `docker` on `$PATH` with a script
  that echoes its args, and inspect the assembled `docker run` line.
