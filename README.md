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
  files. Named Configs are applied explicitly and never reapplied by a Run.

## Quick Start

aibox supports Linux and macOS hosts with a working Docker CLI and daemon.
Building the Rust wrapper requires Rust 1.97 or newer. The bundled image
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
changed or deleted; and aibox adds no CPU or memory limits. The built-in Named
Config templates created by `config create` disable Coding Agent approval
prompts because Docker is the Filesystem Sandbox. Named Configs are never
created or applied automatically. Review the template before applying it when
a more restrictive policy is required.

See [Sandbox and Mounts](docs/sandbox.md) for mount validation and container
cleanup behavior.

## Tenants

The Managed Tenant `default` is initialized by the first Run attempt that passes
mount validation and finds the image, even if Docker later fails or the Coding
Agent exits nonzero. Use a different Tenant when work should not share
credentials, settings, or Sessions:

```sh
aibox tenant create work
aibox run --tenant work
aibox tenant list
aibox tenant delete work
```

Tenant Homes, Named Configs, and Traffic Records persist under `$HOME/.aibox`.
`AIBOX_ROOT` selects another location, which must be a directory dedicated to
aibox because Tenant deletion removes subtrees from it.

A Managed Tenant named `host` is ordinary and runnable. The real host Home is
the separate Host Tenant, selected only by `--host` on `config`, `session`, and
`component` commands. Read [Tenants](docs/tenants.md) before deleting data or
sharing toolchains.

## Components

Install optional status lines or Tenant-local toolchains without changing the
Tenant baseline:

```sh
aibox component list
aibox component install claude-statusline
aibox component install codex-statusline
aibox component --host list
aibox component --host install claude-statusline
aibox component install rust
aibox component install go@1.25.6 --tenant work
aibox component remove rust --tenant work --yes
```

Omitting a Rust or Go version installs the current stable release. Toolchain
installation uses the shared Docker image and requires `aibox build`; status
lines directly edit their native Current Config values; Host statusline
Components are available through `--host`, while Rust and Go remain Managed
Tenant-only. See
[Tenant Components](docs/tenants.md#tenant-components) for lifecycle and
replacement semantics.

## Configs

A Named Config belongs to exactly one Tenant and one Coding Agent. It accepts
a fixed set of native settings and credentials and applies them once:

```sh
aibox config create custom
aibox config get custom
aibox config edit custom
aibox config apply custom
aibox config get --current
aibox config edit --current
aibox config propagate-auth
```

Application overwrites or removes every fixed Config Field and preserves
unrelated native settings such as status-line configuration. It records no
association, backup, or rollback state. `get` displays every native file,
including credentials without redaction; `edit` opens and commits them one at a
time. After a successful interactive Named Config edit, aibox offers to apply
it to the selected Current Config; the default is No, and the standalone
`config apply` command remains available. When Host Codex refreshes a ChatGPT
login, `propagate-auth` explicitly copies that newer credential snapshot to
older same-account existing Configs without creating a persistent relationship.
Read [Configs](docs/configs.md) for the exact schema, Current Config behavior,
Host Tenant risks, file modes, and partial-write behavior.

## Sessions

Session browsing is host-side and does not start Docker:

```sh
aibox session
aibox session get 458cbf92d123
aibox session --host --agent claude list
```

Canonical UUIDs are listed by their final 12 characters; `get` and `delete`
accept a full Session id or unique suffix. Session deletion requires explicit
ids or `--all` and is irreversible.
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
the Traffic Viewer at `http://127.0.0.1:9923/`:

```sh
aibox traffic
```

The command prints its Listen and Viewer addresses followed by concise,
safety-filtered runtime diagnostics. Traffic Records persist under
`$AIBOX_ROOT/traffic/` (`$HOME/.aibox/traffic/` by default).

Traffic Viewer development commands and the embedded asset workflow are
documented in [Traffic UI Development](docs/traffic-ui.md).

Point a model provider at the proxy by placing its complete upstream base URL
after the local address. For Codex, edit the Current Config for the Tenant that
will send traffic (add `--tenant <name>` when needed):

```sh
aibox config edit --current
```

For Codex's built-in OpenAI provider inside an aibox container, remove any
custom `model_provider` selection and set:

```toml
openai_base_url = "http://host.docker.internal:9923/https://api.openai.com/v1"
```

For a custom Codex provider:

```toml
model_provider = "hezubus"

[model_providers.hezubus]
name = "hezubus"
base_url = "http://host.docker.internal:9923/https://hezubus.ai/v1"
wire_api = "responses"
```

That provider block is native Current Config, not the fixed Named Config
schema: arbitrary provider names and `wire_api` cannot be stored verbatim in a
Named Config.

For Claude, edit its Current Config for the selected Tenant:

```sh
aibox config --agent claude edit --current
```

Then set the native base URL:

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "http://host.docker.internal:9923/https://api.anthropic.com"
  }
}
```

Docker Desktop supplies `host.docker.internal`. Native Linux Docker usually
needs `aibox traffic --listen 0.0.0.0:9923`. The selected address serves both
the Traffic Proxy and Traffic Viewer. See
[Traffic Proxy](docs/sandbox.md#traffic-proxy) for the complete behavior.

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
Managed Tenants, Named Configs, Sessions, and the fixed Component catalog.
Discovery is host-side and read-only: it does not create a missing Managed
Tenant or modify Current Config.

## Learn More

- [Domain Language](CONTEXT.md): the canonical terms used by code, help, and
  documentation.
- [Tenants](docs/tenants.md): persistent state, Host Tenant, layout, Sessions,
  deletion, and Components.
- [Configs](docs/configs.md): Named and Current Configs, fixed fields,
  credentials, one-time application, and filesystem behavior.
- [Sandbox and Mounts](docs/sandbox.md): mount rules, security boundary,
  cleanup, Traffic Proxy behavior, and custom images.
- [Embedded Dockerfile](assets/aibox.Dockerfile): installed packages and pinned
  Coding Agent versions.

## Development

Before changing behavior, read the repository constraints in
[AGENTS.md](AGENTS.md), the relevant domain definitions in
[CONTEXT.md](CONTEXT.md), and the [ADR index](docs/adr/README.md). AGENTS.md also
maps behavior to its owning modules. Run the complete Rust checks with:

```sh
make check
```

Changes under `web/traffic/` also require `make traffic-check`; install its
locked dependencies first with `make traffic-ci`. See
[Traffic UI Development](docs/traffic-ui.md) for the generated-asset and
optional browser-test workflow.

## License

[BSD 2-Clause](LICENSE)
