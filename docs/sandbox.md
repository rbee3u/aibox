# Filesystem Sandbox and Mounts

aibox treats the Docker container as the Coding Agent's Filesystem Sandbox. It
limits which host paths enter the container while leaving the agent free to
work inside those mounts.

This Filesystem Sandbox is not a complete security boundary. Networking remains
enabled, credentials can authorize remote effects, and Docker still relies on
the host daemon and platform.

## Run in Another Workspace

The current directory is the default Workspace and is mounted at `/workspace`.
Select another existing directory with `--workspace`:

```sh
aibox run -w ../other-project
```

Relative paths are resolved from the directory where aibox was launched.

## Mount Rules

Add existing host files or directories with Docker-style short mount syntax:

```sh
aibox run -m ../reference:/reference:ro
aibox run -m ./cache:/cache
```

The accepted form is `host:container[:ro]`:

- The source must already exist. Relative sources resolve from the launch
  directory.
- The container target must be absolute.
- Mounts are writable by default; `:ro` is the only accepted mode.
- Host paths containing `:` are rejected because Docker's short `-v` syntax
  cannot represent them safely.
- Extra mounts may be nested beneath `/workspace` or `/home/aibox`, but may not
  replace either managed mount or one of its ancestors.

Within `$AIBOX_ROOT`, only `tenants/<tenant>` or one of its descendants may be
a bind source. Provider catalogs, Agent/Tenant metadata, internal lifecycle
staging directories, and Host Tenant metadata stay host-only.

Mounting another Tenant Home is allowed, but doing so exposes its Coding
Agent credentials and persistent state. Treat every Extra Mount as an explicit
authority grant.

## Runtime Boundary

Each Docker run:

- drops all Linux capabilities;
- enables `no-new-privileges`;
- mounts the selected Tenant Home at `/home/aibox`;
- mounts the selected Workspace at `/workspace`;
- adds only the extra mounts supplied on the command line.

Rust and Go Component installation also uses a disposable, cleanup-aware
container, but mounts only the selected Tenant Home at `/home/aibox`; it does
not mount a Workspace or accept Extra Mounts. The installer retains normal
network access to official toolchain distribution services.

On Linux, the container runs with the invoking uid and gid so Workspace files
retain host ownership. aibox also maps `host.docker.internal` to Docker's host
gateway. Docker Desktop provides the host integration on macOS.

The following remain outside the Filesystem Sandbox:

- Container networking is enabled.
- Credentials may authorize changes to repositories, APIs, cloud accounts, or
  other remote systems.
- aibox adds no CPU, memory, or process-count limits.
- Writable Workspace, Tenant Home, and Extra Mounts can be modified or
  deleted by the Coding Agent.

The default Codex Provider sets `approval_policy = "never"` and
`sandbox_mode = "danger-full-access"`; Docker remains its Filesystem Sandbox.
The default Claude Provider uses `bypassPermissions` and suppresses its
dangerous-mode prompt. Native Agent settings may grant authority beyond the
Filesystem Sandbox, especially through mounted credentials or network services,
and remain the user's responsibility.

## Cleanup

Runs use disposable Docker containers. aibox tracks the Docker child and
container id, and keeps cleanup armed until it has checked that the container
did not outlive the Docker client.

The wrapper handles SIGINT, SIGTERM, and non-ignored SIGHUP by stopping the
active container through Docker. SIGKILL, a wrapper crash, Docker failure, or a
host failure cannot guarantee cleanup. After such an event, inspect Docker for
a leftover container before starting sensitive work.

One aibox process supports one active Run at a time.

## Building the Shared Image

Build the bundled image after installing aibox:

```sh
aibox build
aibox build --force
```

The image contains both Codex and Claude. `--force` disables Docker's build
cache and pulls a fresh Debian base. The build uses an embedded, context-free
[Dockerfile](../assets/aibox.Dockerfile), which is the source of truth for
installed packages and pinned agent versions.

The image includes common Unix development and diagnostic tools, Python with
pip/venv/uv, and Node.js with npm. Rust and Go are installed on demand into a
persistent Managed Tenant; see [Tenant Components](tenants.md#tenant-components).

## Custom Images

Set `AIBOX_IMAGE` to make both image builds and agent runs use another image
tag:

```sh
AIBOX_IMAGE=local/aibox:dev aibox build
AIBOX_IMAGE=local/aibox:dev aibox run
```

A normal run still requires the selected image to exist locally. A replacement
image must:

- provide the selected `codex` or `claude` executable on `PATH`;
- use `/home/aibox` as `HOME`;
- support `/workspace` as its working directory;
- avoid an incompatible `ENTRYPOINT`.

Using that image for Rust or Go Component installation additionally requires
Bash, curl, Python 3 with `tomllib`, `dpkg`, tar, and SHA-256 utilities.

On Linux, aibox overrides the image user with the invoking host uid and gid.
Executables and required image files must therefore be readable and executable
by an arbitrary uid.
