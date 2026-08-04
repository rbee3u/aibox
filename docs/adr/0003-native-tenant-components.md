# Derive optional Tenant Components from native state

Status: accepted

aibox models optional status-line integrations and Tenant-local toolchains as
Tenant Components available only to Managed Tenants. Their native Tenant Home
files are the source of truth rather than a separate registry: this keeps
installation discoverable without adding layout versions, Host Home mutation,
or another lifecycle to reconcile, while accepting that native user edits may
make a Component modified or unmanaged.
