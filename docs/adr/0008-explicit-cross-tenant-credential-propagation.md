# Propagate ChatGPT credentials explicitly across Tenants

Status: accepted

ChatGPT sign-in lets Codex refresh native credentials inside Current Config,
while Named Configs and other Tenant Homes otherwise retain older independent
copies. aibox therefore provides an explicit, one-way Credential Propagation
operation from Host Current Config to older same-account Codex Configs across
Tenants; it copies no API-key credentials, retains no association, creates no
missing Config, and performs no automatic reconciliation.

## Consequences

Credential Propagation is the deliberate global exception to ordinary
Tenant-scoped Config commands. It compares native account identity and refresh
time, replaces each selected `auth.json` atomically, continues after individual
write failures, and offers neither cross-file rollback nor cross-process
coordination. Config Application and Runs remain unchanged and never trigger
propagation.
