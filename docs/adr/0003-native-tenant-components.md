# Derive optional Tenant Components from native state

Status: accepted

aibox models optional status-line integrations and Managed Tenant-local
toolchains as Tenant Components. Status-line Components may target either a
Managed Tenant Home or the existing Host Home; toolchains remain Managed
Tenant-only. Native files are the source of truth rather than a separate
registry, while accepting that native user edits may make a Component modified
or unmanaged.
