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
- [0006: CLI-only command boundary](0006-cli-only-command-boundary.md) preserves
  native Coding Agent arguments without exposing orchestration APIs.
- [0007: Supervised Docker lifecycle](0007-supervised-docker-lifecycle.md) keeps
  container cleanup under wrapper control.
- [0008: Global trusted Traffic service](0008-global-trusted-traffic-service.md)
  separates Traffic from Tenants on one explicitly trusted listener.
- [0009: Traffic Record evidence and projections](0009-traffic-record-evidence-and-projections.md)
  keeps raw diagnostic evidence beside stable materialized views.
