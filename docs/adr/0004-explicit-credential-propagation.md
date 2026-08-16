# Propagate ChatGPT credentials explicitly

Credential Propagation is an explicit global exception that copies one newer
Host Tenant Codex ChatGPT credential snapshot only to older existing Configs
for the same account. It creates nothing, retains no relationship, and never
runs automatically, providing credential refresh without turning Tenant
configuration into centrally synchronized state.
