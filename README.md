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
  files. Named Configs are applied explicitly, observed for drift, and never
  reapplied by a Run.
- **Local Console.** Manage the Runtime Image, Tenants, Components, Configs,
  Sessions, and Requests from one foreground Service.

## Quick Start

aibox supports Linux and macOS hosts with a working Docker CLI and daemon.
Building the Rust wrapper requires Rust 1.97 or newer. The bundled image
supports Linux `amd64` and `arm64`.

```sh
git clone https://github.com/rbee3u/aibox.git
cd aibox
cargo install --locked --path .
aibox console
```

Open `http://127.0.0.1:9923/`. Build the Runtime Image from Overview, then keep
the Service running while using the Console or Request Proxy. In Tenants, open
the `default` Tenant's Components and install Codex, Claude, or both before the
first corresponding Run.

From another terminal in a Workspace directory, start Codex or Claude and
follow the Coding Agent's sign-in flow:

```sh
aibox run
aibox run --agent claude
```

Open a Debug Shell when you need to inspect a Tenant Home or its Component
environment without starting a Coding Agent:

```sh
aibox debug
aibox debug --tenant work
```

Pass prompts or native Coding Agent arguments after the hard `--` boundary:

```sh
aibox run -- "inspect this repository and run the tests"
aibox run -- exec "fix the failing tests"
aibox run --agent claude -- "review the current changes"
```

aibox parses only the left side and forwards the right side unchanged to
`run`. The public CLI is `aibox console [--listen IP:PORT]`, `aibox run`, and
`aibox debug [--tenant TENANT]`. Runtime Image, Tenant, Component, Config, and
Session management lives in the Console.

## Filesystem Boundary

Each Run creates a disposable container with these possible bind mounts:

| Host source | Container path | Access |
| --- | --- | --- |
| Current directory or `--workspace <dir>` | `/workspace` | Read-write |
| Selected Tenant Home | `/home/aibox` | Read-write |
| Each `--mount host:container[:ro]` | Explicit path | Read-write or `:ro` |

A Debug Shell mounts only its selected Tenant Home and starts in
`/home/aibox`; it does not mount a Workspace or accept Extra Mounts.

The Filesystem Sandbox is not a complete authority boundary. Networking is
enabled; credentials can authorize remote actions; writable mounts can be
changed or deleted; and aibox adds no CPU or memory limits. The built-in Named
Config templates created in the Console disable Coding Agent approval
prompts because Docker is the Filesystem Sandbox. Named Configs are never
created or applied automatically. Review the template before applying it when
a more restrictive policy is required.

See [Sandbox and Mounts](docs/sandbox.md) for mount validation and container
cleanup behavior.

## Tenants

`aibox console` creates or repairs the protected Default Managed Tenant baseline
before it starts listening. Running without a Service retains the same fallback:
the first validated Run or Debug Shell can initialize `default` after the
Runtime Image preflight, even if Docker or the invoked process later fails. Use
a different Tenant when work should not share credentials, settings, or
Sessions:

```sh
aibox run --tenant work
```

The first validated Run or Debug Shell can initialize `work`, or you can create
it explicitly from the Console's Tenants module.

Tenant Homes, Named Configs, and Requests persist under `$HOME/.aibox`.
`AIBOX_ROOT` selects another location, which must be a directory dedicated to
aibox because Tenant deletion removes subtrees from it.

A Managed Tenant named `host` is ordinary and runnable. Create, inspect, and
delete other Managed Tenants from the Console's Tenants module; `default` is
protected from deletion. The real host Home is the separate Host Tenant. Read
[Tenants](docs/tenants.md) before deleting data or sharing toolchains.

## Components

Install or update Tenant-local Coding Agents, Node.js, Python, language
toolchains, and optional statuslines from a Tenant's Components view.

Omitting a version installs the current stable release; an exact `X.Y.Z`
selects that release. Runtime installation uses the shared Docker image and
requires a built Runtime Image, but upgrades do not rebuild it. Codex and
Claude use their official standalone/native installers and do not depend on
Node.js. The Python toolchain bundles uv/uvx, one active CPython, pip, and venv;
it is independent from Node, Rust, and Go. Statuslines directly edit native
Current Config values; Host statusline Components are available through the
Host Tenant, while runtime and toolchain Components remain Managed Tenant-only.
See [Tenant Components](docs/tenants.md#tenant-components) for lifecycle and
replacement semantics.

## Configs

A Named Config belongs to exactly one Tenant and one Coding Agent. The Configs
module can create, reveal, edit, delete, and explicitly apply it, as well as
edit Current Config and preview Credential Propagation.

Application overwrites or removes every fixed Config Field and preserves
unrelated native settings such as statusline configuration. A successful
application records Last Application so the Console can derive Config Drift;
this is not activation and never triggers reapplication. No backup or rollback
state is retained. Reveal displays every native file, including credentials
without redaction. When Host Codex refreshes a ChatGPT
login, the Console Configs module explicitly copies that newer credential
snapshot to older same-account existing Configs without creating a persistent
relationship.
Read [Configs](docs/configs.md) for the exact schema, Current Config behavior,
Host Tenant risks, file modes, and partial-write behavior.

## Sessions

Session browsing in the Console is host-side, progressively streams a
Conversation Message/Tool Activity projection, and does not start Docker.

Canonical UUIDs are listed by their final 12 characters; `get` and `delete`
accept a full Session id or unique suffix. Session deletion requires explicit
ids or `--all` and is irreversible.
Session discovery and the Transcript projection are best-effort, but
destructive operations refuse an incomplete filesystem view. The detail view
preserves diagnostic Transcript Evidence, hides internal reasoning, and reads
full native entries only on demand. See
[Sessions](docs/tenants.md#sessions) for parsing warnings and traversal safety.

## Extra Mounts

Expose reference material read-only while working in another Workspace:

```sh
aibox run -w ../other-project -m ../reference:/reference:ro
```

Every Extra Mount is an explicit authority grant. Read the
[mount rules](docs/sandbox.md#mount-rules) before exposing credentials or
another Tenant Home.

## Request Debugging

Start the foreground Service, then open the Requests module at
`http://127.0.0.1:9923/_aibox/ui/requests`:

```sh
aibox console
```

The Service prints its listener and Console address. Requests persist
under `$AIBOX_ROOT/requests/` (`$HOME/.aibox/requests/` by default).

Request storage format v4 does not read or migrate format v3. Before the first
start of an upgraded Service, stop the old Service, optionally back up the
Requests directory, and manually remove `$AIBOX_ROOT/requests`. The new Service
recreates an empty directory.

Console development commands and the embedded asset workflow are documented in
[Console UI Development](docs/console-ui.md).

Point a model provider at the proxy by placing its complete upstream base URL
after the local address. In Configs, select the Tenant and Codex Current Config,
then edit `config.toml`.

For a custom Codex provider in Current Config:

```toml
model_provider = "custom"

[model_providers.custom]
name = "hezubus"
base_url = "http://host.docker.internal:9923/https://hezubus.ai/v1"
wire_api = "responses"
```

The Visual Configs editor can add the Request Proxy Route prefix while showing
only the upstream URL. Raw Current Config can still use arbitrary provider
names and `wire_api`; these values are outside the fixed Named Config schema.

For Claude, select its Current Config and set the native base URL:

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "http://host.docker.internal:9923/https://api.anthropic.com"
  }
}
```

Docker Desktop supplies `host.docker.internal`. Native Linux Docker usually
needs `aibox console --listen 0.0.0.0:9923`. Reachable clients may use the
Request Proxy, while Console and Control API routes still require a loopback
TCP peer. See
[Request Proxy](docs/sandbox.md#request-proxy) for the complete behavior.

## CLI Surface

`aibox console` starts the foreground Service and embedded Console. `aibox run`
starts a transient Coding Agent Run. `aibox debug [--tenant TENANT]` starts a
transient Tenant-only Debug Shell. Runtime Image construction and persistent
lifecycle management remain in the Console.

## Learn More

- [Domain Language](CONTEXT.md): the canonical terms used by code, help, and
  documentation.
- [Tenants](docs/tenants.md): persistent state, Host Tenant, layout, Sessions,
  deletion, and Components.
- [Configs](docs/configs.md): Named and Current Configs, fixed fields,
  credentials, one-time application, and filesystem behavior.
- [Sandbox and Mounts](docs/sandbox.md): mount rules, security boundary,
  cleanup, Request Proxy behavior, and the fixed Runtime Image.
- [Embedded Dockerfile](assets/aibox.Dockerfile): the shared base packages in
  the fixed Runtime Image.

## Development

Before changing behavior, read the repository constraints in
[AGENTS.md](AGENTS.md), the relevant domain definitions in
[CONTEXT.md](CONTEXT.md), and the [ADR index](docs/adr/README.md). AGENTS.md also
maps behavior to its owning modules. Run the complete Rust checks with:

```sh
make check
```

Changes under `console/` also require `make console-check`; install its
locked dependencies first with `make console-ci`. See
[Console UI Development](docs/console-ui.md) for the generated-asset and
optional browser-test workflow.

## License

[BSD 2-Clause](LICENSE)
