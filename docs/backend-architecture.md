# Backend Architecture

AIBox remains one application-only Rust crate. Its only public entry point is
`main_entry`; domain facades and the Control API are internal implementation
boundaries. Domain behavior belongs in the canonical references linked from
[AGENTS.md](../AGENTS.md).

## Ownership and Dependencies

| Owner | Responsibility |
| --- | --- |
| `cli` and crate entry | Parse the three commands and convert CLI DTOs into execution commands |
| `execution` and `sandbox` | Orchestrate Run/Debug and validate their filesystem authority |
| `docker` | Runtime Image construction and supervised container/process cleanup |
| `agent`, `tenant`, `config`, `component`, `session` | Domain contracts, native state, and lifecycle policies |
| `request` | Raw evidence, observation, projections, persistence, and forwarding |
| `management` | Cross-domain use cases and shared process-local management capabilities |
| `service` | Foreground process composition, runtime lifecycle, and Console HTTP adapters |
| `foundation` | Policy-free filesystem, platform, and synchronization mechanics |

Service composes Management and Request capabilities; Management calls domain
facades. Management never imports Service, Axum, or Clap. HTTP handlers decode
wire selectors and inputs before invoking typed use cases and map domain results
to wire responses. Service merges its own listener and uptime information into
Overview responses.

Management coordinators receive only required capabilities, never a whole
Service context. Cloned handles share the mutation gate, Operation Manager,
Credential Propagation plans, and release snapshot. Runtime Image builds use
Operation ownership without the mutation gate. The Operation coordinator owns
container cancellation; the Operation Manager owns state and logs. These
capabilities are process-local.

A Management mutation token owns the gate. Its blocking runner transfers the
token into the worker, so cancelling the asynchronous caller cannot release
mutation exclusion while the worker is still writing. Long Operations consume
the token inside their own worker; ordinary reads use the ungated blocking
runner. Acquiring the token remains an explicit step before scheduling work.

Service `runtime` constructs Management and owns initialization, signals,
background tasks, and shutdown. Every exit cancels and aborts best-effort async
workers without draining them or interrupting running blocking work. Request
response tasks and Management Operations retain separate drain policies.
`http` owns router composition and management guards; `state` carries process,
security, shutdown, and capability handles. `control` owns the internal API and
[generated contract](console-architecture.md#control-api-and-generated-contracts).

## Domain Internals

Facades expose operations and types without exposing their implementation
modules. Model modules hold identities, inputs, and snapshots; only extract a
model module when it separates substantial declarations from facade assembly.
Keep shared mechanics in their narrowest owner.

Session accepts Agent identity and Home at its facade and selects the native
Transcript backend internally. Management owns Tenant selection and scheduling;
it does not construct parsers or pass backend objects between domain operations.

Config's private `storage` boundary owns Named Config paths, validation, bounded
reads, and atomic replacement; `storage::files` holds single-file mechanics.
Catalog, editing, Application, and Credential Propagation call storage directly.
Production and tests use typed Config targets, files, and edits. Metadata has
separate Tenant-and-Agent ownership.

Component owns the healthy capability snapshot used by Tenant Environment
composition. Execution owns the login-shell environment wrapper and Agent/Debug
command construction; Sandbox owns the container Home mount target. Tenant
continues to own identity, storage layout, and recoverable lifecycle.

The SSE observer maps bytes and arrival times to events and raw byte ranges
without file I/O, Request IDs, or schema versions. Store owns the index schema
and persistence. Proxy records raw bytes before indexing; compressed replay uses
only the observer. Index failures degrade diagnostics without failing forwarding.
See [ADR 0007](adr/0007-request-evidence-and-materialized-projections.md).

## Enforcement and Tests

`src/architecture_tests/` enforces source-owner coverage, exact depth-one/depth-two
dependencies, acyclicity at every depth, Management/transport and SSE/Store
boundaries, test placement, and `TEST_ONLY_SURFACE`. Declare each dependency
once in `allowed_dependencies`; remove it when its last use disappears.

Tests follow their owner: Management tests exercise operations and observation
without HTTP; Control tests exercise wire behavior with in-memory Tower
services; Service runtime and HTTP tests exercise startup/shutdown and guards.
Shared Service fixtures live in its test-only `testutil` module. Production
visibility is never expanded to accommodate a test; necessary injection seams
are test-only and listed in `TEST_ONLY_SURFACE`.

Use the checks in [Development](development.md).
