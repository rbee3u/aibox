# Put AI in a Box

Run coding agents (Claude Code, OpenAI Codex) inside a Docker container that
**is** the sandbox boundary. The agent can run without its own permission
prompts while the host blast radius stays limited to the project mount and a
selected profile home.

## Commands

```sh
aibox [--agent codex|claude] [options] [-- <args passed to agent>]
aibox [--agent codex|claude] config <list|get|create|apply|edit|delete> ...
aibox [--agent codex|claude] session [list|get|delete] ...
aibox build [--force]
aibox profile <list|create|delete> ...
```

Normal runs never build images automatically:

```sh
aibox build
cd ~/code/some-project
aibox
aibox --exec -- "run the tests and fix failures"
aibox --agent claude -- "fix the build"
```

## Profile Layout

The default root is `$HOME/.aibox`. Set `AIBOX_ROOT` to override it; a relative
value resolves from the launch directory.

```text
$AIBOX_ROOT/
├── default/                         # shared profile home mounted into Docker
│   ├── .codex/
│   ├── .claude/
│   │   └── statusline.sh
│   └── .gitconfig
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

`aibox -p work` and `aibox --agent claude -p work` both mount `$AIBOX_ROOT/work`
as the agent home. Codex state lives under `.codex`; Claude state lives under
`.claude`.

`-p host` is special and only valid for `config` and `session`. It manages the
real host `$HOME/.codex` or `$HOME/.claude`, while provider snapshots and
backups still live under `$AIBOX_ROOT/.config/host/<agent>/`. Docker runs reject
`-p host`.

Profile and provider names must contain only letters, numbers, `_`, and `-`.

Manage profile homes explicitly when you want to pre-create or remove them:

```sh
aibox profile create work
aibox profile list
aibox profile delete work --yes
aibox profile delete work scratch --yes
aibox profile delete --all --yes
```

Profile creation is still implicit for normal runs and provider setup. Creating
a profile initializes both `.codex` and `.claude`, installs Claude's
`statusline.sh` helper if missing, writes a default `.gitconfig` for GitHub SSH
URL rewriting if missing, and creates both agents' provider-management
directories. Existing regular files are left untouched.

## Config Providers

Provider snapshots are edited and applied explicitly:

```sh
aibox config create openai
aibox config edit openai
aibox config edit openai --auth
aibox config apply openai
aibox config list
aibox config get openai
aibox config delete openai --yes
aibox config delete openai anthropic --yes
aibox config delete --all --yes

aibox --agent claude -p work config create anthropic
aibox --agent claude -p work config edit anthropic
aibox --agent claude -p work config apply anthropic
```

For `profile delete` and `config delete`, omitting targets is the same as
passing `--all`.

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
  },
  "permissions": {
    "defaultMode": "bypassPermissions",
    "skipDangerousModePermissionPrompt": true
  }
}
```

When a Claude profile home is initialized, aibox installs that
`~/.claude/statusline.sh` helper if it is missing. Existing regular files are
left untouched.

Applying a provider deep-merges TOML/JSON config into the active profile:
objects and TOML tables merge recursively; scalars and arrays are replaced.
Keys not mentioned by the provider remain in the active config unless the
provider asks aibox to remove them. The `aibox.config.apply` key is reserved
for apply metadata and is stripped before writing the active config:

```toml
[aibox.config.apply]
remove = ["model_provider", "model_providers.custom"]
```

For Claude `settings.json`, use the same dotted paths in JSON:

```json
{
  "aibox": {
    "config": {
      "apply": {
        "remove": ["some.setting"]
      }
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
aibox session
aibox session get 3f2a
aibox session delete 3f2a
aibox session delete --all -y
aibox session delete -y
aibox -p host session list
```

`list` prints short id, date, and title. `get <id>` prints your typed prompts.
`delete` asks before removing each transcript unless `-y/--yes` is supplied;
omitting ids is the same as passing `--all`.

## Run Options

```text
--agent <codex|claude>      agent selector for run/config/session, default: codex
-p, --profile <name>        profile home, default: default
-w, --work <dir>            directory mounted at /work, default: current dir
-m, --mount <spec>          extra bind mount host:container[:ro], repeatable
--exec                      Codex only: run `codex exec`
```

New provider templates default the agents to unrestricted permission mode
because Docker is the sandbox boundary. To restore agent prompts or sandboxing,
edit the provider or active agent config and apply it again.

`--work` and `--mount` must not overlap `$AIBOX_ROOT/.config`; that management
tree contains provider snapshots and backups and is intentionally kept out of
the container.

## Building Images

```sh
aibox build
aibox build --force
```

`aibox build` builds one shared `aibox:latest` image with both Codex and Claude
Code installed. The embedded Dockerfile is `COPY`-free and pins the installed
agent CLI versions. Use `--force` to ignore Docker cache and pull a fresh Debian
base. Set `AIBOX_IMAGE` to build and run a different shared image tag.
