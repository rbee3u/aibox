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

The accepted form is `host:container[:ro]`. The source rules below apply to the
Workspace as well as Extra Mounts:

- The source must already exist. Relative sources resolve from the launch
  directory.
- Workspace and Extra Mount sources are resolved to their canonical paths
  before validation and before they are passed to Docker. A source symlink
  therefore grants access to its destination, not to the symlink entry.
- Resolved source paths must be valid UTF-8 and must not contain `:`, because
  Docker's short `-v` syntax cannot represent them safely.
- The container target must be absolute.
- Mounts are writable by default; `:ro` is the only accepted mode.
- Extra mounts may be nested beneath `/workspace` or `/home/aibox`, but may not
  replace either managed mount or one of its ancestors.
- `$AIBOX_ROOT` and any host path that contains it are rejected because they
  would expose host-only aibox state indirectly.

Within `$AIBOX_ROOT`, only `tenants/<tenant>` or one of its descendants may be
a bind source. Named Config catalogs and internal lifecycle staging
directories stay host-only.

Mounting another Tenant Home is allowed, but doing so exposes its Coding
Agent credentials and persistent state. Treat every Extra Mount as an explicit
authority grant.

## Runtime Boundary

Each Coding Agent Run:

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

The built-in Codex Named Config template sets `approval_policy = "never"` and
`sandbox_mode = "danger-full-access"`; Docker remains its Filesystem Sandbox.
The built-in Claude template uses `bypassPermissions` and suppresses its
dangerous-mode prompt. Native Agent settings may grant authority beyond the
Filesystem Sandbox, especially through mounted credentials or network
services, and remain the user's responsibility.

## Cleanup

Runs and toolchain installations use disposable Docker containers. aibox tracks
the Docker child and container id, and keeps cleanup armed until it has checked
that the container did not outlive the Docker client.

The wrapper handles SIGINT, SIGTERM, and non-ignored SIGHUP by stopping the
active container through Docker. After forwarding the first signal, it allows a
still-running container up to ten seconds to exit; sending a second signal
skips the remaining grace period and requests an immediate kill. A SIGHUP
already ignored by the parent process (for example under `nohup`) remains
ignored. SIGKILL, a wrapper crash, Docker failure, or a host failure cannot
guarantee cleanup. After such an event, inspect Docker for a leftover container
before starting sensitive work.

On ordinary completion, aibox propagates the `docker run` or Coding Agent exit
status. If the Docker client reports success but leaves a live or uninspectable
container that aibox must kill, aibox changes that successful status to a
failure; an existing failure status is preserved.

One aibox process supports one active container operation at a time: either a
Run or a Rust/Go Component installation.

## Traffic Proxy

`aibox traffic` is an independent foreground HTTP intermediary for temporary
model API debugging. It does not start Docker and may run in a separate process
alongside a Coding Agent Run:

```sh
aibox traffic
aibox traffic --listen 127.0.0.1:8080
aibox traffic --listen 0.0.0.0:9923 --allow-remote
```

The default listener is `127.0.0.1:9923`. `--listen` accepts only a literal
`IP:PORT` with a nonzero port, and every non-loopback address requires
`--allow-remote`. When a specific external or IPv6 address is selected, aibox
also binds `127.0.0.1` on the same port for management; wildcard IPv4 already
covers it. The viewer is always `http://127.0.0.1:<port>/` and rejects
non-loopback peers, non-loopback Host values, cross-origin and cross-site
requests, and invalid per-start CSRF tokens. It sends no CORS permission.

Docker Desktop provides `host.docker.internal`. aibox also maps that name to
the host gateway for Linux Runs, where the host listener commonly needs
`0.0.0.0` plus `--allow-remote`. For example:

```toml
base_url = "http://host.docker.internal:9923/https://hezubus.ai/v1"
```

The proxy removes the first slash and parses the remainder as the complete
upstream URL. Thus a request to
`/https://hezubus.ai/v1/responses?tag=a&tag=` forwards the same method, path,
repeated query values, headers, and body to
`https://hezubus.ai/v1/responses?tag=a&tag=`. Only `http` and `https` targets
are accepted. Redirects are returned without following them; requests are not
retried. System HTTP proxies, cookies, Referer synthesis, and automatic body
decompression are disabled. Host and hop-by-hop headers are rebuilt or
removed. CONNECT receives 405, and Upgrade/WebSocket receives 426. HTTP
trailers are not recorded or forwarded. Invalid targets return 400,
non-public targets 403, connection timeouts 504, other upstream failures 502,
and recording failures 507. Upstream 3xx/4xx/5xx statuses are returned without
being reclassified as proxy failures.

Before connecting, aibox resolves the target and requires every candidate to
be a public address, except that `198.18.0.0/15` is accepted for host-side
Fake-IP DNS proxies. Loopback, private, link-local, CGNAT, ULA, multicast,
unspecified, documentation, other reserved, and metadata destinations are
rejected; accepted addresses are pinned to the actual connection. TLS uses the
host's normal trusted CA roots. The only upstream timeout is a 30-second
connection timeout, so long-running SSE responses have no total or idle
timeout.

Request and response chunks are written to disk before they are forwarded.
This preserves ordinary HTTP bodies, binary data, and SSE event bytes without
parsing, truncation, redaction, or whole-message buffering. Disk latency
therefore applies backpressure. A recording error aborts forwarding and returns
507. Client disconnect, upstream stream failure, SIGINT, and SIGTERM cancel the
remaining attempt and retain bytes already written.

Each direct child of `$AIBOX_ROOT/traffic/` is one Traffic Record:

```text
<UTC-basic-time>-<sanitized-host-or-invalid>-<uuid-v7>/
  request.json
  request.body
  response.json
  response.body
  result.json
```

The collection and Record directories are mode `0700`; files are `0600`.
Metadata stores incoming and upstream URLs, base64 lossless header values,
status, HTTP version, byte counts, timings, outcome, and error details. Body
files contain the exact application-visible bytes. A missing `result.json`
from another process lifetime appears as interrupted/incomplete. Unknown or
structurally incomplete collection entries are ignored with warnings; selected
reads and deletion revalidate real paths and reject symlinks or unexpected
types.

There is no size limit, retention policy, redaction, database, or cross-process
lock. Authorization values, API keys, prompts, tool data, and model output are
stored in full and remain after the proxy exits. Use the viewer's selected
delete or separately confirmed **Delete all** action when debugging ends. An
active Record cannot be deleted; delete-all uses the confirmed current
non-active count and returns a conflict if that count changes.

Claude Messages and OpenAI Responses streaming are HTTP SSE and work through
this path. WebSocket transport is outside v1; if native Codex configuration
manually sets `supports_websockets = true` for the selected custom provider,
set it to `false` while using Traffic. Config Fields are unchanged.
See [Anthropic streaming](https://platform.claude.com/docs/en/build-with-claude/streaming),
[OpenAI Responses streaming](https://platform.openai.com/docs/api-reference/responses-streaming/response/refusal/delta?lang=curl),
and [Docker host networking](https://docs.docker.com/desktop/features/networking/networking-how-tos/).

## Building the Shared Image

Build the bundled image after installing aibox:

```sh
aibox build
aibox build --force
```

The image contains both Codex and Claude. `--force` disables Docker's build
cache and pulls a fresh Debian base. The build uses an embedded, context-free
[Dockerfile](../assets/aibox.Dockerfile), which is the source of truth for
installed packages and pinned Coding Agent versions.

The image includes common Unix development and diagnostic tools, Python with
pip/venv/uv, and Node.js with npm. Rust and Go are installed on demand into a
persistent Managed Tenant; see [Tenant Components](tenants.md#tenant-components).

## Custom Images

Set `AIBOX_IMAGE` to make image builds, Coding Agent Runs, and Rust/Go Component
installations use another image tag:

```sh
AIBOX_IMAGE=local/aibox:dev aibox build
AIBOX_IMAGE=local/aibox:dev aibox run
AIBOX_IMAGE=local/aibox:dev aibox component install rust
```

A Run or a launched toolchain installer still requires the selected image to
exist locally. To support a Run, a replacement image must:

- provide the selected `codex` or `claude` executable on `PATH`;
- use `/home/aibox` as `HOME`;
- support `/workspace` as its working directory;
- avoid an incompatible `ENTRYPOINT`.

For complete output, an installed Claude status-line Component expects Bash,
`jq`, `awk`, and `cat` in the runtime image; Git is optional and supplies the
branch field. The Codex status line uses native TUI support and adds no image
dependency.

Both toolchain installers require `HOME=/home/aibox`, no incompatible
`ENTRYPOINT`, Bash, curl, and standard Unix command-line utilities including
`mktemp`. Rust requires Python 3.11 or newer (for `tomllib`), `sed`, and `grep`;
Go requires Python 3.9 or newer, `dpkg`, tar, and `sha256sum`.

On Linux, aibox overrides the image user with the invoking host uid and gid.
Executables and required image files must therefore be readable and executable
by an arbitrary uid.
