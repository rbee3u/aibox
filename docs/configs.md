# Configs

Tenant-scoped `config` commands manage two distinct objects in one Tenant and
Coding Agent scope:

- A **Named Config** is a reusable, named set of fixed Config Fields.
- The **Current Config** is the complete native configuration read by the
  Coding Agent during a Run.

Config Application copies fixed fields from a Named Config into Current Config
once. A successful Application records Last Application for live Config Drift
inspection, but never creates an active relationship or automatic
reapplication.

Credential Propagation is the explicit global exception: it can copy one newer
Host ChatGPT Credentials snapshot to older same-account Codex Configs without
creating or retaining any association.

The Console's Configs module is the primary interface. The following commands
remain available for one deprecation release; Codex is their default Coding
Agent:

```sh
aibox config list
aibox config create custom
aibox config get custom
aibox config edit custom
aibox config apply custom
aibox config get --current
aibox config edit --current
aibox config propagate-auth
```

Select Claude and another Managed Tenant in the command scope:

```sh
aibox config --agent claude --tenant work create custom
aibox config --agent claude --tenant work edit --current
```

Use `--host` instead of `--tenant` to manage Configs for the Host Tenant with a
Tenant-scoped command. `--host` and the runnable Managed Tenant named `host` are
distinct and mutually exclusive. `propagate-auth` is global and defaults its
source to Host/Codex/Current; it accepts redundant `--host`, `--agent codex`,
and `--current` selectors but rejects `--tenant` and `--agent claude`.

## Commands and Files

The Config commands are `list`, `get`, `create`, `edit`, `delete`, `apply`, and
`propagate-auth`. `get` and `edit` require either a Named Config name or
`--current`; the other Tenant-scoped commands operate only on Named Configs. A
Named Config called `current` remains valid and is selected as a positional
name, not with `--current`.

Named Configs use the direct catalog layout outside Tenant Home:

```text
$AIBOX_ROOT/<agent>/<tenant>/<config>/
$AIBOX_ROOT/<agent>/__host/<config>/
```

Each directory is mode `0700` and contains only the Agent's native mode `0600`
files:

| Coding Agent | Named and Current Config files |
| --- | --- |
| Claude | `settings.json` |
| Codex | `config.toml`, then `auth.json` |

Named Config directories contain no metadata, association state, layout
version, or migration reader. The enclosing Tenant-and-Agent catalog may also
contain one aibox-owned mode `0600` file:

```text
$AIBOX_ROOT/<agent>/<tenant-or-__host>/metadata.json
```

It is a 16 KiB-limited extensible observation document. Last Application owns
one strict section:

```json
{
  "last_application": {
    "applied": "custom",
    "applied_at": "2026-08-17T00:00:00Z"
  }
}
```

Other top-level sections are reserved for typed metadata in future aibox
versions and are preserved when Last Application changes. The document has no
layout version and is not a user or plugin extension surface. Named Config
catalogs and `metadata.json` are host-only and never bind-mounted into a Run.
An absent document or `last_application` section is Untracked; malformed or
unsafe metadata produces Comparison error and blocks Application before it
changes Current Config.

`config create NAME` initializes a missing Managed Tenant and writes the
built-in template. A complete same-name Named Config is an error. If a safe
same-name directory is missing one expected Codex file, `create` validates the
prospective complete Named Config and adds only the missing template file.
Unknown entries, symlinks, unsafe modes, or invalid existing content are
rejected.

The deprecated `config list` returns only complete, structurally safe Named
Configs. It does not parse their contents, so an invalid but complete Named
Config remains visible for inspection or repair. The Console also identifies
safe incomplete and invalid catalog entries so they can be repaired or deleted
explicitly.

`config get NAME` and `config get --current` print every expected native file
in the table order, with headings such as:

```text
==> config.toml <==
model = "gpt-5.6-sol"

==> auth.json <==
{"OPENAI_API_KEY":"secret"}
```

Output is raw and credentials are not redacted. A missing Current Config file
is shown with a `(missing)` heading while other files remain readable. A
missing Managed Tenant is an error. Named Config get requires a complete,
structurally safe directory. Neither form creates files.

`config edit NAME` and `config edit --current` open each expected file in
order, using `$VISUAL`, then `$EDITOR`, then `vim`. Each editor process finishes
before the next starts. A successful edit is committed immediately through a
temporary file and rename; a later cancellation, validation error, or editor
failure does not roll back an earlier file.

After every fully successful `config edit NAME`, including an edit that leaves
the bytes unchanged, aibox asks once whether to apply that Named Config to the
selected Coding Agent and Tenant Current Config when stdin is a terminal. The
prompt names the complete target and defaults to No. Only a case-insensitive
`y` or `yes` triggers Application. Other input, an empty line, or
EOF leaves Current Config unchanged and exits successfully. Non-interactive
edits silently skip the prompt. `config edit --current` and failed or partially
committed Named Config edits never prompt.

Confirming the prompt runs the same one-shot Config Application as `config
apply NAME`. An Application failure makes the edit command fail and reports
that the Named Config edit was already saved; neither the edit nor any Current
Config file already replaced by Application is rolled back.

Named Config edits validate the edited file before committing. Main files must
contain only fixed fields with the documented types, and Codex `auth.json` must
be a JSON object. Files are validated independently so either invalid Codex
file can be repaired in its turn.

Current Config edits intentionally do not parse or validate content. They may
therefore save invalid TOML, JSON, or arbitrary bytes that the Coding Agent
later rejects. Editing initializes a missing Managed Tenant and the selected
Agent state directory. In the Host Tenant, the real Host Home must already
exist, but editing may create its `.codex` or `.claude` directory. A missing
Claude file starts as `{}`, a missing Codex main file as empty TOML, and a
missing Codex auth file as `{}`. New files use mode `0600`; existing file modes
are preserved. Existing Host Home directory modes are never changed.

Operations that read Config content cap each native file at 16 MiB, including
`get`, `edit`, Application, and Credential Propagation. Reduce a larger file
outside aibox before using one of those operations; `config list` and deletion
do not parse its content.

Deletion requires names or `--all`; the forms are mutually exclusive and an
empty selection is an error. Both can remove safe invalid or incomplete Named
Config directories. Confirmation is required unless `--yes` is used. Deleting
a Named Config never changes Current Config.

## Config Fields

Named Config main files use native syntax but accept only the fields below.
Empty intermediate objects or tables are valid. Unknown fields and wrong
primitive types are errors. aibox does not validate model names, URLs, endpoint
availability, or Coding Agent enum values.

Claude `settings.json` supports:

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

Codex `config.toml` supports:

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

`openai_base_url` overrides the Base URL of Codex's built-in `openai` provider
and has no effect while `model_provider` selects `custom`. When it is unset,
Codex uses `https://chatgpt.com/backend-api/codex` for ChatGPT authentication
and `https://api.openai.com/v1` for API key authentication. The provider table
name for the separate custom Config Field group is fixed as `custom`. Codex
Named Config `auth.json` is one complete Config Field: it may be any JSON object
and replaces the complete native Current Config `auth.json` object during
application.

## Built-in Templates

The Codex main template is:

```toml
approval_policy = "never"
sandbox_mode = "danger-full-access"
model_reasoning_effort = "xhigh"
plan_mode_reasoning_effort = "xhigh"
model = "gpt-5.6-sol"
# ChatGPT authentication:
# openai_base_url = "https://chatgpt.com/backend-api/codex"
# API key authentication:
# openai_base_url = "https://api.openai.com/v1"
model_provider = "custom"

[model_providers.custom]
name = "custom"
base_url = "https://example.com/v1"
requires_openai_auth = true
```

Its `auth.json` is:

```json
{
  "OPENAI_API_KEY": "sk-example"
}
```

The Claude `settings.json` template is:

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "https://example.com",
    "ANTHROPIC_AUTH_TOKEN": "sk-example",
    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4-5",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-5[1m]",
    "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-5[1m]",
    "ANTHROPIC_DEFAULT_FABLE_MODEL": "claude-fable-5"
  },
  "permissions": {
    "defaultMode": "bypassPermissions"
  },
  "skipDangerousModePermissionPrompt": true
}
```

The templates configure non-interactive, unrestricted operation inside the
Docker Filesystem Sandbox. Their credentials are placeholders; replace them
before applying. The gateway-specific Claude aliases are intentional.

## Application Semantics

`config apply NAME` validates the complete Named Config and Current Config
before replacing any target file. It then processes every Config Field:

- A field present in the Named Config replaces the corresponding Current
  Config value.
- A field absent from the Named Config is removed from Current Config.
- A scalar or array that blocks a required parent object or table is replaced.
- Empty parent objects or tables left by removal are pruned.
- Values outside the fixed field set are preserved, including status-line
  Component configuration.

Codex application uses structure-preserving TOML edits so unrelated comments,
ordering, and formatting survive. Claude JSON is pretty-printed when changed.
The deprecated `config apply NAME` command is unconditional and has no prompt;
the Console Apply action runs the same operation. Application has no
precondition based on drift, `--force`, backup, deactivation, or restore
operation. Runs consume Current Config without consulting or reapplying Named
Configs.

After every native file replacement succeeds, aibox atomically replaces the
`last_application` metadata section with the Named Config name and timestamp.
If that final metadata write fails, Application reports the error without
rolling back Current Config files that were already replaced. The Console
derives:

- `Untracked` when no successful Application is recorded.
- `Clean` when applying the recorded Named Config now would leave fixed fields
  unchanged.
- `Dirty` when those fixed fields differ.
- `Source missing` when the recorded Named Config is no longer complete.
- `Comparison error` when a safe comparison cannot be made.

These states are observational. Editing a Named Config or Current Config never
applies, repairs, or reconciles another file automatically.

Missing native files are treated as semantically empty. An absent file whose
desired result is still empty remains absent. If an existing file becomes
empty, it remains present using a valid empty representation. Applying a Named
Config preserves existing Current Config file modes and creates files at mode
`0600`. When an Application has a file to write, it may create the selected
native Agent state directory at mode `0700` beneath the existing Home.

Each changed file is replaced atomically, but Codex main and auth replacement
is not atomic as a pair. Re-running the same application converges after an
interruption. aibox provides no cross-process locking, Config edit rollback,
transaction journal, backup, or restore. Host Tenant operations directly read
or change real host configuration and credentials.

## Credential Propagation

Codex can refresh native `auth.json` after ChatGPT sign-in. Run the following
after Host Current Config has refreshed to update older copies explicitly:

```sh
aibox config propagate-auth
aibox config --host --agent codex propagate-auth --current
```

Both forms use `$HOME/.codex/auth.json` as the source. The source must be a JSON
object with `auth_mode = "chatgpt"`, a nonempty `tokens.account_id`, and an RFC
3339 `last_refresh`. The command scans, without creating anything:

- every existing Managed Tenant Codex Current `auth.json`;
- every complete, structurally safe Managed Tenant Codex Named Config; and
- every complete, structurally safe Host Codex Named Config.

Non-ChatGPT and different-account credentials are ignored. Malformed candidate
content produces a warning. For the same account, an older target is replaced
with the source file's exact bytes; a JSON-equivalent target is `unchanged`
regardless of field order or formatting; equal timestamps with different JSON
values are a `conflict`; and a newer target is skipped with both timestamps
shown. `config.toml` is never read for eligibility or changed. Missing Current
Config files, incomplete Named Configs, and orphaned catalogs remain untouched.

The command validates the complete structural view before its first write.
Symlinks, unexpected entries, unsafe Named Config permissions, and read errors
make preflight fail with no changes. Each selected `auth.json` is then replaced
atomically in stable target order. Current Config preserves its existing mode;
Named Config remains mode `0600`. A write failure is reported and later targets
continue, the command exits nonzero, and successful earlier writes are not
rolled back.

Propagation uses the source and target snapshots captured during preflight. It
does not recheck content before each write, lock files, refresh credentials,
run automatically, or retain a synchronization relationship. Output identifies
targets as `tenant/<tenant>/current`, `tenant/<tenant>/config/<config>`, or
`host/config/<config>` and never prints tokens or account ids.
