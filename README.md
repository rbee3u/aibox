# Put AI in a Box

Run coding agents (Claude Code, OpenAI Codex) inside a Docker container that
**is** the sandbox boundary. The agent can run without its own permission
prompts while its host-filesystem access stays limited to the project mount, a
selected profile home, and any extra mounts you explicitly add.

## Requirements and Installation

The Rust wrapper currently targets Linux and macOS hosts. You need a working
Docker CLI and daemon; building the wrapper from source also requires Rust 1.85
or newer. The bundled Linux image supports `amd64` and `arm64`.

```sh
git clone https://github.com/rbee3u/aibox.git
cd aibox
cargo install --locked --path .
aibox build
```

Ensure Cargo's binary directory (normally `$HOME/.cargo/bin`) is on `PATH`.
`aibox build` downloads the image's development toolchains and pinned agent CLI
versions, so the first image build can take a while.

### Shell Completion

Generate and load dynamic completion for your current shell:

```sh
# Bash
source <(aibox completion bash)

# Zsh
source <(aibox completion zsh)

# Fish
aibox completion fish | source
```

If a bare Zsh setup has not initialized completion yet, run
`autoload -Uz compinit && compinit` first. Most Zsh frameworks do this already.

To enable completion in future shells, add the matching command itself to
`~/.bashrc`, `~/.zshrc`, or `~/.config/fish/config.fish`. Keep the command in
the startup file instead of caching its output: the generated registration
script calls back into the installed `aibox` binary and should be regenerated
after upgrades.

Completion covers aibox commands and options, local profile/provider/session
names, and host paths for `--work` and the source side of `--mount`. It stops at
the first `--`; arguments forwarded to Codex or Claude are not completed, and
completion never starts Docker.

## First Run and Authentication

Start the agent interactively the first time and follow its own sign-in flow:

```sh
aibox
aibox --agent claude
```

Agent credentials and settings persist in the selected profile home, so the
default profile keeps Codex state in `$AIBOX_ROOT/default/home/.codex` and
Claude state in `$AIBOX_ROOT/default/home/.claude`. Profiles do not share those
agent state directories. For a custom or API-compatible provider, create and
apply a provider overlay as described in [Config Providers](#config-providers)
before using a headless run.

## Commands

```sh
aibox [--agent codex|claude] [run-options] [-- <args passed to agent>]
aibox build [--force]
aibox completion <bash|zsh|fish>
aibox profile <list|create|delete> ...
aibox config [--agent codex|claude] [-p <profile>] <list|get|create|apply|edit|delete> ...
aibox session [--agent codex|claude] [-p <profile>] [list|get|delete] ...
```

Normal runs never build images automatically:

```sh
aibox build
cd ~/code/some-project
aibox
aibox --exec -- "run the tests and fix failures"
aibox --agent claude -- "fix the build"
```

An agent run returns the `docker run` exit status, which is normally the
selected agent's exit status. `aibox` setup, validation, and host-side command
failures return non-zero, so these commands can be used directly in scripts and
CI.

### Passing Arguments to the Agent

The first `--` is a hard boundary: aibox parses everything before it and
forwards everything after it unchanged to the selected agent. Put the profile,
work directory, mounts, and `--exec` before the boundary; put agent-specific
flags, prompts, and subcommands after it:

```sh
aibox -p work -- --model MODEL_NAME
aibox --agent claude -p work -- --model MODEL_NAME
```

Without `--exec`, aibox starts the selected agent normally. With `--exec`, it
inserts Codex's `exec` subcommand before the forwarded arguments. Subcommands
such as `build`, `completion`, `profile`, `config`, and `session` do not accept
pass-through arguments.

## Profile Layout

The default root is `$HOME/.aibox`. Set `AIBOX_ROOT` to override it; a relative
value resolves from the launch directory.

```text
$AIBOX_ROOT/
├── default/
│   ├── home/                        # only profile subtree mounted as agent home
│   │   ├── .codex/
│   │   ├── .claude/
│   │   │   └── statusline.sh
│   │   └── .gitconfig
│   └── config/
│       ├── codex/
│       │   ├── <provider>/
│       │   ├── .backup/<timestamp>/
│       │   └── .state.json
│       └── claude/
│           ├── <provider>/
│           ├── .backup/<timestamp>/
│           └── .state.json
└── host/
    └── config/
        ├── codex/
        └── claude/
```

For runs, `aibox -p work` and `aibox --agent claude -p work` both mount
`$AIBOX_ROOT/work/home` as the agent home. Codex state lives under `.codex`;
Claude state lives under `.claude`.

`-p host` is special and only valid for `config` and `session`. It manages the
real host `$HOME/.codex` or `$HOME/.claude`, while provider snapshots and
backups live under `$AIBOX_ROOT/host/config/<agent>/`. Docker runs do not
accept `-p host`. This profile is not an isolated copy: `config apply -p host`
writes the selected agent's live host configuration, and
`session delete -p host` deletes its real host transcripts. Config apply still
creates the backups described below; session deletion does not.

`aibox profile list` prints ordinary profiles in name order and then
`host [external-home]`. The built-in `host` profile is never created, deleted,
or selected by `profile delete --all`.

`tracing` is reserved as a future sibling of `home` and `config`. This release
does not create it or provide tracing commands. If present in an ordinary
profile, it is host-only data and is deleted with the profile.

Profile and provider names must contain only letters, numbers, `_`, and `-`.
Keep `$AIBOX_ROOT` dedicated to aibox data: `profile list` validates the whole
root, and commands reject symlinked or unexpected entries in any profile they
touch. Store unrelated files elsewhere rather than alongside `home`, `config`,
or reserved `tracing` directories.

### Upgrading the Legacy Layout

This release does not migrate the former layout automatically. It rejects an
`$AIBOX_ROOT/.config` directory and unexpected entries directly beneath a
profile, rather than guessing how to combine credentials or provider data.
Back up `$AIBOX_ROOT` and stop active aibox runs before moving data:

| Legacy location | Current location |
| --- | --- |
| `$AIBOX_ROOT/<profile>/.codex`, `.claude`, `.gitconfig`, and other home entries | `$AIBOX_ROOT/<profile>/home/` |
| `$AIBOX_ROOT/.config/<profile>/<agent>/` | `$AIBOX_ROOT/<profile>/config/<agent>/` |
| `$AIBOX_ROOT/.config/host/<agent>/` | `$AIBOX_ROOT/host/config/<agent>/` |

Create each destination first, move all former profile-home entries beneath its
new `home/` directory, and remove the legacy `.config` tree only after checking
the migrated profiles and providers.

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
`statusline.sh` helper if missing, writes a default `.gitconfig` that rewrites
GitHub SSH URLs to HTTPS (the host's SSH keys are not mounted by default), and
creates both agents' provider-management directories. Existing regular files
are left untouched, so you can replace either file with profile-specific
settings.

Profiles deliberately do not inherit the host's Git identity, SSH keys, or
credential helpers. The generated `.gitconfig` only rewrites common GitHub SSH
clone URLs; configure commit identity and repository authentication inside the
profile, or mount credentials explicitly and treat that mount as an authority
grant.

`profile delete` removes the entire ordinary profile: both agents' credentials,
settings, sessions, caches, provider snapshots, backups, and reserved tracing
data. It does not delete the shared Docker image. There is no undo, so make a
copy of anything you need before confirming. In a non-interactive shell,
deletion requires `--yes`; otherwise aibox refuses to proceed.

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

aibox config --agent claude -p work create anthropic
aibox config --agent claude -p work edit anthropic
aibox config --agent claude -p work apply anthropic
```

For `profile delete` and `config delete`, omitting targets is the same as
passing `--all`.

`config apply` is explicit: an agent run does not reapply a provider or mount
provider metadata. It uses the active files left under `.codex` or `.claude` by
the last apply (plus any later edits made by the agent or user).

Apply is cumulative, not a clean provider switch. Each apply merges the
provider's main config into the current active main config, so keys introduced
by an earlier provider remain until they are removed explicitly with
`aibox.config.apply.remove` or restored from a backup. Codex `auth.json` is the
exception and is replaced as a whole. The `*` shown by `config list` records the
last successful apply; it does not mean the active files contain only that
provider's settings.

Codex providers contain:

```text
config.toml
auth.json
```

New Codex providers are skeletons for a custom Responses-compatible endpoint.
Review the model and the custom provider's `name` and `base_url` in
`config.toml`; `auth.json` carries the endpoint's `OPENAI_API_KEY`.

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
  "skipDangerousModePermissionPrompt": true,
  "permissions": {
    "defaultMode": "bypassPermissions"
  }
}
```

Replace every `sk-example` placeholder before applying a provider; `apply`
rejects templates that still contain placeholder credentials. `config edit`
uses `$VISUAL`, then `$EDITOR`, and falls back to `vim`. `config list` marks the
last successfully applied provider with `*`. `config get` prints every managed
provider file, including Codex `auth.json`; treat its output as secret.

When a Claude profile home is initialized, aibox installs that
`~/.claude/statusline.sh` helper if it is missing. Existing regular files are
left untouched. With `-p host`, the helper is installed in the real
`$HOME/.claude` and runs on the host rather than in the image; its full
model/context display uses Bash and `jq`, with Git branch detection when `git`
is available. Edit the provider's `statusLine` setting if that is not suitable
for the host.

Applying a provider deep-merges TOML/JSON config into the active profile:
objects and TOML tables merge recursively; scalars and arrays are replaced.
Keys not mentioned by the provider remain in the active config unless the
provider asks aibox to remove them. The entire top-level `aibox` table/object is
reserved for aibox metadata and stripped before writing the active config. The
currently supported metadata is `aibox.config.apply.remove`:

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

Before replacing existing active managed files, `apply` backs them up under the
profile's management directory and keeps the latest 20 backups. The first apply
creates no empty backup when there are no active files yet. Codex auth files and
auth backups are written with private permissions on Unix.

Deleting a provider removes only its saved overlay and may clear the
last-applied marker; it does not roll back files already applied to the active
agent directory. There is no backup-restore subcommand. To restore one, copy
the desired managed files from its timestamped backup into
`$AIBOX_ROOT/<profile>/home/.codex/` or `.claude/` while no aibox run is using
that profile. For `-p host`, restore into the real `$HOME/.codex/` or
`.claude/`. Keep `auth.json` readable only by its owner. Non-interactive
provider deletion requires `--yes`.

## Sessions

Sessions are browsed host-side; no container or provider is needed:

```sh
aibox session
aibox session get 3f2a
aibox session delete 3f2a
aibox session delete --all -y
aibox session delete -y
aibox session -p host list
```

`list` prints short id, date, and title. `get <id>` prints your typed prompts;
`id` may be a full id or any unique prefix. `delete` asks before removing each
transcript unless `-y/--yes` is supplied; omitting ids is the same as passing
`--all`. Session deletion removes transcript files only; it does not remove
credentials, settings, or provider data, and aibox does not back up deleted
transcripts. Non-interactive deletion requires `--yes`.

Sessions with no recognized typed prompt still appear; their title is empty
unless the agent stored one, so bulk deletion can find every transcript. If
part of a session tree or a transcript cannot be read, `list` reports the
problem, prints any readable rows, and exits non-zero; `get` and `delete` abort
rather than operate on a partial view. Host-side browsing does not follow
symlinked profile homes, agent-state directories, transcript roots, or
transcript files.

## Run Options

Options are scoped to the command they affect. Put run options before any
subcommand; put `config` and `session` options after `config` or `session`.

```text
--agent <codex|claude>      agent selector for a run, default: codex
-p, --profile <name>        run profile home, default: default
-w, --work <dir>            directory mounted at /work, default: current dir
-m, --mount <spec>          extra bind mount host:container[:ro], repeatable
--exec                      Codex only: run `codex exec`
```

`config` and `session` each accept their own `--agent <codex|claude>` and
`-p, --profile <name>` after the command name; those scoped options can appear
before or after the leaf subcommand and its arguments.

New provider templates default the agents to unrestricted permission mode
because Docker is the sandbox boundary. To restore agent prompts or sandboxing,
edit and reapply the provider, or edit the active agent config directly.

Within `$AIBOX_ROOT`, `--work` and `--mount` sources are allowed only from an
ordinary `<profile>/home` tree. Profile roots, `config`, reserved `tracing`,
`host`, and ancestors of `$AIBOX_ROOT` are rejected so host-only data cannot
enter the container. This still permits an explicit mount of another ordinary
profile's home or a directory beneath it; doing so can expose that profile's
agent credentials and should be treated as a deliberate authority grant.

For example, select another project and expose reference material read-only:

```sh
aibox -w ../other-project -m ../reference:/reference:ro
```

Relative `--work` and mount source paths resolve from the directory where aibox
was launched. `--work` must name an existing directory; an extra mount source
may be an existing file or directory. Mount targets must be absolute. Because
aibox uses Docker's short `-v` syntax, source paths containing `:` are rejected.
Extra mounts may be nested beneath `/work` or `/home/aibox`, but may not replace
either managed mount or one of its ancestors.

## Sandbox Boundary

Each run drops Linux capabilities, enables `no-new-privileges`, and
bind-mounts only the selected profile home, `/work`, and explicit `--mount`
sources. On Linux, the container uses the invoking user's uid and gid so files
created under `/work` keep host ownership.

The project mount, profile home, and extra mounts are writable by default.
Append `:ro` to an extra mount to make it read-only; no other mount modes are
accepted. Container networking is not disabled, so the agent can still reach
network services allowed by Docker and the host. The boundary does not limit
remote actions authorized by credentials stored in the profile. Treat every
credential and extra mount as an intentional expansion of the agent's
authority. Aibox also adds no CPU, memory, or process-count limits; Docker
daemon defaults apply unless limits are imposed outside aibox.

The wrapper handles SIGINT, SIGTERM, and non-ignored SIGHUP by stopping the
active container through Docker. Uncatchable termination such as SIGKILL, a
wrapper crash, or a host failure cannot run that cleanup; after such an event,
check Docker for a leftover container.

## Building Images

```sh
aibox build
aibox build --force
```

`aibox build` builds one shared `aibox:latest` image with both Codex and Claude
Code installed. The embedded Dockerfile is `COPY`-free and pins the installed
agent CLI versions. Use `--force` to ignore Docker cache and pull a fresh Debian
base. Set `AIBOX_IMAGE` to build and run a different shared image tag.

The Debian-based image includes common Unix development and diagnostic tools,
Python with pip/venv/uv, Node.js with npm, Rust, and Go. The
[embedded Dockerfile](assets/aibox.Dockerfile) is the source of truth for the
installed package list and pinned version defaults.

`AIBOX_IMAGE` changes the image reference, not aibox's runtime contract. An
independently built replacement must provide the selected `codex` or `claude`
binary on `PATH`, set `HOME=/home/aibox`, and support `/work` as the working
directory. It must not define an incompatible `ENTRYPOINT`. On Linux, aibox
also overrides the container user with the invoking host uid and gid, so the
image's executables and required files must be accessible to that arbitrary
user.

## Development

Run the repository checks before submitting changes:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
```

The Dockerfile is embedded at compile time and must remain `COPY`-free because
image builds use an empty context. For run-path changes that are difficult to
unit-test, put a stub `docker` executable first on `PATH` and inspect the
assembled command.

## License

[BSD 2-Clause](LICENSE)
