# Agent Profiles

An Agent Profile is a named set of fixed native configuration values for one
Tenant and one Coding Agent. Applying a Profile updates the current Agent
Configuration once; the Profile is not active afterward and aibox records no
ongoing relationship between the two.

Codex is the default Coding Agent:

```sh
aibox profile create custom
aibox profile edit custom
aibox profile edit custom --auth
aibox profile apply custom
```

Select Claude and another Managed Tenant in the `profile` command scope:

```sh
aibox profile --agent claude --tenant work create custom
aibox profile --agent claude --tenant work apply custom
```

Use `--host` instead of `--tenant` to manage the real host Agent
Configuration. `--host` and the runnable Managed Tenant named `host` are
distinct and mutually exclusive.

## Commands and Catalog

The Profile commands are `list`, `get`, `create`, `edit`, `delete`, and
`apply`.

Profiles are stored outside Tenant Home under the direct catalog layout:

```text
$AIBOX_ROOT/<agent>/<tenant>/<profile>/
$AIBOX_ROOT/<agent>/__host/<profile>/
```

Every Profile directory is mode `0700` and contains exactly two mode `0600`
files:

| Coding Agent | Main configuration | Credentials |
| --- | --- | --- |
| Codex | `config.toml` | `auth.json` |
| Claude | `settings.json` | `auth.json` |

There is no Profile metadata or Tenant/Agent state file. Profile catalogs are
host-only and are never bind-mounted into a Run.

`profile create NAME` initializes a missing Managed Tenant and writes the
built-in template. A complete same-name Profile is an error. If a safe
same-name directory contains only one of the two expected files, `create`
validates the prospective complete Profile and adds the missing template file
without replacing the existing one. Unknown entries, symlinks, unsafe modes,
or invalid existing content are rejected.

`profile list` returns only complete, structurally safe Profiles. It does not
parse their contents, so a complete Profile with invalid syntax or fields
remains visible. `profile get` prints the selected raw file, and `profile edit`
opens a temporary copy using `$VISUAL`, then `$EDITOR`, then `vim`. An edit is
committed only when the complete Profile validates, allowing invalid content
to be repaired. `profile get NAME --auth` prints credentials without redaction.

Deletion requires names or `--all`; the two forms are mutually exclusive and
an empty selection is an error. Both forms can remove safe invalid or
incomplete Profile directories. Confirmation is required unless `--yes` is
used. Deleting a Profile never changes Agent Configuration.

## Fixed Profile Fields

Profile main files use the Coding Agent's native syntax, but only the fields
below are accepted. Empty intermediate objects or tables are valid. Unknown
fields and wrong primitive types are errors. aibox validates strings and
booleans but does not validate model names, URLs, endpoint availability, or
Coding Agent enum values.

Claude `settings.json` supports these fields:

| Field | Type |
| --- | --- |
| `env.ANTHROPIC_BASE_URL` | string |
| `env.ANTHROPIC_DEFAULT_HAIKU_MODEL` | string |
| `env.ANTHROPIC_DEFAULT_SONNET_MODEL` | string |
| `env.ANTHROPIC_DEFAULT_OPUS_MODEL` | string |
| `env.ANTHROPIC_DEFAULT_FABLE_MODEL` | string |
| `permissions.defaultMode` | string |
| `skipDangerousModePermissionPrompt` | boolean |

Claude Profile `auth.json` may be `{}` or contain the single string field
`ANTHROPIC_AUTH_TOKEN`. Application maps it to
`settings.json.env.ANTHROPIC_AUTH_TOKEN`; omission removes only that key and
preserves other environment variables.

Codex `config.toml` supports these fields:

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

The provider table name is fixed as `custom`. Codex Profile `auth.json` may be
any JSON object and replaces the complete native Codex `auth.json` object.

## Built-in Templates

The Codex template is:

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

The Claude template is:

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

Its `auth.json` is:

```json
{
  "ANTHROPIC_AUTH_TOKEN": "sk-example"
}
```

The templates configure non-interactive, unrestricted operation inside the
Docker Filesystem Sandbox. Their example credentials are not usable; replace
them before applying. The gateway-specific Claude aliases are intentional.

## Application Semantics

`profile apply NAME` validates the complete Profile and current Agent
Configuration before replacing any target file. It then processes every fixed
Profile Field:

- A field present in the Profile replaces the corresponding Agent
  Configuration value.
- A field absent from the Profile is removed from Agent Configuration.
- A scalar or array that blocks a required parent object/table is replaced.
- Empty parent objects/tables left by removal are pruned.
- All values outside the fixed field set are preserved, including status-line
  Component configuration.

Codex application uses structure-preserving TOML edits so unrelated comments,
ordering, and formatting survive. Claude JSON is pretty-printed when changed.
Application is unconditional: it has no drift check, prompt, `--force`, backup,
deactivation, or restore operation. Runs consume the resulting native files
without consulting or reapplying Profiles.

Missing `settings.json`, `config.toml`, and `auth.json` are treated as empty.
An absent file whose desired result is still empty remains absent. If an
existing file becomes empty, it remains present using a valid empty
representation. Newly created Agent Configuration files use mode `0600`;
existing file modes are preserved, including in the Host Tenant. Users are
responsible for ensuring an existing mode is sufficiently private for
credentials.

Each changed file is replaced atomically using a temporary file. There is no
transaction journal or cross-file atomicity: a process interruption can leave
Codex `config.toml` and `auth.json` at different stages. Re-running the same
`profile apply` converges both files. aibox provides no cross-process locking
guarantee; avoid modifying the same Tenant and Coding Agent concurrently.
