# Agent Profiles

An Agent Profile is a named set of arbitrary native Agent Configuration and
credentials owned by exactly one Tenant and one Coding Agent. The same name in
two Tenants, or in Codex and Claude, identifies independent Agent Profiles.
Each such scope has zero or one Active Agent Profile.

Agent Profile main settings use each Coding Agent's native configuration
format. Both built-in templates select non-interactive, unrestricted operation
inside the Docker boundary.

## Create and Activate

Codex is the default Coding Agent:

```sh
aibox profile create custom
aibox profile edit custom
aibox profile edit custom --auth
aibox profile activate custom
```

Select Claude and another Managed Tenant in the `profile` command scope:

```sh
aibox profile --agent claude --tenant work create custom
aibox profile --agent claude --tenant work edit custom
aibox profile --agent claude --tenant work edit custom --auth
aibox profile --agent claude --tenant work activate custom
```

Creating an Agent Profile initializes a missing Managed Tenant. Use `--host`
instead of `--tenant` to manage the Host Tenant's real Agent Configuration.
`--host` and `--tenant host` are distinct and mutually exclusive.

Creating an already complete and valid Agent Profile succeeds without replacing
its files. An incomplete or invalid same-name directory is reported rather than
repaired or overwritten. `profile edit` uses `$VISUAL`, then `$EDITOR`, and
falls back to `vim`. It edits a temporary file, validates the complete Agent
Profile, and replaces source only after the editor succeeds.

## Native Files

Each Agent Profile directory contains a native-format main file, a credential
source, and an aibox-owned sidecar:

| Coding Agent | Main configuration | Credentials | Metadata |
| --- | --- | --- | --- |
| Codex | `config.toml` | `auth.json` as a whole JSON object | `.metadata.json` |
| Claude | `settings.json` | `auth.json` as a JSON string map | `.metadata.json` |

Agent Profile `.metadata.json` contains reconciliation tombstones. It has no
layout or schema version field and should not be edited manually.

Claude credentials are materialized into `settings.json`'s `env` object. A key
cannot be declared in both Claude `settings.json.env` and Agent Profile `auth.json`.

Every Agent Profile file, including its `.metadata.json`, is mode `0600`. An
empty `auth.json` object adds no `/auth` ownership. Activation therefore
inherits the corresponding `base` credentials instead of materializing a new
credential value; an absent native auth file in `base` remains absent. Values
explicitly placed in the main configuration, including values under Claude
`settings.json.env`, remain ordinary `/config` ownership and are not covered by
this exception.

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

Its built-in `auth.json` credential source is:

```json
{
  "OPENAI_API_KEY": "sk-example"
}
```

This disables Codex approval prompts and its agent sandbox because Docker is
the Filesystem Sandbox. The template may be activated unchanged. aibox
validates its syntax and ownership but does not probe whether the configured
endpoint or model is available at runtime.

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

Its built-in `auth.json` credential source is:

```json
{
  "ANTHROPIC_AUTH_TOKEN": "sk-example"
}
```

This enables Claude's bypass-permissions mode and suppresses its dangerous-mode
prompt. The gateway-specific `…5[1m]` and Fable aliases are intentional. The
template may be activated unchanged; aibox does not probe endpoint or model
availability. It does not enable the status line. Neither built-in template
contains a usable credential: replace the `sk-example` placeholder in
`auth.json` before running the Coding Agent. During Claude activation, the
placeholder (or its replacement) is materialized as
`settings.json.env.ANTHROPIC_AUTH_TOKEN`.

## Agent Configuration and Activation

Agent Configuration means the real files read and modified by Codex or Claude:

| Coding Agent | Agent Configuration |
| --- | --- |
| Codex | `.codex/config.toml`, `.codex/auth.json` |
| Claude | `.claude/settings.json` |

Agent Configuration files materialized under Agent Profile ownership use mode
`0600`. When no status-line Component state is present, deactivation restores
every pre-activation file's original presence, bytes, and mode. When Component
state is installed, modified, or incomplete, its protected configuration paths
keep their current values while the main file's original mode is restored.
Existing Host Home directory modes are not changed.

Activation tracks four related states:

| State | Meaning |
| --- | --- |
| `base` | Exact Agent Configuration before first activation |
| `applied` | Exact Agent Profile definition used by the last successful transaction |
| `source` | Current Tenant-local Agent Profile files |
| `working` | Current native Agent Configuration, directly mutable by the user or TUI |

Agent Profile values are materialized over `base`. Every declared native path is
owned; scalars, arrays, and structural type replacements are atomic. Runs then
consume only `working`. They never mount Agent Profile source, inject credentials,
or reapply configuration.

Editing an Active Agent Profile changes `source` only. Editing native Agent
Configuration changes `working` only. A Run warns when either side has diverged
from `applied` and continues with `working` unchanged. If divergence inspection
itself fails, the Run warns and still continues with native Agent Configuration;
use a `profile` command to diagnose or repair the Agent Profile state.

Tenant Components are separate configuration owners. The status-line paths are:

| Component | Logical paths |
| --- | --- |
| Claude status line | `/config/statusLine` |
| Codex status line | `/config/tui/status_line`, `/config/tui/status_line_use_colors` |

When a status-line Component is installed, modified, or incomplete, these
paths do not participate in Agent Profile working diff, automatic adoption, or
base restoration. Their current values, including absence, survive activation,
reconciliation, switching, and deactivation.

Activating an Agent Profile that overlaps a present Component is rejected.
Installing a Component whose paths are owned by the Active Agent Profile is
also rejected. Inactive Agent Profiles may contain overlapping values, but
cannot activate until the Component is removed or the Agent Profile changes.

Activating another Agent Profile, or activating the same Agent Profile again,
starts from the original `base`. It refuses if `working` has drifted. Discard
that drift explicitly only when it is no longer needed:

```sh
aibox profile activate other --discard-config-changes
```

Deactivate to restore the pre-activation files and permissions, subject to the
status-line Component exception above:

```sh
aibox profile deactivate
aibox profile deactivate --discard-config-changes
```

Deactivating an inactive Tenant succeeds without changing anything. The
discard form is required when unreconciled working changes exist.

## Status and Diff

`profile status` prints `inactive`, or the Active Agent Profile name followed by
sorted path classifications:

```text
active custom
source-only /config/model
working-only /config/model_providers/custom/base_url
conflict /auth
```

An Active Agent Profile with no divergence prints `clean` after its name.

The classifications compare changes since `applied`:

| Classification | Meaning |
| --- | --- |
| `working-only` | Only native Agent Configuration changed |
| `source-only` | Only Agent Profile source changed |
| `both-same` | Both sides made the same change |
| `conflict` | Both sides changed the same atomic path differently |

Status never prints values. By default, `profile diff` prints only the side,
change classification (`added`, `removed`, or `modified`), and logical JSON
Pointer. Add `--show-values` to print old and new values for non-credential
paths. Paths under `/auth` remain redacted even with that option. Do not put
secrets in the main file if value output is unsafe for your terminal or logs.

```sh
aibox profile diff
aibox profile diff --show-values
```

`profile diff` also prints `clean` when both sides still match `applied`; it is
an error when there is no Active Agent Profile.

`profile get NAME` prints the main file. Credential output is deliberately
separate and explicit:

```sh
aibox profile get custom --auth
```

Both forms write unredacted file contents to standard output. In particular,
avoid `--auth` in shared terminals or commands whose output is captured in
logs. Secrets placed in the main configuration are likewise not redacted by
`profile get`.

## Reconciliation

Reconciliation performs a fresh three-way merge from `applied` on every
invocation:

- Working-only changes update Agent Profile source.
- Source-only changes update native Agent Configuration.
- Non-overlapping changes merge automatically.
- Identical changes on both sides are accepted.
- Divergent changes to one atomic path require an explicit choice.

Run the automatic merge with:

```sh
aibox profile reconcile
```

Conflicts are addressed with logical JSON Pointers, regardless of whether the
native main file is TOML or JSON:

```sh
aibox profile reconcile \
  --take-profile /config/model \
  --take-config /config/model_providers/custom/base_url
```

Use `--take-profile-all` or `--take-config-all` when one side should win every
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
`base`. If Agent Profile source stops declaring a value, reconciliation stops
owning it and reveals the corresponding `base` value. Users do not write
deletion metadata manually.

## Interrupted Operations

State-changing Agent Profile operations use the Agent/Tenant `.metadata.json`,
which is mode `0600`. Before modifying source or Agent Configuration, aibox
records a typed `pending` transaction while preserving the last committed
Active Agent Profile state. Each change can identify only a known Agent file,
Agent Profile directory, or Agent Profile file; it cannot contain an arbitrary
filesystem path.

Changes are applied idempotently in order. After all changes succeed, aibox
commits the requested Active Agent Profile state and removes `pending`. If a
process or host failure interrupts the operation, the record remains. The next
`profile` command resumes it. A Run also resumes pending work for its selected
Managed Tenant, and a status-line Component installation or removal resumes
work for its Coding Agent. Completion never performs recovery and remains
read-only.

This is roll-forward recovery, not rollback. aibox provides no Agent Profile
backup or restore command and no cross-process lock. Avoid changing the same
Tenant and Coding Agent from multiple aibox processes at once.

## Agent Profile Deletion

Delete explicit Agent Profiles or deliberately select all inactive Agent Profiles:

```sh
aibox profile delete old staging
aibox profile delete --all
```

An empty selection is an error, and `--all` cannot be combined with names.
Deletion asks for confirmation unless `--yes` is supplied. Explicitly naming
the Active Agent Profile is an error. `--all` keeps the Active Agent Profile and
deletes the inactive Agent Profiles. Missing explicitly named Agent Profiles are
ignored.

Agent Profile catalogs and Agent/Tenant metadata are host-only and are never
bind-mounted into a Run. See [Tenants](tenants.md#host-tenant) before modifying
real host Agent Configuration with `--host`.
