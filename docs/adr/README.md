# Architectural Decision Records

These ADRs record the architectural decisions that currently shape aibox and
the reasons those choices remain deliberate. Detailed behavior belongs in the
reference documents linked from the main [README](../../README.md#learn-more),
while [AGENTS.md](../../AGENTS.md) defines active repository constraints.

- [0001: Tenant identity and direct storage](0001-tenant-identity-and-direct-storage.md)
  models persistent identity without management metadata or Run History.
- [0002: Native Tenant Components](0002-native-tenant-components.md) derives
  optional capabilities from native state rather than a registry.
- [0003: One-shot Config Application](0003-one-shot-config-application.md) keeps
  Named and Current Config separate without retained activation state.
- [0004: Explicit Credential Propagation](0004-explicit-credential-propagation.md)
  distributes refreshed ChatGPT credentials without synchronization.
- [0005: Filesystem Sandbox and host trust](0005-filesystem-sandbox-and-host-trust.md)
  makes Docker the filesystem boundary and treats writable state as untrusted.
- [0006: Application-only command boundary](0006-cli-only-command-boundary.md)
  preserves native Coding Agent arguments without exposing orchestration APIs.
- [0007: Supervised Docker lifecycle](0007-supervised-docker-lifecycle.md) keeps
  container cleanup under wrapper control.
- [0008: Global trusted Request Proxy](0008-global-trusted-request-service.md)
  separates the Request Proxy from Tenants on one explicitly trusted listener.
- [0009: Request Record evidence and projections](0009-request-record-evidence-and-projections.md)
  keeps raw diagnostic evidence beside stable materialized views.
- [0010: Foreground Service and Console](0010-foreground-service-and-console.md)
  moves management into one Root-local foreground Service while preserving Run.
- [0011: Shared listener management boundary](0011-shared-listener-management-boundary.md)
  reserves loopback management paths without narrowing Request Proxy reachability.
- [0012: Ephemeral Management Operations](0012-ephemeral-management-operations.md)
  bounds and cancels one Service-lifetime long-running action.
- [0013: Last Application and Config Drift](0013-last-application-and-config-drift.md)
  records diagnostic application provenance without reconciliation.
- [0014: Fixed Runtime Image](0014-fixed-runtime-image.md) keeps every Run,
  build, and toolchain installer on `aibox:latest`.
- [0015: Named-only Visual Config Editing](0015-visual-and-raw-config-editors.md)
  keeps Visual editing on fixed Config Fields while preserving native Raw data.
- [0016: Session conversation projection](0016-session-conversation-projection.md)
  separates readable conversation, Tool Activity, and on-demand Transcript
  Evidence while preserving native order and diagnostic boundaries.
- [0017: Single-value Tenant selection](0017-single-value-tenant-selection.md)
  distinguishes Host and Managed Tenants without a second identity term.
- [0018: AIBox-owned Console visual system](0018-console-visual-system.md) uses
  Ant Design for shared interaction primitives; it is superseded by ADR 0019.
- [0019: Native Console UI primitives](0019-native-console-ui-primitives.md)
  keeps ordinary controls native and AIBox-owned without a general visual or
  headless UI framework.
