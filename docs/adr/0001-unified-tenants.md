# Model managed and host state as Tenants

Status: accepted

aibox models persistent identity as `Tenant::{Managed, Host}` instead of
maintaining separate Namespace and Target concepts. Managed Tenants are
runnable and own `$AIBOX_ROOT/tenants/<name>` Homes; the Host Tenant is
management-only, uses the real host Home, and stores its Named Config catalog
under the reserved `__host` key. This keeps Named Config and Session scoping
uniform without making host state runnable or deletable.

## Consequences

`--tenant <name>` and `--host` remain explicit, mutually exclusive CLI choices.
A Managed Tenant named `host` is ordinary and runnable. The direct layout has
no layout marker, management wrapper, migration reader, or lock directory; only
a Managed Tenant Home subtree may be mounted from `$AIBOX_ROOT`.
