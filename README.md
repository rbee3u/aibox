# Put AI in a box

Run coding agents (Claude Code, OpenAI Codex) inside a Docker container that
**is** the sandbox boundary. The agent can run without its own permission
prompts while the host blast radius stays limited to the project mount and a
selected profile home.

## Commands

```sh
aibox build [claude|codex] [--force]
aibox codex [options] [-- <args passed to codex>]
aibox claude [options] [-- <args passed to claude>]
aibox codex config <list|get|create|apply|edit|delete> ...
aibox claude config <list|get|create|apply|edit|delete> ...
aibox codex session [list|get|delete|rm] ...
aibox claude session [list|get|delete|rm] ...
```

Normal runs never build images automatically:

```sh
aibox build codex
cd ~/code/some-project
aibox codex
aibox codex --exec -- "run the tests and fix failures"
```

## Profile Layout

The default root is `$HOME/.aibox`. Set `AIBOX_CONFIG_ROOT` to override it; a
relative value resolves from the launch directory.

```text
$AIBOX_ROOT/
├── default/                         # shared profile home mounted into Docker
│   ├── .codex/
│   └── .claude/
└── .config/
    └── default/
        ├── codex/
        │   ├── <provider>/
        │   ├── .backup/<timestamp>/
        │   └── .state.json
        └── claude/
            ├── <provider>/
            ├── .backup/<timestamp>/
            └── .state.json
```

`aibox codex -p work` and `aibox claude -p work` both mount
`$AIBOX_ROOT/work` as the agent home. Codex state lives under `.codex`; Claude
state lives under `.claude`.

`-p host` is special and only valid for `config` and `session`. It manages the
real host `$HOME/.codex` or `$HOME/.claude`, while provider snapshots and
backups still live under `$AIBOX_ROOT/.config/host/<agent>/`. Docker runs reject
`-p host`.

Profile and provider names must contain only letters, numbers, `_`, and `-`.

## Config Providers

Provider snapshots are edited and applied explicitly:

```sh
aibox codex config create openai
aibox codex config edit openai
aibox codex config edit openai --auth
aibox codex config apply openai
aibox codex config list
aibox codex config get openai
aibox codex config delete openai --yes

aibox claude -p work config create anthropic
aibox claude -p work config edit anthropic
aibox claude -p work config apply anthropic
```

Codex providers contain:

```text
config.toml
auth.json
```

Claude providers contain:

```text
settings.json
```

New Claude providers start with an `env` template for `ANTHROPIC_*` settings
and a command status line:

```json
{
  "env": {
    "ANTHROPIC_AUTH_TOKEN": "sk-example",
    "ANTHROPIC_BASE_URL": "https://example.ai",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-opus-5",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-opus-5[1m]",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-5[1m]",
    "ANTHROPIC_DEFAULT_FABLE_MODEL": "claude-fable-5[1m]"
  },
  "statusLine": {
    "type": "command",
    "command": "bash ~/.claude/statusline.sh"
  }
}
```

When a Claude profile home is initialized, aibox installs that
`~/.claude/statusline.sh` helper if it is missing. Existing regular files are
left untouched.

Applying a provider deep-merges TOML/JSON config into the active profile:
objects and TOML tables merge recursively; scalars and arrays are replaced.
Keys not mentioned by the provider remain in the active config unless the
provider asks aibox to remove them. The top-level `aibox` key is reserved for
apply metadata and is stripped before writing the active config:

```toml
[aibox.apply]
remove = ["model_provider", "model_providers.custom"]
```

For Claude `settings.json`, use the same dotted paths in JSON:

```json
{
  "aibox": {
    "apply": {
      "remove": ["some.setting"]
    }
  }
}
```

Remove paths that do not exist are ignored; malformed paths such as empty
strings or `foo..bar` fail the apply. Codex `auth.json` is not merged; it is
validated as a non-empty JSON object and replaced as a whole file.

Every `apply` creates a backup of existing active managed files under the
profile's management directory and keeps the latest 20 backups. Codex auth files
and auth backups are written with private permissions on Unix.

## Sessions

Sessions are browsed host-side; no container or provider is needed:

```sh
aibox codex session
aibox codex session get 3f2a
aibox codex session delete 3f2a
aibox codex session delete -y
aibox codex -p host session list
```

`list` prints short id, date, and title. `get <id>` prints your typed prompts.
`delete` asks before removing each transcript unless `-y/--yes` is supplied.

## Run Options

```text
-p, --profile <name>        profile home, default: default
-w, --work <dir>            directory mounted at /work, default: current dir
-m, --mount <spec>          extra bind mount host:container[:ro], repeatable
--safe                      keep the agent's own prompts/sandbox
--exec                      Codex only: run `codex exec`
```

By default, aibox launches the agent with its permission prompts/sandbox
bypassed because Docker is the sandbox boundary. Use `--safe` when you want the
agent's own approval and workspace sandbox as an additional layer.

## Building Images

```sh
aibox build
aibox build codex
aibox build claude --force
```

`aibox build` first builds the shared local base image, then the requested agent
image(s). The embedded Dockerfiles are `COPY`-free and pin the installed agent
CLI versions. Use `--force` to ignore Docker cache and pull a fresh Debian base.
