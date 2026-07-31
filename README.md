# aibox

**Put AI in a Box.** Run OpenAI Codex or Claude Code with Docker as the
Filesystem Sandbox while keeping sign-in, settings, Sessions, and toolchains
persistent in named Tenants.

## Why aibox

- **One runtime for Codex and Claude.** Codex is the default; select Claude
  with `--agent claude`.
- **Persistent identities.** Each Managed Tenant has one isolated Home shared
  by both Coding Agents across Runs.
- **Explicit host access.** A Run sees its Workspace, Tenant Home, and only the
  Extra Mounts supplied on that command.
- **Native configuration.** Runs use the Coding Agent's real configuration
  files. Providers are activated explicitly and never reapplied by a Run.

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

From a Workspace directory, start Codex or Claude and follow the Coding
Agent's sign-in flow:

```sh
aibox run
aibox run --agent claude
```

Pass prompts or native Coding Agent arguments after the hard `--` boundary:

```sh
aibox run -- "inspect this repository and run the tests"
aibox run -- exec "fix the failing tests"
aibox run --agent claude -- "review the current changes"
```

aibox parses only the left side and forwards the right side unchanged. Use
`aibox --help` and `aibox <command> --help` for the complete CLI reference.

## Filesystem Boundary

Each Run creates a disposable container with these possible bind mounts:

| Host source | Container path | Access |
| --- | --- | --- |
| Current directory or `--workspace <dir>` | `/workspace` | Read-write |
| Selected Tenant Home | `/home/aibox` | Read-write |
| Each `--mount host:container[:ro]` | Explicit path | Read-write or `:ro` |

The Filesystem Sandbox is not a complete authority boundary. Networking is
enabled; credentials can authorize remote actions; writable mounts can be
changed or deleted; and aibox adds no CPU or memory limits. The default Provider
templates disable agent-level approval prompts because Docker is the Filesystem
Sandbox. Review or edit native Agent Configuration before activation when a
more restrictive policy is required.

See [Sandbox and Mounts](docs/sandbox.md) for mount validation and container
cleanup behavior.

## Tenants

The Managed Tenant `default` is initialized by the first successful Run. Use a
different Tenant when work should not share credentials, settings, or Sessions:

```sh
aibox tenant create work
aibox run --tenant work
aibox tenant list
aibox tenant delete work
```

A Managed Tenant named `host` is ordinary and runnable. The real host Home is
the separate Host Tenant, selected only by `--host` on Provider and Session
commands. Read [Tenants](docs/tenants.md) before deleting data or sharing
toolchains.

## Components

Install optional status lines or Tenant-local toolchains without changing the
Tenant baseline:

```sh
aibox component list
aibox component install claude-statusline
aibox component install codex-statusline
aibox component install rust
aibox component install go@1.25.6 --tenant work
```

Omitting a Rust or Go version installs the current stable release. Toolchain
installation uses the shared Docker image and requires `aibox build`; status
lines are merged directly into native Agent Configuration. See
[Tenant Components](docs/tenants.md#tenant-components) for replacement and
Provider interaction semantics.

## Providers

A Provider belongs to exactly one Tenant and one Coding Agent. Create its
connection settings, activate it, then inspect later source or working changes:

```sh
aibox provider create custom
aibox provider edit custom
aibox provider edit custom --auth
aibox provider activate custom
aibox provider status
aibox provider diff
```

The Coding Agent or user may continue editing native Agent Configuration after
activation. `provider reconcile` three-way merges those working changes with
Provider source changes. `provider deactivate` restores the exact
pre-activation configuration. A Run does not mutate or reapply Provider
configuration.

State-changing Provider commands are resumable: an interrupted operation is
recorded and completed by the next Provider command, or by the next Run for that
Managed Tenant. There is no Provider backup or restore command.

Provider main configuration is displayed by `provider get`; credential output
requires `provider get --auth`. Read [Providers](docs/providers.md) for conflict
resolution, Host Tenant usage, credentials, and failure recovery.

## Sessions

Session browsing is host-side and does not start Docker:

```sh
aibox session
aibox session get 3f2a
aibox session --host --agent claude list
```

Session deletion requires explicit ids or `--all` and is irreversible.

## Extra Mounts

Expose reference material read-only while working in another Workspace:

```sh
aibox run -w ../other-project -m ../reference:/reference:ro
```

Every Extra Mount is an explicit authority grant. Read the
[mount rules](docs/sandbox.md#mount-rules) before exposing credentials or
another Tenant Home.

## Shell Completion

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

- [Tenants](docs/tenants.md): persistent state, Host Tenant, layout, deletion,
  and Components.
- [Providers](docs/providers.md): activation, reconciliation, secrets, and
  resumable transactions.
- [Sandbox and Mounts](docs/sandbox.md): mount rules, security boundary,
  cleanup, and custom images.
- [Embedded Dockerfile](assets/aibox.Dockerfile): installed packages and pinned
  Coding Agent versions.

## License

[BSD 2-Clause](LICENSE)
