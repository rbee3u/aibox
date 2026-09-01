# Filesystem Sandbox and Mounts

AIBox treats its Docker container as the Coding Agent, Debug Shell, or
Component installer's Filesystem Sandbox. It controls which host paths enter
the container; it does not confine network or credential authority.

## Workspace and Mount Rules

The launch directory is the default Workspace mounted at `/workspace`. Select
another existing directory with:

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
- Extra Mounts may be nested below `/workspace` or `/home/aibox`, but cannot
  replace either managed mount or one of its ancestors.
- `$AIBOX_ROOT` and host paths containing it are rejected. Inside that root,
  only `tenants/<name>` or descendants may be mounted.

Mounting another Tenant Home exposes its Agent credentials and persistent
state. Every Extra Mount is an explicit authority grant.

## Runtime Boundary

Each Run drops Linux capabilities, enables `no-new-privileges`, mounts the
selected Tenant Home at `/home/aibox`, mounts the Workspace at `/workspace`,
and adds only requested Extra Mounts.

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

## Request Proxy

The Request Proxy is an always-on part of the foreground Service. It runs on
the host, is global rather than Tenant-owned, and never starts Docker.

### Setup

Start the shared Console and proxy listener:

```sh
aibox console
aibox console --listen 127.0.0.1:8080
```

Docker Desktop can reach the default listener through
`host.docker.internal`. Native Linux Docker commonly needs a wildcard listener:

```sh
aibox console --listen 0.0.0.0:9923
```

For Codex, set a custom provider in Current `config.toml`:

```toml
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "http://host.docker.internal:9923/https://api.openai.com/v1"
wire_api = "responses"
requires_openai_auth = true
```

For Claude, set its native base URL in Current `settings.json`:

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "http://host.docker.internal:9923/https://api.anthropic.com"
  }
}
```

Use the Configs module to edit Current Config. The proxy prefix contains the
complete upstream base URL.

### Routing and Network Policy

The default listener is `127.0.0.1:9923`. `--listen` accepts one literal
`IP:PORT` with a nonzero port and binds exactly that socket. The same listener
serves proxy traffic and the Console.

Console paths (`/` and `/_aibox/*`) require an actual loopback TCP peer and
loopback Host. Browser mutations additionally require JSON, a same-origin
Origin, and the startup CSRF token. Other paths are Request Proxy input, so a
wildcard listener can serve containers without exposing management routes.

The path after the first slash is the complete absolute upstream URL. AIBox
preserves the method, path, repeated query values, headers, and body. Only
`http` and `https` targets are accepted. Redirects pass through without being
followed, and requests are not retried. Host and hop-by-hop headers are rebuilt
or removed; CONNECT and Upgrade/WebSocket are unsupported.

Before connecting, AIBox resolves the target and requires every address to be
public. The only exception is `198.18.0.0/15` for host-side Fake-IP DNS.
Loopback, private, link-local, CGNAT, ULA, multicast, unspecified,
documentation, other reserved, and metadata addresses are rejected. Accepted
addresses are pinned to the connection; TLS uses the host's trusted CA roots.

| Failure | Status |
| --- | ---: |
| Invalid target | 400 |
| Non-public target | 403 |
| CONNECT | 405 |
| Upgrade/WebSocket | 426 |
| Connection timeout | 504 |
| Other upstream failure | 502 |
| Recording failure before response commit | 507 |

Upstream error responses pass through normally. The only upstream timeout is a
30-second connection timeout; long-running SSE responses have no total or idle
timeout.

### Recording and Storage

Request and response chunks are written to disk before forwarding. AIBox
preserves application-visible header values and body bytes without parsing,
truncation, redaction, decompression, or whole-message buffering. Disk latency
therefore applies backpressure, and a recording error aborts forwarding.

Before downstream headers commit, a recording failure can replace the response
with 507. After commit, the body is truncated and the downstream stream errors.
Client disconnect, upstream failure, signal shutdown, or interruption retains
bytes already written.

Requests live below `$AIBOX_ROOT/requests/`. An active directory is renamed
after its terminal Summary commits. Each Request stores raw request and
response metadata, bodies, lifecycle Summary, and an optional best-effort SSE
index. Directory names are ordering hints; the Summary is lifecycle authority.

New Requests begin at the collection root. When more than 500 ungrouped
Requests exist, the Service periodically moves the oldest 200 eligible terminal
Requests into an immutable Request Group. Groups are not merged or refilled.
Deleting grouped Requests updates the Group count and removes an empty Group;
interrupted grouping is reconciled on a later read or compaction tick.

Collection and Request directories use `0700`; evidence files use `0600`.
Listing reads persisted Summaries, while detail strictly opens raw evidence.
Malformed or unsafe evidence can therefore fail detail without hiding a valid
list row. Unknown collection entries warn and are ignored; selected operations
revalidate paths and reject symlinks or unexpected types.

### Diagnostics

The proxy best-effort recognizes OpenAI Responses, OpenAI Chat Completions, and
Claude Messages. It records model, reasoning effort, response mode, First
Token, final Token Usage, Provider diagnostics, and an unverified Coding Agent
Session ID. Recognition never changes forwarding.

For recognized streams, First Token is the receipt time of the first nonempty
SSE `data:` line that is not a `[DONE]` prefix. It is a transport diagnostic,
not proof that semantic model output or a tokenizer token arrived. Unknown,
malformed, oversized, or content-encoded streams remain raw and readable even
when semantic indexing degrades.

Request Assessment keeps lifecycle, HTTP status, Provider Error, and warnings
as independent evidence:

| Assessment | Meaning |
| --- | --- |
| Active | The Request has not terminated |
| OK | Terminal with no abnormal evidence |
| Warning | Interrupted, disconnected, or degraded without error-class evidence |
| Error | Recording, transport, HTTP, Provider, or protocol failure |

HTTP semantics rather than transport frames are recorded. Original header
casing and cross-name order, framing, informational responses, trailers, TLS
records, and HTTP/2 frames are not preserved.

### Retention and Deletion

There is no body limit, retention policy, redaction, database, or cross-process
lock. Authorization values, API keys, prompts, tool data, and model output
persist in full after the Service exits.

Delete evidence from the Requests module when debugging ends. Active Requests
cannot be deleted. Selected deletion validates every target before removing
any; a grouped deletion updates or removes its Group. Deletion is irreversible.

Claude Messages, OpenAI Responses, and Chat Completions streams work as HTTP
SSE. WebSocket and CONNECT transports are outside the supported surface.

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
