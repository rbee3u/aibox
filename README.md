# AIBox

**Put AI in a Box.** Run OpenAI Codex or Claude Code in a Docker Filesystem
Sandbox while keeping sign-in, settings, Sessions, and toolchains in persistent
Tenants.

## Why AIBox

- Run Codex or Claude in the same disposable container environment.
- Keep each Managed Tenant's credentials, settings, and tools isolated.
- Expose only the Workspace, Tenant Home, and explicitly requested mounts.
- Apply reusable Configs to the Agents' native files without hiding them behind
  a proprietary format.
- Manage Tenants, Components, Configs, Sessions, the Runtime Image, and recorded
  Requests from a local Console.

## Quick Start

AIBox supports Linux and macOS hosts with Docker. Building the Rust wrapper
requires Rust 1.97 or newer; the Runtime Image supports Linux `amd64` and
`arm64`.

```sh
git clone https://github.com/rbee3u/aibox.git
cd aibox
cargo install --locked --path .
aibox console
```

Open `http://127.0.0.1:9923/`. From Overview, build the Runtime Image. Then
open the `default` Tenant's Components and install Codex, Claude, or both.

From a Workspace directory, start an Agent and follow its native sign-in flow:

```sh
aibox run
aibox run --agent claude
```

Open a Tenant-only Debug Shell without mounting a Workspace:

```sh
aibox debug
aibox debug --tenant work
```

Pass prompts or native Agent arguments after `--`:

```sh
aibox run -- "inspect this repository and run the tests"
aibox run -- exec "fix the failing tests"
aibox run --agent claude -- "review the current changes"
```

AIBox parses only the left side and forwards the right side unchanged to
`run`. Its public commands are:

```text
aibox console [--listen IP:PORT]
aibox run [AIBox OPTIONS] -- [AGENT ARGUMENTS]
aibox debug [--tenant TENANT]
```

Runtime Image, Tenant, Component, Config, Session, and Request management lives
in the Console.

## Security and Persistent State

Each Run mounts a Workspace at `/workspace` and the selected Tenant Home at
`/home/aibox`. Extra Mounts explicitly grant access to another host path:

```sh
aibox run -w ../other-project -m ../reference:/reference:ro
```

The container is a **Filesystem** Sandbox, not a complete authority boundary.
Networking remains enabled, credentials can authorize remote actions, writable
mounts can be changed or deleted, and AIBox adds no resource limits. Built-in
Config templates disable native Agent approval prompts; review them before
applying if you need a more restrictive policy. See the complete
[mount and sandbox rules](docs/sandbox.md).

Managed Tenant Homes, Named Configs, and recorded Requests persist under
`$HOME/.aibox` by default. `AIBOX_ROOT` selects another location; dedicate that
directory to AIBox because lifecycle operations remove selected subtrees.
Request recording preserves raw headers and bodies, which can include API keys,
prompts, tool data, and model output. AIBox provides no automatic redaction or
retention policy.

## Learn More

- [Domain Language](CONTEXT.md): canonical terms used throughout the project.
- [Tenants, Sessions, and Components](docs/tenants.md): identity, lifecycle,
  Components, Transcripts, and Tenant Environment.
- [Configs](docs/configs.md): Named and Current Configs, application, drift, and
  Credential Propagation.
- [Filesystem Sandbox and Mounts](docs/sandbox.md): mounts, cleanup, Runtime
  Image, and Request Proxy behavior.
- [Console UI Development](docs/console-ui.md): frontend architecture, tests,
  generated assets, and interaction contracts.

## Development

Before changing behavior, read [AGENTS.md](AGENTS.md),
[CONTEXT.md](CONTEXT.md), and the [ADR index](docs/adr/README.md). Install the
locked Console dependencies once per environment, then run:

```sh
make console-ci
make check
```

Use `make help` for focused commands.

## License

[BSD 2-Clause](LICENSE)
