# aibox

Run coding agents (Claude Code, OpenAI Codex) inside a Docker container that
**is** the sandbox boundary — so the agent can skip every permission prompt and
work unrestricted, while the blast radius stays inside the container.

Two sibling tools, same design:

| Tool          | Agent        | Wraps            |
| ------------- | ------------ | ---------------- |
| `aibox-claude` | Claude Code  | `@anthropic-ai/claude-code` |
| `aibox-codex`  | OpenAI Codex | `@openai/codex`  |

## Why

These agents run best with their permission prompts and OS sandbox turned off —
but that's only safe if something else contains them. Here the container is that
something: the agent runs wide-open *inside*, your host and other projects stay
untouched *outside*. Only the project you point it at (mounted at `/work`) and an
isolated per-profile home are reachable.

Hardening on every run: `--security-opt no-new-privileges`, `--cap-drop ALL`,
runs as your host uid/gid on Linux, credentials delivered ephemerally (never
written into the mounted home).

## Requirements

- Docker (Desktop on macOS, Engine on Linux)
- A relay/endpoint that serves the agent's API. Codex specifically needs a
  **Responses-API** endpoint; put a translating proxy (LiteLLM, etc.) in front
  of anything that speaks only OpenAI-chat or OpenRouter.

## Install

Symlink the scripts onto your `$PATH` (they resolve their own location, so the
Dockerfile is found through the symlink):

```sh
ln -s "$PWD/aibox-claude" ~/.local/bin/aibox-claude
ln -s "$PWD/aibox-codex"  ~/.local/bin/aibox-codex
```

## Quick start

```sh
# 1. Build the image (also re-run any time to upgrade to latest upstream)
aibox-codex --build

# 2. First use of a relay name scaffolds a config stub, then stops
aibox-codex -e myrelay
#    -> edit ~/.aibox/codex/default/envs/myrelay with your endpoint + key

# 3. Run it against the current directory
cd ~/code/some-project
aibox-codex -e myrelay
```

`aibox-claude` works the same way (`aibox-claude --build`, `-e <relay>`, …).

## Config layout

Everything is per-profile on the host, under `~/.aibox/<tool>/`:

```
~/.aibox/codex/
└── default/                 # profile (-c <name> to switch; default is "default")
      ├── base               # shared config inherited by every relay
      ├── envs/               # relay endpoints — pick one per run with -e <name>
      │     └── myrelay
      └── home/               # mounted as the agent's home (sessions, auth, config)
```

`base` holds shared settings; each relay under `envs/` is merged on top of it
(the relay wins; set a key to empty to blank out a base default). There's no
default endpoint — every run picks one with `-e`.

Config files use `docker --env-file` format (`KEY=VALUE`). Each key is
documented inline, then shown as a commented `#EXAMPLE`. You set one by adding a
real line under its example — leave the example itself as living reference.

### codex relay keys

| Key | Maps to | |
| --- | --- | --- |
| `CODEX_BASE_URL` | `model_providers.<id>.base_url` | required |
| `CODEX_API_KEY` | the API key | required |
| `CODEX_MODEL` | `model` | required |
| `CODEX_REASONING` | `model_reasoning_effort` | optional |
| `CODEX_PLAN_REASONING` | `plan_mode_reasoning_effort` | optional |
| `CODEX_REQUIRES_OPENAI_AUTH` | auth mode: env var (default) vs `auth.json` | optional |
| `CODEX_QUERY_PARAMS` | provider `query_params` (e.g. Azure `api-version`) | optional |
| `CODEX_INSTRUCTIONS_FILE` | `model_instructions_file` (a **host** path) | optional |

The endpoint is injected into Codex ephemerally via its own `-c key=value`
overrides, so nothing endpoint-related ever lands in the mounted `config.toml`
(which stays yours — `codex login`, trust levels, MCP servers live there). The
API key is staged in a `0600` temp file and removed on exit.

### claude relay keys

| Key | |
| --- | --- |
| `ANTHROPIC_BASE_URL` | required |
| `ANTHROPIC_AUTH_TOKEN` | required |
| `ANTHROPIC_DEFAULT_HAIKU_MODEL` / `_SONNET_` / `_OPUS_` / `_FABLE_` | model tiers |

## Keeping templates fresh: `sync`

Config files carry a template version stamp (`# aibox-template: vN`). When the
scripts' templates evolve, a normal run nudges you if a file is stale. Refresh
the docs without losing your config:

```sh
aibox-codex sync            # base + every relay in the profile
aibox-codex sync base       # just base
aibox-codex sync myrelay    # one relay
aibox-codex sync --dry-run  # print the result instead of writing
```

`sync` rewrites the doc/example comments to the current template while keeping
every real config line you added — each re-placed under its matching example.
A real key whose example no longer exists is kept in a trailing block, so
nothing is lost.

## Common flags

Both tools share these (see `-h` for the full list):

| Flag | |
| --- | --- |
| `-e, --env <name\|path>` | relay endpoint (required) |
| `-c, --config <profile>` | config profile (default `default`) |
| `-w, --work <dir>` | project dir mounted at `/work` (default `$PWD`) |
| `-m, --mount <spec>` | extra bind mount (`host:container[:ro]`, repeatable) |
| `-s, --shell` | open a bash shell instead of the agent |
| `--safe` | keep the agent's normal prompts/sandbox instead of bypassing |
| `--build` | rebuild the image from scratch (this is how you upgrade) |
| `--no-net` | disable networking |

`aibox-codex` also has `--exec` for headless runs: `aibox-codex -e r --exec -- "fix the build"`.

## Upgrading

`--build` forces `--no-cache --pull`, which re-pulls the base image and
re-resolves the "latest" Node / Go / agent versions. A plain cached build would
freeze them, so `--build` is the way to pick up new upstream releases.
