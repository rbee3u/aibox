# AGENTS.md

`aibox` is a Rust CLI that runs Claude Code or OpenAI Codex inside a Docker
container that **is** the sandbox boundary:
`aibox [--agent codex|claude] [options] [-- <args passed straight to the agent>]`.
Top-level subcommands carry `config` for provider overlays and `session` for
host-side transcript browsing. Run/config/session accept `--agent`; `build` and
`profile` do not. User docs live in `README.md`.

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
  platform.rs          # Linux-specific run flags, uid/gid, and TTY probes
  profile.rs           # profile/root/host layout and path boundary checks
  runspec.rs           # mount boundary checks + docker-run arg assembly
  session.rs           # transcript browsing shared dispatch + backend trait
  session_claude.rs    # Claude transcript backend
  session_codex.rs     # Codex transcript backend
  testutil.rs          # shared test-only env, stub, argv, and fixture helpers
assets/
  aibox.Dockerfile     # shared image, embedded via include_str!
  claude-status.sh     # default Claude status-line helper
```

## Hard Constraints

**Agent divergence is centralized in `AgentKind` (`agent.rs`).** Everything
per-agent: active state dir (`.codex`/`.claude`), managed config files,
supported invocation modes, command binary, and session backend. The Docker
image and container home are shared. Shared logic takes an `AgentKind`;
transcript parsing is the only split backend.

**The first `--` is the agent-argument boundary.** `main.rs` must split argv
before clap sees it, and only an agent run may consume the pass-through side.
Root `--agent`/`--profile` select a run; `config` and `session` own their
agent/profile options; `build` and `profile` accept neither. Keep clap help,
README examples, and scope-rejection tests aligned when changing this surface.

**Provider metadata never enters the container.** Normal profiles use
`$AIBOX_ROOT/{profile}/home` as the mounted home for both agents. Provider
snapshots, `.backup`, and `.state.json` live under
`$AIBOX_ROOT/{profile}/config/{agent}/`; provider directories are direct
children of that directory. `tracing` is reserved as another host-only profile
subtree. User mount sources inside `$AIBOX_ROOT` are allowed only beneath an
ordinary profile's `home`. `$AIBOX_ROOT` defaults to `$HOME/.aibox`.
The former root-level `.config/{profile}/{agent}` layout is rejected rather
than migrated implicitly; the user migration map belongs in `README.md`.

**`host` is a management-only profile.** `-p host` is valid for `config` and
`session` only. It targets the real host `$HOME/.codex` or `$HOME/.claude` while
keeping provider metadata under `$AIBOX_ROOT/host/config/{agent}/`. It has no
managed `home`; Docker runs and profile deletion must reject `host`.

**Config apply is explicit and persistent.** Runs use the active agent files
left by an earlier `config apply`; they do not reapply a provider or inject
provider data at launch. Do not reintroduce `-e`, `base`, `envs`, runtime
endpoint injection, or refresh templates. Codex providers own `config.toml`
plus `auth.json`; Claude providers own `settings.json`. `config apply`
deep-merges TOML/JSON config into the active agent dir. Codex `auth.json` is
validated and replaced as a whole file. The top-level `aibox` table/object is
reserved metadata and is stripped from active output. Applies merge into the
current active files; changing providers is not an implicit rollback or reset.

**Managed Docker mounts define the boundary.** `runspec.rs` owns `/work`, the
shared container home, and every extra bind mount. Always resolve and
validate host bind sources before passing them to Docker: relative sources can
become named volumes, and `:` breaks `-v` parsing. User `-m` mounts may be
nested under managed targets, but must not replace `/work` or
`AgentKind::container_home()`.

**Docker runs remain child processes.** `docker::run` registers the child pid
and cidfile through `creds.rs`, so catchable wrapper-only signals trigger
daemon-side container cleanup. The registry is process-global and supports one
active run; do not call `docker::run` concurrently or bypass it for agent runs.
Registration order is part of the safety contract: register the cidfile before
spawning Docker and record the child pid immediately afterward. After a
successful spawn, do not clear either until `finish_child` has checked for a
container that outlived the client. Signal-path Docker commands must stay
bounded, inherited ignored SIGHUP must remain ignored, and a second signal must
retain immediate escalation.

**Host-side session access must stay beneath the selected home.** Profile homes
are container-writable, so transcript discovery and reads must reject symlinked
homes, agent state directories, transcript roots, and transcript files.
`session list` may report readable rows alongside traversal errors, but
`get`/`delete` must fail on a partial view. A no-id delete includes every
transcript, even one with no typed prompt.

## Config Safety

- Profile and provider names are restricted to `[A-Za-z0-9_-]+`.
- Aibox-managed replacements of active config, provider templates, and state
  use same-directory temporary files and atomic rename.
- Backups use unique directories; failed backup creation removes the incomplete
  directory.
- Codex auth files and auth backups are private on Unix.
- Profile initialization and deletion validate the complete selected profile
  layout before writing or removing anything; legacy, unknown, and symlinked
  layout entries are rejected.
- Existing symlinked active dirs/files must be rejected rather than followed.
- `real_dir_exists`, `open_real_file`, and `ensure_real_dir` protect only their
  final path entry. Callers operating below container-writable homes must first
  validate every ancestor instead of treating these helpers as recursive
  symlink protection.
- `config delete` must ask before removing a provider unless `-y/--yes` is set.

## Dockerfiles

The embedded Dockerfile must stay `COPY`-free (fetch via apt/curl/npm): the build
context is unused, so `docker.rs` pipes it to `docker build -f -` with an
empty context.

## Checks

- `cargo fmt --check`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`
- `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`
- Run-path changes you cannot unit-test: stub `docker` on `$PATH` with a script
  that echoes its args, and inspect the assembled `docker run` line.
