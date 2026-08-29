# Model persistent identity as Tenants in direct storage

AIBox models persistent Coding Agent state as either a runnable Managed Tenant
or the management-only Host Tenant, while Runs and Debug Shells remain
transient and unrelated to Session identity. The dedicated but unmarked AIBox
Root uses native files and real Tenant directories as its direct source of
truth, keeping state inspectable and Tenant scoping uniform without management
wrappers, layout metadata, or Run History.
