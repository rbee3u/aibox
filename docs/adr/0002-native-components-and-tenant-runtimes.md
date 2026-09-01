# Keep Components native and runtimes Tenant-local

AIBox derives Components from native Tenant state and keeps mutable language
runtimes, toolchains, and Coding Agent executables inside Managed Tenants while
the Runtime Image remains fixed. This preserves independent Tenant state and
native edits without a registry or reconciliation.
