# Architectural Decision Records

ADRs preserve the reason for an architectural choice, including choices that
were later replaced. Use the status below before treating an older decision as
current guidance. [AGENTS.md](../../AGENTS.md) defines the active repository
constraints, and the user-facing behavior has one canonical home in the
documents linked from the main [README](../../README.md#learn-more).

| ADR | Status | Decision |
| --- | --- | --- |
| [0001](0001-unified-tenants.md) | Accepted | Model managed and host state as Tenants |
| [0002](0002-tenant-local-agent-profile-reconciliation.md) | Superseded by 0005 | Materialize Tenant-local Agent Profiles with reconciliation |
| [0003](0003-native-tenant-components.md) | Accepted | Derive optional Tenant Components from native state |
| [0004](0004-agent-profile-and-component-ownership.md) | Superseded by 0005 | Separate Agent Profile and Component ownership |
| [0005](0005-one-time-fixed-field-config-application.md) | Accepted | Manage Named and Current Config without retained state |
| [0006](0006-global-traffic-records.md) | Accepted | Keep Traffic Records global and independent of Tenants |
| [0007](0007-upstream-semantic-traffic-records.md) | Accepted; v1 compatibility superseded by 0011 | Record HTTP semantics with raw evidence and a protocol summary |
| [0008](0008-explicit-cross-tenant-credential-propagation.md) | Accepted | Propagate ChatGPT credentials explicitly across Tenants |
| [0009](0009-relay-compatible-first-token.md) | Accepted; Record coexistence superseded by 0011 | Define First Token by relay-compatible SSE data arrival |
| [0010](0010-materialize-traffic-record-end-order.md) | Accepted | Materialize Traffic Record end order in directory names |
| [0011](0011-materialize-traffic-summary-assessment.md) | Accepted | Materialize the Traffic list projection and Record Assessment |
