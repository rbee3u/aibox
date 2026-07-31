# Providers

Providers are host-side configuration overlays for custom Codex or Claude API
setups. Creating a provider does not affect an agent run. The configuration
becomes active only after an explicit `provider apply`.

## Codex Workflow

Codex is the default agent for provider commands:

```sh
aibox provider create custom
aibox provider edit custom
aibox provider edit custom --auth
aibox provider apply custom
aibox provider list
```

A Codex provider contains `config.toml` and `auth.json`. The built-in template
targets a custom Responses-compatible endpoint and contains an
`OPENAI_API_KEY` placeholder. Replace every `sk-example` value before applying;
aibox rejects placeholder credentials.

`provider edit` opens the main config. Add `--auth` to edit `auth.json`.

## Claude Workflow

Select Claude within the provider command scope:

```sh
aibox provider --agent claude create custom
aibox provider --agent claude edit custom
aibox provider --agent claude apply custom
aibox provider --agent claude list
```

A Claude provider contains `settings.json`. Its template includes custom
`ANTHROPIC_*` environment variables, the bundled status line, and unrestricted
permission mode. Replace its placeholder credential before applying. Claude
does not have a separately managed auth file, so it does not accept
`provider edit --auth`.

For either agent, `provider edit` uses `$VISUAL`, then `$EDITOR`, and falls back
to `vim`.

## Active Configuration

An apply writes these files into the selected profile's active agent state:

| Agent | Main config | Auth |
| --- | --- | --- |
| Codex | `home/.codex/config.toml` | `home/.codex/auth.json` |
| Claude | `home/.claude/settings.json` | No separately managed file |

Runs consume the active files left by the last apply. They do not mount the
provider snapshot or reapply it, so later edits made by the agent or user stay
active until another operation changes them.

Apply is cumulative:

- TOML tables and JSON objects merge recursively into the current active main
  config.
- Scalars and arrays replace existing values.
- Keys not mentioned by the provider remain in the active config.
- Codex `auth.json` is validated and replaced as a whole instead of merged.

The `*` printed by `provider list` marks the last provider applied successfully.
It does not mean the active config contains only that provider's keys.

## Removing Active Keys

Use dotted paths in `aibox.provider.apply.remove` when an apply must remove keys
left by an earlier provider or edit.

For Codex TOML:

```toml
[aibox.provider.apply]
remove = ["model_provider", "model_providers.custom"]
```

For Claude JSON:

```json
{
  "aibox": {
    "provider": {
      "apply": {
        "remove": ["some.setting"]
      }
    }
  }
}
```

The reserved top-level `aibox` table or object is apply metadata and is not
written into the active agent config.

## Permission Mode

New provider templates configure the selected agent for unrestricted operation
inside the container: Codex uses `approval_policy = "never"` with
`sandbox_mode = "danger-full-access"`, and Claude uses bypass-permissions mode.

This is optional behavior, not a property of every aibox run. Edit the provider
before applying, or edit the active agent config afterward, if you want the
agent to retain its own permission prompts or sandbox.

The Docker boundary still allows networking, writable managed mounts, and any
remote authority granted by credentials. Read [Sandbox and Mounts](sandbox.md)
before disabling agent-level restrictions.

## Secrets, Backups, and Deletion

`aibox provider get <provider>` prints every managed provider file, including
Codex `auth.json`. Treat the output as secret.

Before an apply replaces existing active files, aibox copies them to:

```text
$AIBOX_ROOT/<profile>/provider/<agent>/.backup/<timestamp>/
```

The latest 20 generated backups are retained. The first apply creates no empty
backup when the active files do not exist yet.

There is no restore command. Stop runs using the profile, then copy the desired
managed files back into `home/.codex/` or `home/.claude/`. For the `host`
profile, restore them into the real host agent directory. Keep Codex
`auth.json` readable only by its owner.

Delete saved provider overlays with:

```sh
aibox provider delete custom
aibox provider delete --all
```

Deletion asks for confirmation unless `--yes` is supplied. Deleting a provider
does not roll back configuration already applied to the active agent files.

See [Profiles](profiles.md#the-host-profile) before applying providers to the
real host agent configuration.
