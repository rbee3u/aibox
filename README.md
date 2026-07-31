# aibox

**Put AI in a Box.** Run OpenAI Codex or Claude Code with Docker as the
sandbox boundary, so you can choose fewer agent-level permission prompts
without giving the agent unrestricted access to your host.

## Why aibox

- **One runtime for Codex and Claude.** Codex is the default; switch agents with
  one flag.
- **Persistent, isolated profiles.** Credentials, settings, sessions, caches,
  and optional toolchains survive container runs without sharing between
  profiles.
- **Predictable host access.** The agent sees the project, its profile home,
  and only the extra mounts you explicitly add. On Linux, generated project
  files keep your uid and gid.
- **Host-side management.** Inspect sessions and prepare provider configuration
  without starting Docker or exposing management data to a container.

## Quick Start

aibox supports Linux and macOS hosts with a working Docker CLI and daemon.
Building the Rust wrapper requires Rust 1.85 or newer. The bundled image
supports Linux `amd64` and `arm64`.

```sh
git clone https://github.com/rbee3u/aibox.git
cd aibox
cargo install --locked --path .
aibox build
```

The first image build downloads the development runtimes and pinned agent CLI
versions, so it can take a while. Normal runs never build the image
automatically.

From a project directory, start Codex or Claude and follow the agent's sign-in
flow:

```sh
aibox run
aibox run --agent claude
```

Pass a prompt or any agent-specific arguments after `--`:

```sh
aibox run -- "inspect this repository and run the tests"
aibox run -- exec "fix the failing tests"
aibox run --agent claude -- "review the current changes"
```

The first `--` is a hard boundary. aibox parses options on the left and
forwards everything on the right unchanged to the selected agent. Use
`aibox --help` and `aibox <command> --help` for the complete CLI reference.

## How It Works

Each run creates a container with three possible kinds of host access:

| Host source | Container path | Access |
| --- | --- | --- |
| Current directory or `--work <dir>` | `/work` | Read-write |
| Selected profile home | `/home/aibox` | Read-write |
| Each `--mount host:container[:ro]` | Explicit target | Read-write or `:ro` |

The profile home is what makes agent sign-in, settings, sessions, and caches
persistent. The default profile is `default`; select another with
`aibox run --profile work`.

The container is a filesystem boundary, not a complete authority boundary:

- Networking is enabled.
- The project and profile mounts are writable.
- Credentials can authorize remote actions outside the mounted filesystem.
- Extra mounts expand host access, and aibox adds no CPU or memory limits.

See [Sandbox and Mounts](docs/sandbox.md) for the complete boundary and cleanup
model.

## Common Workflows

### Separate Work Environments

Use profiles to keep agent state and development toolchains apart:

```sh
aibox profile create work
aibox run --profile work
aibox profile list
```

Profiles are also created on the first run. See
[Profiles](docs/profiles.md) before using the special `host` profile or
deleting profile data.

### Configure an API Provider

Provider overlays let you prepare and explicitly apply agent configuration:

```sh
aibox provider create custom
aibox provider edit custom
aibox provider edit custom --auth
aibox provider apply custom
```

New templates contain placeholder credentials and configure the selected
agent for unrestricted operation inside the container. Replace the
placeholders before applying. Apply is persistent and cumulative, not a clean
provider switch. See [Providers](docs/providers.md) for Claude examples, merge
semantics, backups, and restore steps.

### Browse Saved Prompts

Session commands run on the host and do not need Docker:

```sh
aibox session
aibox session get 3f2a
```

Use `aibox session --help` for selection and deletion options. Transcript
deletion has no backup.

### Mount Extra Files

Expose reference material read-only while working in another project:

```sh
aibox run -w ../other-project -m ../reference:/reference:ro
```

Read the [mount rules](docs/sandbox.md#mount-rules) before exposing credentials
or another profile.

### Enable Shell Completion

```sh
# Bash
source <(aibox completion bash)
# Zsh
source <(aibox completion zsh)
# Fish
aibox completion fish | source
```

Add the matching command to your shell startup file.

## Learn More

- [Profiles](docs/profiles.md): persistent state, the `host` profile,
  deletion, and profile-local toolchains.
- [Providers](docs/providers.md): Codex and Claude overlays, cumulative apply,
  secrets, backups, and restore.
- [Sandbox and Mounts](docs/sandbox.md): mount validation, network and
  credential boundaries, cleanup, and custom images.
- [Embedded Dockerfile](assets/aibox.Dockerfile): installed packages and pinned
  agent CLI versions.

## License

[BSD 2-Clause](LICENSE)
