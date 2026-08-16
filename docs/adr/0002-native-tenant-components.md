# Derive Tenant Components from native state

aibox models optional status lines and Tenant-local toolchains as Tenant
Components whose state is derived from their native files rather than a
registry. This preserves compatibility with direct user and Coding Agent edits
and keeps Component ownership independent from Config Fields, while accepting
that native state can be incomplete, modified, or unmanaged.
