# Providers

A Provider is a reusable set of connection configuration and credentials owned
by exactly one Tenant and one Coding Agent. The same name in two Tenants, or in
Codex and Claude, identifies independent Providers.

Provider settings are native Agent Configuration. Both default templates select
non-interactive, unrestricted operation inside the Docker boundary.

## Create and Activate

Codex is the default Coding Agent:

```sh
aibox provider create custom
aibox provider edit custom
aibox provider edit custom --auth
aibox provider activate custom
```

Select Claude and another Managed Tenant in the Provider command scope:

```sh
aibox provider --agent claude --tenant work create custom
aibox provider --agent claude --tenant work edit custom
aibox provider --agent claude --tenant work edit custom --auth
aibox provider --agent claude --tenant work activate custom
```

Creating a Provider initializes a missing Managed Tenant. Use `--host` instead
of `--tenant` to manage the Host Tenant's real Agent Configuration. `--host`
and `--tenant host` are distinct and mutually exclusive.

Creating an already complete and valid Provider succeeds without replacing its
files. `provider edit` uses `$VISUAL`, then `$EDITOR`, and falls back to `vim`.
It edits a temporary file, validates the complete Provider, and replaces source
only after the editor succeeds.

## Native Files

Each Provider directory contains native files plus an aibox-owned sidecar:

| Coding Agent | Main configuration | Credentials | Metadata |
| --- | --- | --- | --- |
| Codex | `config.toml` | `auth.json` as a whole JSON object | `.metadata.json` |
| Claude | `settings.json` | `auth.json` as a JSON string map | `.metadata.json` |

Provider `.metadata.json` contains reconciliation tombstones. It has no layout
or schema version field and should not be edited manually.

Claude credentials are materialized into `settings.json`'s `env` object. A key
cannot be declared in both Claude `settings.json.env` and Provider `auth.json`.

Every Provider `auth.json` is mode `0600`. An empty `{}` means the Provider
does not own credentials: existing Agent Configuration credentials remain
unchanged, and an absent native auth file remains absent.

The built-in Codex template is:

```toml
approval_policy = "never"
sandbox_mode = "danger-full-access"
model_reasoning_effort = "xhigh"
plan_mode_reasoning_effort = "xhigh"
model = "gpt-5.6-sol"
model_provider = "custom"

[model_providers.custom]
base_url = "https://example.com/v1"
requires_openai_auth = true
```

This disables Codex approval prompts and its agent sandbox because Docker is
the Filesystem Sandbox. Edit the Provider before activation when a different
model, endpoint, or policy is required.

The built-in Claude template is:

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "https://example.com",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4-5",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-5[1m]",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-5[1m]",
    "ANTHROPIC_DEFAULT_FABLE_MODEL": "claude-fable-5[1m]"
  },
  "permissions": {
    "defaultMode": "bypassPermissions"
  },
  "skipDangerousModePermissionPrompt": true
}
```

This enables Claude's bypass-permissions mode and suppresses its dangerous-mode
prompt. Edit the Provider before activation when a different endpoint, model,
or permission policy is required. It does not enable the status line. Neither
built-in template contains credentials.

## Agent Configuration and Activation

Agent Configuration means the real files read and modified by Codex or Claude:

| Coding Agent | Agent Configuration |
| --- | --- |
| Codex | `.codex/config.toml`, `.codex/auth.json` |
| Claude | `.claude/settings.json` |

Activation tracks four related states:

| State | Meaning |
| --- | --- |
| `base` | Exact Agent Configuration before first activation |
| `applied` | Exact Provider definition used by the last successful transaction |
| `source` | Current Tenant-local Provider files |
| `working` | Current native Agent Configuration, directly mutable by the user or TUI |

Provider values are materialized over `base`. Every declared native path is
owned; scalars, arrays, and structural type replacements are atomic. Runs then
consume only `working`. They never mount Provider source, inject credentials,
or reapply configuration.

Editing an Active Provider changes `source` only. Editing native Agent
Configuration changes `working` only. A Run warns when either side has diverged
from `applied` and continues with `working` unchanged.

Status-line Components also edit native Agent Configuration directly. They do
not update Provider source, base snapshots, or Active Provider metadata. When a
Component is installed after activation, a later Provider activation or
deactivation may replace it according to the existing base and reconciliation
rules; install it before activation when it must be part of the base.

Activating another Provider, or activating the same Provider again, starts
from the original `base`. It refuses if `working` has drifted. Discard that
drift explicitly only when it is no longer needed:

```sh
aibox provider activate other --discard-config-changes
```

Deactivate to restore the exact pre-activation files and permissions:

```sh
aibox provider deactivate
aibox provider deactivate --discard-config-changes
```

Deactivating an inactive Tenant succeeds without changing anything. The
discard form is required when unreconciled working changes exist.

## Status and Diff

`provider status` prints `inactive`, or the Active Provider name followed by
sorted path classifications:

```text
active custom
source-only /config/model
working-only /config/model_providers/custom/base_url
conflict /auth
```

The classifications compare changes since `applied`:

| Classification | Meaning |
| --- | --- |
| `working-only` | Only native Agent Configuration changed |
| `source-only` | Only Provider source changed |
| `both-same` | Both sides made the same change |
| `conflict` | Both sides changed the same atomic path differently |

Status never prints values. `provider diff` prints applied-to-working and
applied-to-source old/new values. Main configuration values are shown raw;
credential paths under `/auth` are always redacted. Do not put secrets in the
main file if raw diff output is unsafe for your terminal or logs.

`provider get NAME` prints the main file. Credential output is deliberately
separate and explicit:

```sh
aibox provider get custom --auth
```

## Reconciliation

Reconciliation performs a fresh three-way merge from `applied` on every
invocation:

- Working-only changes update Provider source.
- Source-only changes update native Agent Configuration.
- Non-overlapping changes merge automatically.
- Identical changes on both sides are accepted.
- Divergent changes to one atomic path require an explicit choice.

Run the automatic merge with:

```sh
aibox provider reconcile
```

Conflicts are addressed with logical JSON Pointers, regardless of whether the
native main file is TOML or JSON:

```sh
aibox provider reconcile \
  --take-provider /config/model \
  --take-config /config/model_providers/custom/base_url
```

Use `--take-provider-all` or `--take-config-all` when one side should win every
current conflict. A selector that is not a current conflict is rejected, as is
supplying opposite choices for one path.

Examples of logical paths:

```text
/config/model_provider
/config/model_providers/custom/base_url
/auth
/auth/ANTHROPIC_AUTH_TOKEN
```

Codex credential ownership is whole-file at `/auth`. Claude credential keys
are independently mergeable under `/auth/<key>`. JSON Pointer escaping applies:
`~1` represents `/` and `~0` represents `~` inside a key.

If working Agent Configuration deletes an owned value, successful
reconciliation records an internal tombstone so the deletion continues to mask
`base`. If Provider source stops declaring a value, reconciliation stops owning
it and reveals the corresponding `base` value. Users do not write deletion
metadata manually.

## Interrupted Operations

State-changing Provider operations use the Agent/Tenant `.metadata.json`, which
is mode `0600`. Before modifying source or Agent Configuration, aibox records a
typed `pending` transaction while preserving the last committed Active Provider
state. Each change can identify only a known Agent file, Provider directory, or
Provider file; it cannot contain an arbitrary filesystem path.

Changes are applied idempotently in order. After all changes succeed, aibox
commits the requested Active Provider state and removes `pending`. If a process
or host failure interrupts the operation, the record remains and the next
Provider command resumes it. A Run also resumes pending work for its selected
Managed Tenant. Completion never performs recovery and remains read-only.

This is roll-forward recovery, not rollback. aibox provides no Provider backup
or restore command and no cross-process lock. Avoid changing the same Tenant
and Coding Agent from multiple aibox processes at once.

## Provider Deletion

Delete explicit Providers or deliberately select all inactive Providers:

```sh
aibox provider delete old staging
aibox provider delete --all
```

An empty selection is an error, and `--all` cannot be combined with names.
Deletion asks for confirmation unless `--yes` is supplied. Explicitly naming
the Active Provider is an error. `--all` keeps the Active Provider and deletes
the inactive Providers. Missing explicitly named Providers are ignored.

Provider catalogs and Agent/Tenant metadata are host-only and are never
bind-mounted into a Run. See [Tenants](tenants.md#host-tenant) before modifying
real host Agent Configuration with `--host`.
