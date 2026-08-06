# Materialize Tenant-local Agent Profiles with roll-forward reconciliation

Status: superseded by
[ADR 0005](0005-one-time-fixed-field-config-application.md).

Each Agent Profile belongs to one Tenant and one Coding Agent. Activation
records the pre-activation Agent Configuration and exact applied Agent Profile,
then materializes native files; later source and working changes are reconciled
against that common applied state instead of being silently reapplied or reset.

## Considered Options

Cumulative deep merge could not identify an Active Agent Profile, safely switch
Agent Profiles, or distinguish user changes from source changes. Reapplying on
every Run would overwrite TUI changes. Backup-and-rollback transactions added
a second recovery model and could themselves fail midway.

## Consequences

Runs consume native Agent Configuration and only warn about divergence.
State-changing commands persist typed pending changes and roll them forward
idempotently after interruption; they do not accept arbitrary paths and do not
provide backup or restore. Same-path divergent edits require an explicit
Agent Profile-wins or configuration-wins choice.
