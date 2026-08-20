# Configs

The Console Configs module is the only management surface for Named Configs and
Current Config. The `aibox run` command consumes Current Config from the selected
Managed Tenant; it never reads or reapplies a Named Config.

## Scope

Every Config belongs to one Tenant and one Coding Agent. Select the Managed
Tenant or Host Tenant and the Agent in the Console before opening Configs.

- A **Named Config** is a reusable set of the fixed Config Fields defined by
  `AgentKind`.
- **Current Config** is the native file set that the Coding Agent reads during
  a Run or a host-side Session operation.
- **Config Application** is an explicit, one-shot projection of a Named Config
  into Current Config.
- **Config Drift** compares Current Config with the Named Config recorded as
  Last Application. It does not reconcile or reapply anything.

The Host Tenant uses the real Host Home. Host Config reads expose native
credentials without redaction, so treat every Host operation as a direct edit of
your account state.

## Native Files

Claude Named Configs contain only `settings.json`. Codex Named Configs contain
only `config.toml` and `auth.json`. The Agent contract defines file order,
templates, empty Current Config content, and the fixed Config Fields. A Named
Config directory contains no scope marker or management metadata.

Config files are native text. Claude files are JSON and Codex main files are
TOML; Codex `auth.json` is a complete JSON object and replaces the native auth
file as a whole. Named Config files are mode `0600` and their directories are
mode `0700`.

## Create And Edit

Use the Configs module to create a Named Config from the built-in Agent template,
then reveal or edit each native file in Agent-defined order. Reveal includes all
credential bytes. Named Config writes validate the selected file before commit.
Current Config writes preserve arbitrary bytes without syntax validation and may
initialize a missing Managed Tenant or Agent state directory.

The detail editor has two modes. A complete, safe Named Config main file
(`settings.json` for Claude or `config.toml` for Codex) opens in **Visual Editor**
when its native content is valid. Visual fields are sourced from the fixed
`AgentKind` Config Field contract and are grouped with a friendly label, native
path, description, and an **Include** switch. Turning Include off omits that
Config Field; Config Application then removes the field from Current Config.
Included empty strings and custom values are valid. Suggested values in a select
are convenience choices, not a closed enum. Sensitive fields use a masked input
with an explicit reveal control. Safe incomplete Named Config files remain
Raw-only until the Config is repaired.

**Raw Editor** remains available for supported Named Config main files and is the
only editor for Current Config and Codex `auth.json`. It uses native JSON/TOML
syntax highlighting and debounced backend diagnostics. Diagnostics prevent a
switch from Raw to Visual but do not change Current Config's arbitrary-byte save
semantics. A non-UTF-8 Current Config is read-only in the Console and can be
downloaded as its original bytes. Switching modes, files, Configs, or scopes
uses the existing unsaved-change confirmation.

Each file is committed independently. If a later file fails, an earlier file is
not rolled back. Existing file modes are preserved for Current Config; newly
created Current Config files use mode `0600`.

The Configs view reports `ready`, `incomplete`, or `invalid` for each catalog
entry. Incomplete entries can be repaired from the built-in template. Unknown
entries are ignored while listing, but an explicitly selected unsafe entry is an
error.

## Config Application

Applying a Named Config sets every present fixed field, removes fixed fields
that are absent from the source, and preserves unrelated native settings such as
status-line configuration or custom provider tables. The write is atomic per
file, not across files. Repeating the operation converges.

After all changed files are committed, a strict `last_application` section is
written to the Agent catalog-root `metadata.json`. It stores the source name and
timestamp only. This record is observational provenance, not an active binding,
rollback point, or reconciliation state.

The Console derives these statuses from that record:

- `untracked`: no successful application is recorded;
- `clean`: Current Config matches the recorded source's fixed fields;
- `dirty`: one or more fixed fields differ;
- `source-missing`: the recorded Named Config no longer exists;
- `comparison-error`: the source or Current Config cannot be safely compared.

## Credential Propagation

Credential Propagation is separate from Config Application. From the Configs
module, preview and explicitly execute one snapshot of Host Codex Current Config
`auth.json`. The snapshot is copied only to older existing same-account ChatGPT
Credentials in complete safe Named Configs and Managed Tenant Current Configs.

It creates nothing, stores no relationship, and never runs automatically. Other
providers, different accounts, equal timestamps with different content, newer
targets, malformed candidates, and unsafe filesystem structures are reported or
rejected according to the preview. The plan is snapshotted before writes;
individual target failures do not roll back successful targets.

## Layout And Safety

Named Config catalogs use the direct layout:

```text
$AIBOX_ROOT/
  claude/<tenant>/<name>/settings.json
  codex/<tenant>/<name>/config.toml
  codex/<tenant>/<name>/auth.json
  claude/<tenant>/metadata.json
  codex/<tenant>/metadata.json
```

The Host Tenant uses `__host` as the catalog key. Host-side reads, writes, and
deletions reject symlinks, unexpected entries, unsafe ancestors, and files over
the configured size limits. `metadata.json` is mode `0600` and limited to 16
KiB; unknown top-level metadata sections are preserved when a known section is
updated.

There is no activation state, migration reader, backup, rollback, lock
directory, or Run History. A missing read-only scope stays quiet and creates no
directories. Service startup is the separate lifecycle operation that ensures
the Default Managed Tenant baseline. The Console is the management boundary;
the public CLI remains limited to `serve`, `run`, and `build`.

## Config Fields

Named Config main files use native syntax but accept only these fixed fields.
Unknown fields and wrong primitive types are errors; model names, URLs, endpoint
availability, and provider enum values are not resolved by aibox.

Claude `settings.json` fields:

| Field | Type |
| --- | --- |
| `env.ANTHROPIC_BASE_URL` | string |
| `env.ANTHROPIC_AUTH_TOKEN` | string |
| `env.ANTHROPIC_DEFAULT_HAIKU_MODEL` | string |
| `env.ANTHROPIC_DEFAULT_SONNET_MODEL` | string |
| `env.ANTHROPIC_DEFAULT_OPUS_MODEL` | string |
| `env.ANTHROPIC_DEFAULT_FABLE_MODEL` | string |
| `permissions.defaultMode` | string |
| `skipDangerousModePermissionPrompt` | boolean |

Codex `config.toml` fields:

| Field | Type |
| --- | --- |
| `approval_policy` | string |
| `sandbox_mode` | string |
| `model_reasoning_effort` | string |
| `plan_mode_reasoning_effort` | string |
| `model` | string |
| `openai_base_url` | string |
| `model_provider` | string |
| `model_providers.custom.name` | string |
| `model_providers.custom.base_url` | string |
| `model_providers.custom.requires_openai_auth` | boolean |

`openai_base_url` controls Codex's built-in `openai` provider and has no effect
while `model_provider` selects `custom`. The provider table name for the fixed
custom field group is `custom`. Codex `auth.json` is one complete Config Field:
it may be any JSON object and replaces the complete native Current Config object
during Application.

## Built-In Templates

The Console creates these native templates. Credentials are placeholders and
must be replaced before applying them.

Codex `config.toml`:

```toml
approval_policy = "never"
sandbox_mode = "danger-full-access"
model_reasoning_effort = "xhigh"
plan_mode_reasoning_effort = "xhigh"
model = "gpt-5.6-sol"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://example.com/v1"
requires_openai_auth = true
```

Codex `auth.json` is an API-key object. Claude's `settings.json` template sets
the native `ANTHROPIC_*` environment fields, `permissions.defaultMode` to
`bypassPermissions`, and `skipDangerousModePermissionPrompt` to `true` without
installing a status line.

## Application Details

Application validates the complete Named Config and Current Config before
changing files. A present field replaces the matching Current Config value; an
absent field is removed. Scalars or arrays that block a required parent object
or table are replaced, while values outside the fixed field set are preserved.
Codex edits preserve unrelated TOML comments and ordering; changed Claude JSON
is pretty-printed.

Missing native files are semantically empty. A desired empty result remains
absent when the file was absent; an existing file that becomes empty remains
present using its native empty representation. Current Config modes are
preserved, and newly created files use `0600`. Each changed file is atomic, but
the main and auth files are not an atomic pair. If the final metadata write
fails, already replaced files remain replaced.

## Propagation Details

The Host source must be a JSON object with `auth_mode` `chatgpt`, a nonempty
`tokens.account_id`, and an RFC 3339 `last_refresh`. The Console scans existing
Managed Tenant Current `auth.json`, complete safe Managed Named Configs, and
complete safe Host Named Configs without creating anything. `config.toml` is
never read for eligibility or changed.

Non-ChatGPT and different-account credentials are ignored. Older same-account
targets receive the source bytes; equivalent content is `unchanged`, equal
timestamps with different values are `conflict`, and newer targets are skipped.
Malformed candidates are reported. Preflight validates the full structural view
before any write; each selected auth file is then replaced independently in
stable order, and a failed target does not roll back earlier successes.
