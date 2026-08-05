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
  files. Agent Profiles are applied explicitly and never reapplied by a Run.

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
changed or deleted; and aibox adds no CPU or memory limits. The built-in Agent
Profile templates created by `profile create` disable Coding Agent approval
prompts because Docker is the Filesystem Sandbox. Agent Profiles are never
created or applied automatically. Review the template before applying it when
a more restrictive policy is required.

See [Sandbox and Mounts](docs/sandbox.md) for mount validation and container
cleanup behavior.

## Tenants

The Managed Tenant `default` is initialized by the first validated Run attempt,
even if Docker later fails or the Coding Agent exits nonzero. Use a different
Tenant when work should not share credentials, settings, or Sessions:

```sh
aibox tenant create work
aibox run --tenant work
aibox tenant list
aibox tenant delete work
```

A Managed Tenant named `host` is ordinary and runnable. The real host Home is
the separate Host Tenant, selected only by `--host` on `profile` and `session`
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
aibox component remove rust --tenant work --yes
```

Omitting a Rust or Go version installs the current stable release. Toolchain
installation uses the shared Docker image and requires `aibox build`; status
lines directly edit their native Agent Configuration values. See
[Tenant Components](docs/tenants.md#tenant-components) for lifecycle and
replacement semantics.

## Agent Profiles

An Agent Profile belongs to exactly one Tenant and one Coding Agent. It accepts
a fixed set of native settings and credentials and applies them once:

```sh
aibox profile create custom
aibox profile edit custom
aibox profile edit custom --auth
aibox profile apply custom
```

Application overwrites or removes every fixed Profile Field and preserves
unrelated native settings such as status-line configuration. It records no
active Profile, backup, or rollback state. Read
[Agent Profiles](docs/profiles.md) for the exact schema, credentials, Host
Tenant risks, file modes, and partial-write behavior.

## Sessions

Session browsing is host-side and does not start Docker:

```sh
aibox session
aibox session get 3f2a
aibox session --host --agent claude list
```

Session deletion requires explicit ids or `--all` and is irreversible.
Session discovery and the typed-prompt view are best-effort, but destructive
operations refuse an incomplete filesystem view. See
[Sessions](docs/tenants.md#sessions) for parsing warnings and traversal safety.

## Extra Mounts

Expose reference material read-only while working in another Workspace:

```sh
aibox run -w ../other-project -m ../reference:/reference:ro
```

Every Extra Mount is an explicit authority grant. Read the
[mount rules](docs/sandbox.md#mount-rules) before exposing credentials or
another Tenant Home.

## Traffic Debugging

Start the temporary host-side HTTP/SSE recorder in the foreground, then open
the viewer at `http://127.0.0.1:9923/`:

```sh
aibox traffic
```

Point a model provider at the proxy by placing its complete upstream base URL
after the local address. For Codex inside an aibox container:

```toml
[model_providers.hezubus]
name = "hezubus"
base_url = "http://host.docker.internal:9923/https://hezubus.ai/v1"
wire_api = "responses"
```

For Claude, set the native configuration for the selected Tenant:

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "http://host.docker.internal:9923/https://api.anthropic.com"
  }
}
```

Docker Desktop supplies `host.docker.internal`. Native Linux Docker usually
needs `aibox traffic --listen 0.0.0.0:9923 --allow-remote`; the management page
remains loopback-only. Traffic Records contain unredacted authorization
headers, prompts, and responses. See
[Traffic Proxy](docs/sandbox.md#traffic-proxy) before use and delete Records
from its viewer afterward.

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

Completion evaluates command-aware candidates on demand, including existing
Managed Tenants, Agent Profiles, Sessions, and the fixed Component catalog.
Discovery is host-side and read-only: it does not create a missing Managed
Tenant or modify Agent Configuration.

## Learn More

- [Domain Language](CONTEXT.md): the canonical terms used by code, help, and
  documentation.
- [Tenants](docs/tenants.md): persistent state, Host Tenant, layout, Sessions,
  deletion, and Components.
- [Agent Profiles](docs/profiles.md): fixed fields, one-time application,
  credentials, and filesystem behavior.
- [Sandbox and Mounts](docs/sandbox.md): mount rules, security boundary,
  cleanup, Traffic Proxy behavior, and custom images.
- [Embedded Dockerfile](assets/aibox.Dockerfile): installed packages and pinned
  Coding Agent versions.

## Development

Before changing behavior, read the repository constraints in
[AGENTS.md](AGENTS.md), the relevant domain definitions in
[CONTEXT.md](CONTEXT.md), and the decisions in [docs/adr](docs/adr/). AGENTS.md
also lists the required Rust checks.

## License

[BSD 2-Clause](LICENSE)
