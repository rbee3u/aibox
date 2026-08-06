# Configs

The `config` command manages two distinct objects in one Tenant and Coding
Agent scope:

- A **Named Config** is a reusable, named set of fixed Config Fields.
- The **Current Config** is the complete native configuration read by the
  Coding Agent during a Run.

Config Application copies fixed fields from a Named Config into Current Config
once. It records no ongoing relationship between them.

Codex is the default Coding Agent:

```sh
aibox config list
aibox config create custom
aibox config get custom
aibox config edit custom
aibox config apply custom
aibox config get --current
aibox config edit --current
```

Select Claude and another Managed Tenant in the command scope:

```sh
aibox config --agent claude --tenant work create custom
aibox config --agent claude --tenant work edit --current
```

Use `--host` instead of `--tenant` to manage Configs for the Host Tenant.
`--host` and the runnable Managed Tenant named `host` are distinct and mutually
exclusive.

## Commands and Files

The Config commands are `list`, `get`, `create`, `edit`, `delete`, and `apply`.
`get` and `edit` require either a Named Config name or `--current`; the other
commands operate only on Named Configs. A Named Config called `current` remains
valid and is selected as a positional name, not with `--current`.

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

There is no Config metadata, association state, layout version, or migration
reader. Named Config catalogs are host-only and are never bind-mounted into a
Run.

`config create NAME` initializes a missing Managed Tenant and writes the
built-in template. A complete same-name Named Config is an error. If a safe
same-name directory is missing one expected Codex file, `create` validates the
prospective complete Named Config and adds only the missing template file. Unknown
entries, symlinks, unsafe modes, or invalid existing content are rejected.

`config list` returns only complete, structurally safe Named Configs. It does
not parse their contents, so an invalid but complete Named Config remains visible for
inspection or repair.

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

Named Config edits validate the edited file before committing. Main files must
contain only fixed fields with the documented types, and Codex `auth.json` must
be a JSON object. Files are validated independently so either invalid Codex
file can be repaired in its turn.

Current Config edits intentionally do not parse or validate content. They may
therefore save invalid TOML, JSON, or arbitrary bytes that the Coding Agent
later rejects. Editing initializes a missing Managed Tenant and Agent state
directory. A missing Claude file starts as `{}`, a missing Codex main file as
empty TOML, and a missing Codex auth file as `{}`. New files use mode `0600`;
existing file modes are preserved. Host Home directory modes are never changed.

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
| `model_provider` | string |
| `model_providers.custom.name` | string |
| `model_providers.custom.base_url` | string |
| `model_providers.custom.requires_openai_auth` | boolean |

The provider table name is fixed as `custom`. Codex Named Config `auth.json`
may be any JSON object and replaces the complete native Current Config
`auth.json` object during application.

## Built-in Templates

The Codex main template is:

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
    "ANTHROPIC_DEFAULT_FABLE_MODEL": "claude-fable-5[1m]"
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
Application is unconditional: it has no drift check, prompt, `--force`, backup,
deactivation, or restore operation. Runs consume Current Config without
consulting or reapplying Named Configs.

Missing native files are treated as semantically empty. An absent file whose
desired result is still empty remains absent. If an existing file becomes
empty, it remains present using a valid empty representation. Applying a Named
Config preserves existing Current Config file modes and creates files at mode
`0600`.

Each changed file is replaced atomically, but Codex main and auth replacement
is not atomic as a pair. Re-running the same application converges after an
interruption. aibox provides no cross-process locking, Config edit rollback,
transaction journal, backup, or restore. Host Tenant operations directly read
or change real host configuration and credentials.
