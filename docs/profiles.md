# Profiles

Profiles give each work environment a persistent container home. Agent
credentials, settings, sessions, caches, and optional language toolchains stay
available across runs without being shared with other profiles.

## Everyday Use

The default profile is named `default`. Select another profile with `--profile`:

```sh
aibox run --profile work
aibox run --agent claude --profile work
```

A normal run creates the profile if it does not exist. You can also manage
profiles explicitly:

```sh
aibox profile create work
aibox profile list
aibox profile delete work
aibox profile delete --all
```

Deletion covers both agents' credentials, settings, sessions, caches,
providers, and backups. It does not delete the shared Docker image. aibox asks
before each deletion; scripts and other non-interactive callers must use
`--yes`. There is no undo.

## Stored Data

The default aibox root is `$HOME/.aibox`. Set `AIBOX_ROOT` to use another
location. A relative value is resolved from the directory where aibox was
launched.

An ordinary profile uses this layout:

| Path | Purpose |
| --- | --- |
| `<profile>/home/` | Mounted at `/home/aibox` during a run |
| `home/.codex/` | Codex credentials, settings, and sessions |
| `home/.claude/` | Claude credentials, settings, and sessions |
| `home/.cargo/`, `home/.rustup/` | Optional Rust installation and caches |
| `home/.goroot/`, `home/.gopath/` | Optional Go SDK, commands, and caches |
| `provider/<agent>/` | Host-only providers and active-config backups |

Only `home/` is container-visible. Provider snapshots, backups, and other
management data stay on the host.

Profile and provider names may contain letters, numbers, `_`, and `-`.

## Profile Initialization

Creating a profile, starting a run, or creating its first provider initializes
both agent state directories. aibox also installs these files when they are
missing:

- `~/.claude/statusline.sh`, used by the built-in Claude provider template.
- `~/.gitconfig`, which rewrites common GitHub SSH clone URLs to HTTPS.

Existing regular files are preserved. Profiles do not inherit the host's Git
identity, SSH keys, or credential helpers. Configure them inside the profile,
or mount the exact files you need and treat that mount as an explicit authority
grant.

## The host Profile

`host` is a built-in management profile for working with the real host agent
directories:

```sh
aibox provider --profile host list
aibox session --profile host list
aibox session --agent claude --profile host list
```

For this profile, provider and session commands use the real `$HOME/.codex` or
`$HOME/.claude`. Provider snapshots and backups still live beneath
`$AIBOX_ROOT/host/provider/<agent>/`.

This is not an isolated copy:

- `aibox provider apply --profile host` changes the active host agent configuration.
- `aibox session delete --profile host` deletes real host transcripts without a backup.
- Docker runs reject `--profile host`, and `profile delete` cannot delete it.

Applying a Claude provider to `host` also installs and may enable the bundled
status-line script in the real `$HOME/.claude`. On the host, that script expects
Bash and `jq`; Git branch detection is optional.

## Profile-Local Toolchains

The shared image includes Python and Node.js, but not Rust or Go. Their
locations are already configured so an installation can persist in the
selected profile:

- Rust binaries and caches use `$HOME/.cargo`; toolchains use
  `$HOME/.rustup`. Install with [rustup](https://rustup.rs/).
- Go uses `$HOME/.goroot` for the SDK and `$HOME/.gopath` for installed
  commands, modules, and build caches. Follow the official
  [Go installation guide](https://go.dev/doc/install) and install the SDK into
  `$HOME/.goroot`.

The corresponding binary directories are already on `PATH`. Install a
toolchain once per profile, either through the agent or with `docker exec`
while that profile's container is running.

See [Sandbox and Mounts](sandbox.md) before sharing toolchains or credentials
between profiles with extra mounts.
