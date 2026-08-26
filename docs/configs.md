# Configs

The Console Configs module is the only management surface for Named Configs and
Current Config. The `aibox run` command consumes Current Config from the selected
Managed Tenant; it never reads or reapplies a Named Config.

## Tenant

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
Config directory contains no Tenant marker or management metadata.

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
when its native content is valid. Visual Config Options are sourced from the
fixed `AgentKind` Config Field contract and use compact label-and-control rows.
Native paths remain in Raw. A help icon exposes each Option's description on
hover or keyboard focus, and required Options use an accessible `*` marker.
Required Config Fields cannot be omitted. Optional free-text Options retain an
**Include** control, optional enums use **Default** to omit the field, and
optional booleans use **Default**, **Enabled**, or **Disabled**. Enum Options
are closed to their declared values. An existing unknown string remains
selectable as **Unsupported** so Visual can preserve it, but Visual cannot
create a new unknown value. Codex custom-provider fields stay included while
the Custom provider is enabled. Omitting another Config Field makes Config
Application remove it from Current Config. Sensitive Options use a masked input
with an explicit reveal control.
Codex Visual treats Custom provider as one optional aggregate. Disabled omits
both provider tables. Enabled immediately supplies `custom`,
`https://example.com/v1`, and `requires_openai_auth = true`. Saving an enabled
Custom provider creates the fixed `sk-example` placeholder in a missing or
empty Named Config `auth.json`; existing credentials are never overwritten.
The main Codex file may still open in Visual when auth is missing or malformed,
so that Raw repair and Visual provider editing remain independent.

**Raw Editor** remains available for Named Config and is the only editor for
Current Config. Codex shows `config.toml` above `auth.json` at a fixed 2:1 ratio;
each file scrolls, diagnoses, tracks revisions, and saves independently. Visual
mode keeps a separate Codex credentials section below the main fields. ChatGPT
credentials may be inspected in Raw and explicitly converted to an API-key
object; the conversion remains local until `auth.json` is saved. A non-UTF-8
Current Config is read-only in the Console and can be downloaded as its original
bytes. Leaving a Config or Tenant guards all dirty files, and Save-and-continue
commits them in Agent-defined order without rollback.

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
statusline configuration or custom provider tables. The write is atomic per
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
directory, or Run History. A missing read-only Tenant stays quiet and creates no
directories. Service startup is the separate lifecycle operation that ensures
the Default Managed Tenant baseline. The Console is the management boundary;
the public CLI is limited to `console`, `run`, and the Tenant-only `debug`
shell.

## Config Fields

Named Config main files use native syntax but project only these fixed fields.
Unknown fields are warnings and remain in the native source; wrong primitive
types are errors. Model names, URLs, and endpoint availability are not resolved
by aibox. Visual enum values are the closed sets declared by `AgentKind`; Raw
may preserve unknown string values. Codex `approval_policy` is currently limited
to `untrusted`, `on-request`, or `never`; the native granular object form is not
supported by Named Config validation.

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
| Custom provider | optional aggregate |
| `model_providers.custom.name` | string, default `custom` |
| `model_providers.custom.base_url` | string, default `https://example.com/v1` |

Codex Named Config either omits `model_provider` and `model_providers` to use
the official OpenAI default, or contains only `model_provider = "custom"` and
the exact three-key `model_providers.custom` table. The table's
`requires_openai_auth` is always `true`; Raw mode exposes it for repair but the
current version rejects `false`. Codex
`auth.json` is one complete Config Field:
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
installing a statusline.

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
