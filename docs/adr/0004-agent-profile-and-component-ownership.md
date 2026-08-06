# Separate Agent Profile and Tenant Component ownership

Status: superseded by
[ADR 0005](0005-one-time-fixed-field-config-application.md).

Each Tenant and Coding Agent scope has at most one Active Agent Profile.
Reconciliation automatically adopts working-only changes into that Agent
Profile, including internal tombstones for deletions. A Tenant Component owns
its native configuration paths independently: Agent Profile comparison and
base restoration exclude those paths, and any operation that would create
overlapping ownership is rejected before it writes.

## Considered Options

Treating Component edits as ordinary working changes would absorb status-line
configuration into the Active Agent Profile and later remove it during a
switch or deactivation. Letting both owners write with a precedence rule made
the result order-dependent. A separate Component registry would duplicate the
native files that already express Component state.

## Consequences

Installed, modified, or incomplete status-line values survive Agent Profile
activation, reconciliation, switching, and deactivation. An inactive Agent
Profile may contain overlapping paths but cannot activate while the Component
exists; installation is likewise refused when the Active Agent Profile owns a
Component path. This preserves native-state discovery and roll-forward
transactions at the cost of explicit conflict resolution by changing or
removing one owner first.
