# Apply fixed Agent Profile fields without retained state

Status: accepted

Each Agent Profile belongs to one Tenant and Coding Agent and contains only a
fixed set of native configuration fields. `profile apply` projects those fields
once into the current Agent Configuration: present Profile Fields replace the
corresponding values, missing Profile Fields remove them, and unrelated native
configuration is preserved. The command retains no Active Profile association,
base snapshot, applied snapshot, tombstone, or transaction record. Runs consume
Agent Configuration without consulting Profiles.

Claude credentials expose only `ANTHROPIC_AUTH_TOKEN`, projected into
`settings.json.env`; Codex Profile auth replaces native `auth.json` as a whole.
Status-line configuration is outside the fixed Profile Fields and is therefore
preserved by application without cross-owner coordination.

## Considered Options

Three-way reconciliation preserved an ongoing relationship between a Profile
and mutable native configuration, but required base, source, applied, and
working state plus explicit conflict resolution. Reapplying on every Run would
make Run mutate configuration and overwrite interactive edits. Whole-file
replacement would be simpler but would discard unrelated native settings and,
for Codex TOML, comments and layout.

## Consequences

Profiles are reusable templates rather than continuously active state. Applying
the same Profile is idempotent, and rerunning after interruption converges.
Each changed native file is written through a temporary file and rename, but a
main configuration and auth file may be only partially applied if the process
stops between replacements. There is no backup, rollback, migration reader,
cross-file transaction, or cross-process lock. Callers must review Host Tenant
Profiles because application directly changes real host configuration.
