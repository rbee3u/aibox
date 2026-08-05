# Keep Traffic Records global and independent of Tenants

Status: accepted

The Traffic Proxy is a temporary host-side HTTP/SSE diagnostic tool. Its
Traffic Records live directly under `$AIBOX_ROOT/traffic/<record>/`, outside
Managed Tenant Homes and Agent Profile catalogs. The command owns only
`--listen` and `--allow-remote`; it neither selects a Tenant or Coding Agent nor
starts Docker.

Each Traffic Record represents one proxy attempt rather than one Coding Agent
Run, Session, or upstream conversation. A single Traffic Proxy can observe
requests from multiple containers, host processes, Tenants, or Coding Agents,
and a request does not carry enough trustworthy information to assign Tenant
ownership.

## Considered Options

Tenant-local storage would appear to align credentials with their usual
Tenant, but the proxy cannot authenticate that association and can be used by
arbitrary local clients. Run-local storage would invent persistent Run History
and a Run-to-Session relationship that aibox deliberately does not maintain.
Using Coding Agent Transcripts would lose raw headers, bodies, transport
failures, and streaming timing while coupling the proxy to agent-specific
formats.

## Consequences

aibox exposes Traffic Record inspection and deletion only through the loopback
management page of a running Traffic Proxy; the owner can still inspect the raw
files directly. Records are not mounted into a Run, do not affect Tenant
lifecycle, and survive Tenant deletion. The collection is flat and
intentionally unindexed for temporary debugging. Records contain unredacted
credentials and model data, so owner-only filesystem modes and explicit
deletion are essential. Multiple Traffic Proxy processes have no locking or
coordination guarantee.
