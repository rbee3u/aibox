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
`aibox build` downloads the image's development runtimes and pinned agent CLI
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

Add the matching command to your shell startup file. A bare Zsh setup may need
`autoload -Uz compinit && compinit` first.

Completion includes host-side profile, provider, session, and path candidates.
It stops at the first `--` and never initializes profiles or starts Docker.

## Quick Start

Start the agent interactively and follow its sign-in flow:

```sh
aibox
aibox --agent claude
```

Credentials and settings persist in the selected profile. Profiles do not
share agent state. Normal runs never build the image automatically:

```sh
aibox build
cd ~/code/some-project
aibox
aibox -- exec "run the tests and fix failures"
aibox --agent claude -- "fix the build"
```

The first `--` is a hard boundary: aibox parses everything before it and
forwards everything after it unchanged to the selected agent. Put the profile,
work directory, and mounts before the boundary; put agent-specific flags,
prompts, and subcommands after it:

```sh
aibox -p work -- --model MODEL_NAME
aibox -p work -- exec "run the tests and fix failures"
aibox --agent claude -p work -- --model MODEL_NAME
```

Use `aibox --help` and `aibox <command> --help` for the full command reference.

## Profile Layout

The default root is `$HOME/.aibox`; set `AIBOX_ROOT` to override it. A relative
override resolves from the directory where aibox was launched.

| Path | Purpose |
| --- | --- |
| `<profile>/home/` | Mounted at `/home/aibox` for both agents |
| `home/.codex/`, `home/.claude/` | Agent credentials, settings, and sessions |
| `home/.cargo/`, `home/.rustup/` | Optional Rust installation |
| `home/.goroot/`, `home/.gopath/` | Optional Go installation |
| `config/<agent>/` | Host-only providers and backups |
| `config/<agent>/.lock` | Host-only provider mutation coordination |
| `.locks/<profile>` | Host-only run/mutation coordination |

`-p host` is special and only valid for `config` and `session`. It manages the
real host `$HOME/.codex` or `$HOME/.claude`, while provider snapshots and
backups live under `$AIBOX_ROOT/host/config/<agent>/`. Docker runs do not
accept `-p host`. This profile is not an isolated copy: `config apply -p host`
writes the selected agent's live host configuration, and
`session delete -p host` deletes its real host transcripts. Config apply still
creates the backups described below; session deletion does not.

Applying a Claude provider to `host` also installs and may enable the bundled
status-line script in the real `$HOME/.claude`. It runs on the host and expects
Bash and `jq`; Git branch detection is optional.

Profile and provider names use only letters, numbers, `_`, and `-`. Keep
`$AIBOX_ROOT` dedicated to aibox data: profile operations reject unsupported
or symlinked layout entries instead of guessing how to handle them.

Manage profile homes explicitly when you want to pre-create or remove them:

```sh
aibox profile create work
aibox profile list
aibox profile delete work --yes
aibox profile delete work scratch --yes
aibox profile delete --all --yes
```

Profiles are also initialized by a normal run or provider creation. This seeds
both agent state directories, Claude's `statusline.sh`, and a `.gitconfig` that
rewrites common GitHub SSH clone URLs to HTTPS. Existing regular seed files are
preserved. Profiles do not inherit the host's Git identity, SSH keys, or
credential helpers; configure those in the profile or mount them explicitly.

`profile delete` removes both agents' credentials, settings, sessions, caches,
providers, and backups. It does not delete the shared image. Omitting names
means all ordinary profiles. Deletion asks before each profile unless `--yes`
is set, and non-interactive deletion requires it. There is no undo.

### Profile-local Rust

Rust is not installed in the shared image. Have the agent run rustup, or use
`docker exec -it <container> sh` while that profile's container is running:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
  | sh -s -- -y --no-modify-path --profile default
```

Rustup persists binaries and caches in `$HOME/.cargo` and toolchains in
`$HOME/.rustup`. Aibox already puts `$HOME/.cargo/bin` on `PATH`. Append
`--default-toolchain <version>` to pin a release.

### Profile-local Go

Go is also profile-local. Have the agent run the following command, or use
`docker exec` as above:

```sh
rm -rf "$HOME/.goroot" && mkdir "$HOME/.goroot"
version=1.26.5
curl -fsSL "https://go.dev/dl/go${version}.linux-$(dpkg --print-architecture).tar.gz" \
  | tar -xz -C "$HOME/.goroot" --strip-components=1
```

Update `version` from the official [Go downloads](https://go.dev/dl/) page.
Aibox sets
`GOROOT=$HOME/.goroot`, `GOPATH=$HOME/.gopath`, and adds both binary directories
to `PATH`. The module and build caches also stay in the selected profile home.

## Config Providers

Create, edit, and explicitly apply a provider overlay:

```sh
aibox config create openai
aibox config edit openai
aibox config edit openai --auth
aibox config apply openai
aibox config --agent claude -p work create anthropic
aibox config --agent claude -p work apply anthropic
```

Runs use the active files left by `config apply`; they do not reapply or mount
provider metadata.

Apply is cumulative, not a clean provider switch. Each apply merges the
provider's main config into the current active main config, so keys introduced
by an earlier provider remain until they are removed explicitly with
`aibox.config.apply.remove` or restored from a backup. Codex `auth.json` is the
exception and is replaced as a whole. The `*` shown by `config list` records the
last successful apply; it does not mean the active files contain only that
provider's settings.

Codex providers manage `config.toml` and `auth.json`; Claude providers manage
`settings.json`. New providers are editable templates. Replace placeholder
credentials before applying. `config edit` uses `$VISUAL`, then `$EDITOR`, and
falls back to `vim`. `config get` may print credentials; treat its output as
secret.

Objects and TOML tables merge recursively; scalars and arrays are replaced. To
remove keys during apply, use dotted paths in `aibox.config.apply.remove`:

```toml
[aibox.config.apply]
remove = ["model_provider", "model_providers.custom"]
```

For Claude JSON, use the equivalent nested object. The reserved top-level
`aibox` metadata is stripped from active output. Before replacing active
managed files, apply copies the existing ones into the host-only management
directory and retains the latest 20 generated backups. An initial apply with no
active managed files does not create an empty backup.

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

```sh
aibox config get openai
aibox config delete openai --yes
aibox config delete --all --yes
```

Deleting a provider does not roll back configuration already applied. Omitting
provider names means all providers, and non-interactive deletion requires
`--yes`. Provider create/edit/delete operations are serialized with each other
and cannot overlap `config apply` or profile deletion. They may run while an
agent uses the profile because runs consume only the separately applied active
files. `config apply` and profile creation/deletion refuse to modify a profile
while an aibox run is using it; stop that run and retry.

There is no backup-restore command. To restore a backup, stop runs using the
profile and copy its managed files from
`$AIBOX_ROOT/<profile>/config/<agent>/.backup/<timestamp>/` into the profile's
`home/.codex/` or `home/.claude/` directory. For `-p host`, restore into the
real host agent directory. Keep Codex `auth.json` readable only by its owner.

## Sessions

Sessions are browsed host-side; no container or provider is needed:

```sh
aibox session
aibox session get 3f2a
aibox session delete 3f2a
aibox session delete --all --yes
aibox session -p host list
```

`list` prints each session's short id, start time, and title. `get` accepts a
full id or unique prefix. Deletion removes transcripts only and does not create
backups. Omitting ids means all sessions. Deletion asks before each transcript
unless `--yes` is set, and non-interactive deletion requires it.

Sessions without a recognized typed prompt still appear so bulk deletion can
find every transcript. If discovery is only partially readable, `list` prints
the readable rows and reports the errors, while `get` and `delete` abort rather
than operate on a partial view. Transcript discovery does not follow symlinked
homes, agent state directories, transcript roots, or transcript files.

## Run Options

Options are scoped to the command they affect. Put run options before any
subcommand; put `config` and `session` options after `config` or `session`.

```text
--agent <codex|claude>      agent selector for a run, default: codex
-p, --profile <name>        run profile home, default: default
-w, --work <dir>            directory mounted at /work, default: current dir
-m, --mount <spec>          extra bind mount host:container[:ro], repeatable
```

`config` and `session` accept their own `--agent` and `--profile` options after
the command name.

New provider templates put the selected agent in unrestricted permission mode
because Docker is the sandbox boundary. Edit and reapply the provider, or edit
the active agent config, to restore agent-level prompts or sandboxing.

Within `$AIBOX_ROOT`, mount sources are allowed only beneath an ordinary
profile's `home`; provider metadata and `host` remain host-only. Mounting a
different profile's home is allowed, but can expose that profile's credentials.

For example, select another project and expose reference material read-only:

```sh
aibox -w ../other-project -m ../reference:/reference:ro
```

Relative sources resolve from the launch directory. Mount targets must be
absolute, sources must already exist, and `:ro` is the only accepted mode.
Because aibox uses Docker's short `-v` syntax, source paths containing `:` are
rejected. Extra mounts may be nested beneath `/work` or `/home/aibox`, but may
not replace either managed mount or an ancestor.

## Sandbox Boundary

Each run drops Linux capabilities, enables `no-new-privileges`, and mounts only
the selected profile home, `/work`, and explicit extras. On Linux, the
container uses the invoking uid and gid so project files keep host ownership,
and maps `host.docker.internal` to Docker's host gateway.

Mounts are writable unless marked `:ro`. Networking is enabled, and aibox adds
no CPU, memory, or process limits. Credentials and extra mounts can authorize
effects outside the filesystem boundary; treat each as an explicit grant.

The wrapper handles SIGINT, SIGTERM, and non-ignored SIGHUP by stopping the
active container through Docker. Uncatchable termination such as SIGKILL, a
wrapper crash, or a host failure cannot run that cleanup; after such an event,
check Docker for a leftover container.

## Building Images

```sh
aibox build
aibox build --force
```

The shared image contains both agents. Use `--force` to ignore Docker cache and
pull a fresh Debian base. Set `AIBOX_IMAGE` to make both `build` and agent runs
use a different image tag; a normal run still requires that image to exist
locally.

An image selected with `AIBOX_IMAGE` must provide the selected `codex` or
`claude` binary on `PATH`, use `/home/aibox` as `HOME`, support `/work` as the
working directory, and avoid an incompatible `ENTRYPOINT`. On Linux, required
image files must be readable by an arbitrary uid because aibox runs the
container as the invoking host user.

The Debian-based image includes common Unix development and diagnostic tools,
Python with pip/venv/uv, and Node.js with npm. Rust and Go can be installed into
a persisted profile home as described in [Profile-local
Rust](#profile-local-rust) and [Profile-local Go](#profile-local-go). The
[embedded Dockerfile](assets/aibox.Dockerfile) is the source of truth for the
installed package list and pinned version defaults.

## License

[BSD 2-Clause](LICENSE)
