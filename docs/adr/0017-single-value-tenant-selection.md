# Encode Tenant selection as one value

The Control API and shareable Console URLs identify a Tenant with one `tenant`
value: `host` for the Host Tenant or `managed:<name>` for a Managed Tenant,
defaulting an omitted value to `managed:default`. This keeps Tenant as the
canonical identity term, distinguishes the Host Tenant from a Managed Tenant
named `host`, and avoids a separate kind-and-name pair; the previous `scope`
encoding is intentionally unsupported rather than retained as a migration
alias.
