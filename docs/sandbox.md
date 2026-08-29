# Filesystem Sandbox and Mounts

AIBox treats the Docker container as the Coding Agent or Debug Shell's
Filesystem Sandbox. It limits which host paths enter the container while
leaving the process free to work inside those mounts.

This Filesystem Sandbox is not a complete security boundary. Networking remains
enabled, credentials can authorize remote effects, and Docker still relies on
the host daemon and platform.

## Run in Another Workspace

The current directory is the default Workspace and is mounted at `/workspace`.
Select another existing directory with `--workspace`:

```sh
aibox run -w ../other-project
```

Relative paths are resolved from the directory where `aibox` was launched.

## Mount Rules

Add existing host files or directories with Docker-style short mount syntax:

```sh
aibox run -m ../reference:/reference:ro
aibox run -m ./cache:/cache
```

The accepted form is `host:container[:ro]`. The source rules below apply to the
Workspace as well as Extra Mounts:

- The source must already exist. An Extra Mount source may be a file or a
  directory; a Workspace must be a directory. Relative sources resolve from the
  launch directory.
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
  would expose host-only AIBox state indirectly.

Within `$AIBOX_ROOT`, only `tenants/<tenant>` or one of its descendants may be
a bind source. Named Config catalogs, Requests, and internal lifecycle
staging directories stay host-only.

Mounting another Tenant Home is allowed, but doing so exposes its Coding
Agent credentials and persistent state. Treat every Extra Mount as an explicit
authority grant.

## Runtime Boundary

Each Run:

- drops all Linux capabilities;
- enables `no-new-privileges`;
- mounts the selected Tenant Home at `/home/aibox`;
- mounts the selected Workspace at `/workspace`;
- adds only the extra mounts supplied on the command line.

A Debug Shell uses the same disposable Runtime Image and security flags, but
mounts only the selected Tenant Home at `/home/aibox`, starts there, and accepts
no Workspace or Extra Mount. It requires no Coding Agent Component. The shell
has network access and can directly modify credentials, Sessions, Configs, and
Component state in the Tenant Home.

Runtime and toolchain Component installation also uses a disposable,
cleanup-aware container, but mounts only the selected Tenant Home at
`/home/aibox`; it does not mount a Workspace or accept Extra Mounts. The
installer retains normal network access to official distribution services.

On Linux, the container runs with the invoking uid and gid so Workspace files
retain host ownership. AIBox also maps `host.docker.internal` to Docker's host
gateway. Docker Desktop provides the host integration on macOS.

The following remain outside the Filesystem Sandbox:

- Container networking is enabled.
- Credentials may authorize changes to repositories, APIs, cloud accounts, or
  other remote systems.
- AIBox adds no CPU, memory, or process-count limits.
- Writable Workspace, Tenant Home, and Extra Mounts can be modified or
  deleted by the Coding Agent.

The built-in Codex Named Config template sets `approval_policy = "never"` and
`sandbox_mode = "danger-full-access"`; Docker remains its Filesystem Sandbox.
The built-in Claude template uses `bypassPermissions` and suppresses its
dangerous-mode prompt. Native Agent settings may grant authority beyond the
Filesystem Sandbox, especially through mounted credentials or network
services, and remain the user's responsibility.

## Cleanup

Runs, Debug Shells, and Component installations use disposable Docker
containers. AIBox tracks the Docker child and container id, and keeps cleanup
armed until it has checked that the container did not outlive the Docker
client.

The wrapper handles SIGINT, SIGTERM, and non-ignored SIGHUP by stopping the
active container through Docker. After forwarding the first signal, it allows a
still-running container up to ten seconds to exit; sending a second signal
skips the remaining grace period and requests an immediate kill. A SIGHUP
already ignored by the parent process (for example under `nohup`) remains
ignored. SIGKILL, a wrapper crash, Docker failure, or a host failure cannot
guarantee cleanup. After such an event, inspect Docker for a leftover container
before starting sensitive work.

On ordinary completion, AIBox propagates the `docker run`, Debug Shell, or
Coding Agent exit status. If the Docker client reports success but leaves a
live or uninspectable container that AIBox must kill, AIBox changes that
successful status to a failure; an existing failure status is preserved.

One `aibox` process supports one active container operation at a time: a Run,
Debug Shell, or Component installation.

## Request Proxy

The Request Proxy is an always-on part of the foreground AIBox Service. It does
not start Docker and may run alongside a separate `aibox run` process:

```sh
aibox console
aibox console --listen 127.0.0.1:8080
aibox console --listen 0.0.0.0:9923
```

The foreground command prints its Listen and Console addresses;
runtime events include RFC 3339 UTC timestamps. Runtime output is intentionally
concise: safe internal warnings and Error-assessed abnormal Request Outcomes
include only a 12-character Request ID, method, upstream host, fixed reason, UTC
time, and duration. It never prints request paths, headers, bodies, prompts,
credentials, or raw upstream errors.
Completed Requests are not printed merely because an upstream returned HTTP
4xx/5xx or a Provider diagnostic was recorded; inspect those details in the
Requests module. Ctrl-C exits successfully after active Requests are finalized;
SIGTERM exits 143. A second signal forces exit using its conventional signal
exit code.

The default listener is `127.0.0.1:9923`. `--listen` accepts only a literal
`IP:PORT` with a nonzero port. AIBox binds exactly that socket; it does not
resolve hostnames, add a loopback listener, or add another IP protocol family.
The same socket serves the Request Proxy and Console. Console paths (`/` and
`/_aibox/*`) require an actual loopback TCP peer and loopback Host. Other paths
remain Request Proxy input, so wildcard listeners can be reached by containers
without exposing management. Browser mutations also require JSON, same-origin
Origin, and the startup CSRF token.

Docker Desktop provides `host.docker.internal`. AIBox also maps that name to
the host gateway for Linux Runs, where the host listener commonly needs
`0.0.0.0`. A custom provider uses the complete-upstream encoding in its provider
table:

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
and recording failures 507 while downstream response headers can still be
replaced. Upstream 3xx/4xx/5xx statuses are returned without being reclassified
as proxy failures.

Before connecting, AIBox resolves the target and requires every candidate to
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
therefore applies backpressure. A recording error aborts forwarding. Before
downstream response headers are committed, the proxy can replace the response
with 507; afterward it reports an error on the downstream body and truncates
the exchange. A client disconnect, upstream stream failure, SIGINT, or SIGTERM
cancels the remaining attempt and retains bytes already written. For SSE, a
recognized terminal signal from Claude Messages, OpenAI Responses, or OpenAI
Chat Completions completes the Request Outcome even when the client closes
immediately after consuming that signal.

Each direct child of `$AIBOX_ROOT/requests/` is one Request:

```text
active-<start-UTC-basic-time>-<sanitized-host-or-invalid>-<uuid-v7>/
  # renamed after terminal Summary commit to:
<end-UTC-basic-time>-<sanitized-host-or-invalid>-<uuid-v7>/
  request.json
  request.body
  response.json          # present only after upstream response headers arrive
  response.body
  response.events.jsonl  # optional index for recognized identity-coded SSE
  summary.json
```

The UUID is the Request identity. The directory name is a materialized ordering
hint: `active-` means only that a terminal name has not been successfully
materialized, not that the Request is necessarily active. `summary.json` is the
lifecycle authority. A process interruption can leave a non-terminal Summary,
and a rename or directory-sync failure can leave a terminal Summary under an
`active-` name; the latter is warned without changing its Outcome or End Time.
Safe older unprefixed names remain readable without migration. New names use
UTC-basic millisecond timestamps derived from the Summary's canonical start or
end time.

The collection and Request directories are mode `0700`; files are `0600`.
Metadata stores the upstream URL, base64 lossless header values, upstream
status and HTTP version, nanosecond timing checkpoints, outcome, and
diagnostics. Request format v4 makes `summary.json` the complete Request
Summary used by list reads: request and response list fields, lifecycle,
Request Assessment, the optional Coding Agent Session ID reported by a
recognized model request, and the optional Model Protocol Summary. The latter
contains protocol family, response terminality, requested/effective model and
reasoning effort, requested/observed response mode, First Token, final Token
Usage, and provider diagnostics. Body files contain the exact
application-visible bytes;
their current lengths are derived rather than persisted. `summary.json` exists
from Request creation and remains non-terminal if the process is interrupted. A
recognized identity-coded event-stream response also has a best-effort JSONL
index whose byte ranges point back into `response.body`. Recognition normally
uses `Content-Type: text/event-stream`; a successful recognized model request
that asked for streaming but has no Content-Type is sniffed from its body
prefix. Content-encoded streams remain unindexed because decoded boundaries
cannot be mapped to raw byte offsets. Indexing never changes forwarding or the
Request Outcome. Unknown, incompatible, or structurally incomplete
collection entries are ignored with warnings; selected reads and deletion
revalidate real paths and reject symlinks or unexpected types.

List scanning opens only each real `summary.json`; it does not inspect raw
request/response metadata, Bodies, or the SSE index. A valid Summary therefore
keeps its list row visible when separate raw evidence is malformed or unsafe.
Detail reads remain strict over that evidence and fail rather than following or
repairing it. Format v3 Requests are unsupported: they are not read, migrated,
rewritten, or deleted by the Service. Before the first start of an upgraded
Service, stop the old Service, optionally back up the collection, and manually
remove `$AIBOX_ROOT/requests`; the new Service recreates an empty collection.

Every terminal Request Outcome, including rejection, upstream failure, client
disconnect, recording failure, and server shutdown, has a Request End Time
derived from the Summary start anchor and terminal monotonic offset. Active and
interrupted Requests do not. The Requests module orders canonical directory
basenames by descending ASCII order: active and interrupted Requests first by
start time, then terminal Requests by End Time, with host and UUID breaking
millisecond ties. A terminal Summary stranded under an `active-` name is ordered by the
terminal name it should have received.

The Request Proxy best-effort materializes the protocol overview for OpenAI
Responses, OpenAI Chat Completions, and Claude Messages as stable facts become
available. It recognizes normalized upstream paths ending in `/responses`,
`/chat/completions`, or `/messages`; exact native response object types can also
identify a protocol on another path. Partial Token Usage stays in memory until
the model protocol reports a terminal response. Chat Completions maps prompt,
cached, cache-write, completion, and reasoning counters into the existing Token
Usage categories, and warns when reported totals are inconsistent. A stream
that requested `stream_options.include_usage` also warns when its normal
`[DONE]` signal arrives without usage. Protocol parsing failures add warnings
without changing forwarding or the Request Outcome.

For recognized streaming responses, First Token is recorded when the first
trim-nonempty SSE `data:` line not beginning with `[DONE]` is completely
received. The line need not form valid JSON or contain output, so ping, error,
empty-delta, `message_start`, `response.created`, and role-only Chat Completions
chunks qualify. Comments, other SSE fields, blank data, and `[DONE]` prefixes do
not. Split lines use their terminator's arrival time, while an unterminated EOF
line uses the final body arrival time. First Token is never inferred from
response headers or the first response body byte, and remains absent for
unknown protocols and non-streaming responses.

For Chat Completions, only an SSE data value that trims exactly to `[DONE]` is a
normal protocol terminal signal; a choice `finish_reason` does not end the
stream because a final usage chunk may follow. `stop`, `tool_calls`, and the
legacy `function_call` are normal finish reasons. `length` and `content_filter`
become Provider Errors, while an unknown nonempty finish reason is a diagnostic
warning. A structured Chat Completions stream error is an error terminal signal
and does not require a later `[DONE]`.

Raw Bodies remain unlimited, but the best-effort SSE observer stops further
indexing and protocol interpretation for a response after 16 MiB of an
unterminated line or accumulated Event fields. It records a warning and
releases its buffered Event while raw forwarding and recording continue
unchanged. This keeps a malformed or extreme event from making the host-side
diagnostic parser retain an unbounded copy of the stream.

Request Assessment classifies active Requests as Active and clean terminal
Requests as OK. Recording, proxy/transport, HTTP 4xx/5xx, Provider Error,
`response.failed`, `response.incomplete`, and incomplete or filtered Chat
Completions evidence is Error. Client
disconnect or request upload abort, process interruption, a missing recognized
model terminal event, diagnostic degradation, and an unaccompanied
`response.cancelled` are Warning. Request Outcome, HTTP status, and Provider
Error remain separately recorded; for example HTTP 200 can have a Provider
Error or an upstream stream failure, while HTTP 401 can coexist with structured
Provider Error details.

Content-encoded SSE remains raw and unindexed. A supported zstd stream is
semantically interpreted only after complete EOF, without synthesizing First
Token or Event timings from decoded offsets.

The raw request, response, and SSE index remain the diagnostic evidence. The
list API reads the persisted Summary only; the detail API opens raw metadata
and Body entries strictly. Unknown HTTP remains readable without being treated
as a model API response.

The Request Proxy records HTTP semantics rather than transport frames. Downstream and
upstream protocol negotiation are independent. Header values and repeated
same-name values are retained, but `HeaderMap` cannot preserve cross-name wire
order and may normalize field-name casing; HTTP/2 names are lowercase. Hop-by-
hop and framing fields, HTTP/2 pseudo-headers, automatically generated Host
fields, informational responses, trailers, TLS records, and HTTP/2 frames are
not captured. Only a final upstream response is persisted. A proxy-generated
error response is returned to the client but is not written as upstream data.

There is no size limit, retention policy, redaction, database, or cross-process
lock. Authorization values, API keys, prompts, tool data, and model output are
stored in full and remain after the Service exits. Use the Requests module's
single-Request or selected delete action when debugging ends. An active Request
cannot be deleted. Selected deletion strictly validates every target before
deleting any of them.

Claude Messages, OpenAI Responses, and OpenAI Chat Completions streaming are
HTTP SSE and work through this path. WebSocket transport is outside the Request
Proxy's supported surface; if native Codex configuration manually sets
`supports_websockets = true` for the selected custom provider, set it to
`false` while using the Request Proxy. Config Fields are unchanged.
See [Claude Messages streaming](https://platform.claude.com/docs/en/build-with-claude/streaming),
[OpenAI Responses streaming](https://platform.openai.com/docs/api-reference/responses-streaming/response/refusal/delta?lang=curl),
[OpenAI Chat Completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create/),
and [Docker host networking](https://docs.docker.com/desktop/features/networking/networking-how-tos/).

## Building the Shared Image

After installing AIBox, start `aibox console`, open Console Overview, and choose
**Build** to build the bundled image. **Build without cache** reruns every layer
and pulls a fresh Debian base image.

The image is a shared OS substrate without callable application language
runtimes or Coding Agents. The build uses an embedded, context-free
[Dockerfile](../assets/aibox.Dockerfile), which is the source of truth for
image-owned packages.

The image includes common Unix development and diagnostic tools plus the
download, checksum, extraction, and compilation tools needed by Tenant
Component installers. Python/uv, Node.js, Codex, Claude, Rust, and Go are
installed explicitly into a persistent Managed Tenant; see [Tenant
Components](tenants.md#tenant-components). A system diagnostic such as GDB may
retain a transitive `libpython` ABI dependency, but the image provides no
callable `python`, `pip`, `uv`, or `uvx` command.

For complete output, an installed Claude statusline Component expects Bash,
`jq`, `awk`, and `cat` in the runtime image; Git is optional and supplies the
branch field. It renders the model/reasoning, current directory (abbreviating
Home as `~`), branch, compact context-window size, and context-used percentage
in that order. The Codex statusline uses native TUI support and adds no image
dependency.

Component installers require `HOME=/home/aibox`, no incompatible `ENTRYPOINT`,
Bash, curl, and standard Unix command-line utilities including `mktemp`. Node
uses tar, xz, jq, and `sha256sum`; Python bootstraps the official standalone uv
installer, which downloads hash-verified Astral CPython builds; Rust resolves
stable versions through rustup and shell tools; Go uses `jq`, `dpkg`, tar, and
`sha256sum`. None of these installers requires an image-owned Python.

On Linux, AIBox overrides the image user with the invoking host uid and gid.
Executables and required image files must therefore be readable and executable
by an arbitrary uid.
