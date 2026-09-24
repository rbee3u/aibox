# Requests and Request Proxy

The Request Proxy is an always-on part of the foreground Service. It runs on
the host, is global rather than Tenant-owned, and never starts Docker.

## Setup

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

## Routing and Network Policy

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

Before connecting, AIBox resolves the target and pins the resolved addresses
to the connection. It accepts public, private, and loopback upstream addresses;
TLS uses the host's trusted CA roots. To forward through a local service on the
AIBox host at port 18787, use an upstream base URL such as
`http://host.docker.internal:9923/http://127.0.0.1:18787/v1` from a Managed
Tenant. The `127.0.0.1` inside the proxy path is resolved by the host-side
Service. Any client that can reach the proxy listener can also request host-local
upstreams through it.

| Failure | Status |
| --- | ---: |
| Invalid target | 400 |
| CONNECT | 405 |
| Upgrade/WebSocket | 426 |
| Connection timeout | 504 |
| Other upstream failure | 502 |
| Recording failure before response commit | 507 |

Upstream error responses pass through normally. The only upstream timeout is a
30-second connection timeout; long-running SSE responses have no total or idle
timeout.

## Recording and Storage

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

## Diagnostics

The proxy best-effort recognizes OpenAI Responses, OpenAI Chat Completions, and
Claude Messages. It records model, reasoning effort, response mode, First
Token, final Token Usage, Provider diagnostics, and an unverified Agent
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

## Retention and Deletion

There is no body limit, retention policy, redaction, database, or cross-process
lock. Authorization values, API keys, prompts, tool data, and model output
persist in full after the Service exits.

Delete evidence from the Requests module when debugging ends. Active Requests
cannot be deleted. Selected deletion validates every target before removing
any; a grouped deletion updates or removes its Group. Deletion is irreversible.

Claude Messages, OpenAI Responses, and Chat Completions streams work as HTTP
SSE. WebSocket and CONNECT transports are outside the supported surface.

