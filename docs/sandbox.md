# Filesystem Sandbox and Mounts

AIBox treats its Docker container as the Agent, Debug Shell, or
Component installer's Filesystem Sandbox. It controls which host paths enter
the container; it does not confine network or credential authority.

## Workspace and Mount Rules

The launch directory is the default Workspace. A Run mounts it at
`/workspace/<directory name>` and uses that path as its working directory.
Select another existing directory with:

```sh
aibox run --workspace ../other-project
```

Relative paths resolve from the launch directory. Extra Mounts use Docker-style
short syntax:

```sh
aibox run --mount ../reference:/reference:ro
aibox run --mount ./cache:/cache
```

The accepted form is `host:container[:ro]`. Workspace and Extra Mount sources
share these rules:

- Sources must exist. A Workspace is a directory; an Extra Mount may be a file
  or directory.
- Sources are canonicalized before validation and before Docker sees them, so a
  source symlink grants access to its destination.
- Resolved sources must be UTF-8 and contain no `:` because Docker's short `-v`
  syntax cannot represent them safely.
- Container targets must be absolute. Mounts are writable unless marked `:ro`.
- Extra Mount targets are literal: `/workspace/cache` is beside a Workspace
  mounted at `/workspace/project`, while `/workspace/project/cache` is inside
  it. They may be nested under the Workspace or `/home/aibox`, but cannot
  replace either managed mount or one of its ancestors.
- `$AIBOX_ROOT` and host paths containing it are rejected. Inside that root,
  only `tenants/<name>` or descendants may be mounted.

Mounting another Tenant Home exposes its Agent credentials and persistent
state. Every Extra Mount is an explicit authority grant.

## Runtime Boundary

Each Run drops Linux capabilities, enables `no-new-privileges`, mounts the
selected Tenant Home at `/home/aibox`, mounts the Workspace at
`/workspace/<directory name>`, and adds only requested Extra Mounts. The name
comes from the resolved Workspace source, so symlink aliases use the real
directory name. A filesystem root such as `/` cannot be a Workspace because
it has no directory name. Different host directories with the same name share
the same container path. Existing Agent state stored for `/workspace` stays
where it is; AIBox does not migrate it.

A Debug Shell uses the same disposable image and security flags but mounts only
the selected Tenant Home. Component installation does the same. Both retain
network access; Debug can modify every credential, Session, Config, and
Component file in that Home.

On Linux, the container uses the invoking uid and gid to preserve Workspace
ownership. AIBox maps `host.docker.internal` to Docker's host gateway; Docker
Desktop supplies the corresponding macOS integration.

The Filesystem Sandbox does not prevent network or credential-authorized remote
effects, changes through writable mounts, or excessive CPU, memory, and process
use. Built-in Config templates disable native Agent approval prompts because
Docker is the Filesystem Sandbox; native settings and credentials can still
grant authority beyond it.

## Cleanup

Runs, Debug Shells, and Component installers use disposable containers. AIBox
tracks the Docker child and cidfile and keeps cleanup armed until it confirms
that the container did not outlive the Docker client.

SIGINT, SIGTERM, and non-ignored SIGHUP stop the active container. The first
signal allows up to ten seconds for exit; a second skips the grace period and
kills immediately. An inherited ignored SIGHUP stays ignored. SIGKILL, wrapper
or host crashes, and some Docker failures cannot guarantee cleanup; inspect
Docker for leftovers after such an event.

Ordinary completion propagates the Docker, shell, or Agent exit status. If the
Docker client reports success but leaves a live or uninspectable container that
AIBox must kill, AIBox returns failure. One process supports only one active
Run, Debug Shell, or Component installation.

See [Requests and Request Proxy](requests.md) for host-side proxy routing,
recording, and diagnostics.

## Building the Shared Image

The Runtime Image is the fixed `aibox:latest` base used by Runs, Debug Shells,
and Component installers. Build it explicitly from Console Overview.

The embedded Dockerfile has an empty context and fetches dependencies during
`docker build -f -`. It supplies the shared OS, login shell, build and download
tools, diagnostics, fonts, and browser ABI libraries. Python, uv, Node.js,
Codex, Claude, Rust, and Go belong to Managed Tenant Components; the image also
installs no browser.

The image is not rebuilt when Components change. Runtime Image construction,
Component installation, Runs, and Debug Shells share the same one-active-
container-operation limit within one AIBox process.
