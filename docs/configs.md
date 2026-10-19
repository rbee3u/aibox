# Configs

The Console Configs module manages Named Configs and Current Config. A Run uses
only Current Config from its selected Managed Tenant; it never reads or
reapplies a Named Config.

## Model and Native Files

Every Config belongs to one Tenant and Agent. The Host Tenant operates
directly on the real Host Home, including unredacted credentials.

`AgentKind` defines each Named Config's fixed Config Fields and native files.
See [Domain Language](../GLOSSARY.md#configuration) for Config terminology.

| Agent | Named Config files | Native role |
| --- | --- | --- |
| Claude | `settings.json` | JSON settings, including endpoint and credential environment values |
| Codex | `config.toml`, `auth.json` | TOML settings and one complete JSON auth object |

Codex `auth.json` is one complete Config Field and is replaced as a whole. The
Agent contract defines file order, built-in templates, empty content, and
supported fields. Named Config directories contain no management metadata.
Templates use native unrestricted, non-interactive settings and credential
placeholders that must be reviewed before application.

## Claude Authentication

Claude Named Configs accept at most one explicit credential in `settings.json`:

| Config Field under `env` | Native authentication |
| --- | --- |
| `CLAUDE_CODE_OAUTH_TOKEN` | Claude subscription OAuth token from `claude setup-token` |
| `ANTHROPIC_API_KEY` | Claude Console API key for API usage billing |
| `ANTHROPIC_AUTH_TOKEN` | Custom Bearer credential, commonly for a custom endpoint |

Each included credential must be a nonempty, non-whitespace string. AIBox does
not infer credential types from prefixes, check account eligibility, or validate
credentials online. The same rules apply to Named Config Visual and Raw saves
and Application. Current Config remains Raw-only and accepts arbitrary native
content, including multiple credentials.

`ANTHROPIC_BASE_URL` is optional and independent of credential selection. Omit
it to use Claude's native default endpoint, or include it to route through a
custom endpoint or the [Request Proxy](requests.md#setup). An endpoint override
alone does not replace subscription authentication.

### Subscription Token

Generate a token with the official Claude Code CLI's `claude setup-token`, then
copy its terminal output into `env.CLAUDE_CODE_OAUTH_TOKEN`. With the Runtime
Image built and the Claude Component installed in a Managed Tenant, the same
command can run inside its Filesystem Sandbox:

```sh
aibox run --agent claude --tenant work -- setup-token
```

The command opens Anthropic's browser authorization flow; if the container's
browser callback is unreachable, paste the displayed authorization code back
into its terminal. AIBox does not implement a login flow or acquire the token.
The official documentation describes a one-year token for Pro, Max, Team, or
Enterprise subscriptions. Requests use that subscription's limits and billing
rules. This is distinct from a browser cookie or website session token.

```json
{
  "env": {
    "CLAUDE_CODE_OAUTH_TOKEN": "replace-with-setup-token-output"
  },
  "permissions": { "defaultMode": "default" }
}
```

The configured token is fixed, not automatically refreshed by AIBox. If it
expires or is revoked, generate a replacement, save and apply it, then restart
Claude. It supports model requests but not Remote Control or claude.ai
connectors; local MCP servers still work. Claude's `--bare` mode does not read
this variable. See [Claude's token documentation](https://code.claude.com/docs/en/authentication#generate-a-long-lived-token).

### API Key and Custom Authentication

For official API usage billing, obtain a key from Claude Console:

```json
{
  "env": { "ANTHROPIC_API_KEY": "replace-with-console-api-key" },
  "permissions": { "defaultMode": "default" }
}
```

Existing custom Bearer configurations remain supported, and new Named Configs
retain the custom endpoint and credential placeholders:

```json
{
  "env": {
    "ANTHROPIC_BASE_URL": "https://gateway.example",
    "ANTHROPIC_AUTH_TOKEN": "replace-with-gateway-token"
  },
  "permissions": { "defaultMode": "default" }
}
```

### Native Login and Credential Switching

Omit all three credentials to let Claude Code use its native authentication:

```json
{
  "permissions": { "defaultMode": "default" }
}
```

After Application, run Claude in the selected Tenant and use its official
`/login` flow if needed. The [Tenant Home](tenants.md#tenant-identity-and-layout)
persists native login state. A credential-free Config does not prove that the
Tenant is logged in.

Application writes the included credential and removes the other two fixed
credential fields, or removes all three when none is included. It also removes
an omitted Base URL. It preserves unrelated settings, `apiKeyHelper`, and native
login state such as `.credentials.json`; it does not log out or overwrite that
file. Host macOS Keychain credentials are not imported into Managed Tenants.
Existing Configs with multiple explicit credentials are invalid until the user
chooses one; AIBox does not migrate them by guessing.

Actual authentication follows [Claude's native precedence](https://code.claude.com/docs/en/authentication#authentication-precedence).
For ordinary direct or custom-endpoint sessions, custom Auth token and API key
take precedence over `apiKeyHelper`, which takes precedence over the subscription
Token and saved subscription login. Unmanaged helpers, profiles, cloud-provider
settings, and environment sources may therefore affect the selected credential.
Use Claude's `/status` to verify it. AIBox does not automatically switch to API
billing when subscription limits are reached.

## Create, Reveal, and Edit

Creating a Named Config copies its Agent template. Reveal displays every byte,
including credentials without redaction.

A safe, complete Named Config main file opens in the **Visual Editor** when its
native content is valid. Its model enforces required and optional fields and
declared enum values. An existing unknown enum string can be preserved but not
newly created in Visual mode. Codex custom-provider fields form one optional
aggregate; enabling one supplies safe placeholders without overwriting
credentials.

Claude's Visual Editor includes Skip dangerous mode prompt only under
`bypassPermissions`. Leaving that permission mode clears inclusion; returning
requires re-enabling it. Visual saves remove the field when the condition is
unmet, including in existing Configs. Opening writes nothing; Raw editing does
not enforce this condition.

**Raw Editor** remains available for every Named Config and is the only editor
for Current Config. Named Config writes validate the selected file. Current
Config writes preserve arbitrary bytes without syntax validation and may
initialize missing Managed Tenant or Agent state. Non-UTF-8 Current Config is
downloadable but read-only in the Console.

Files save independently in Agent-defined order, and leaving a dirty Config is
guarded. A later failure does not roll back an earlier save. Existing Current
Config file modes are preserved; new files use `0600`.

Catalog entries report `ready`, `incomplete`, or `invalid`. Incomplete entries
can be repaired from the template. Unknown entries are ignored while listing;
selecting an unsafe entry fails.

## Config Application and Drift

Application validates the complete Named Config and Current Config before
changing files. For each fixed field it applies the source value when present,
removes the native value when omitted, and preserves values outside the fixed
set, including statuslines and unrelated provider tables.

Values blocking a required parent object or table are replaced. Codex edits
preserve unrelated TOML comments and ordering; changed Claude JSON is
pretty-printed. Missing files are semantically empty. An absent file remains
absent when the desired result is empty; an existing file that becomes empty
keeps its native empty representation.

Each changed file is replaced atomically, but application across files is not
atomic. Rerunning the same application converges. Only after every file
succeeds does AIBox write `last_application` to the Tenant-and-Agent catalog's
`metadata.json`; it records the source name and timestamp, not an active
binding, backup, or rollback point. A metadata failure leaves already replaced
files in place without recording Last Application.

The Console reports:

| Drift state | Meaning |
| --- | --- |
| `untracked` | No successful application is recorded |
| `clean` | Current Config matches the recorded source's fixed fields |
| `dirty` | One or more fixed fields differ |
| `source-missing` | The recorded Named Config no longer exists |
| `comparison-error` | Source or Current Config cannot be compared safely |

The Console can explain these differences for Current Config and the recorded
Named Config. Comparison observes the source's current saved definition, not a
historical application snapshot. Main-file differences follow Application
projection semantics, including omitted fields and blocking parent structures;
unrelated native content and formatting do not count. Codex auth remains one
complete Config Field, with internal property differences available for inspection.

Read-only comparison accepts the visible editor drafts without saving or
initializing state. Each file can independently report that comparison is
unavailable. Draft comparison does not predict linked-file writes performed by
Save, and the catalog's Config Drift continues to describe saved files.

## Credential Propagation

Credential Propagation is separate from Config Application. The Console
previews and explicitly executes a snapshot of Host Codex Current Config
`auth.json`. The source must be a JSON object with `auth_mode = chatgpt`, a
nonempty `tokens.account_id`, and an RFC 3339 `last_refresh`.

AIBox scans existing Managed Tenant Current Configs and complete, safe Managed
and Host Named Configs. It reads no candidate `config.toml`, creates nothing,
and stores no relationship. Only older, same-account ChatGPT credentials
receive the source bytes. Equivalent content is `unchanged`; equal timestamps
with different content are `conflict`; newer targets, other accounts, and other
providers are skipped; malformed candidates are reported.

Preflight validates the complete structural view before any write. Selected
auth files are then replaced independently in stable order using the previewed
snapshots. A target failure neither rolls back earlier successes nor stops
later targets.

## Layout and Safety

Named Config catalogs use this direct layout:

```text
$AIBOX_ROOT/
  claude/<tenant>/<name>/settings.json
  codex/<tenant>/<name>/config.toml
  codex/<tenant>/<name>/auth.json
  claude/<tenant>/metadata.json
  codex/<tenant>/metadata.json
```

The Host Tenant uses `__host` as its catalog key. Names are lowercase DNS
labels of 1–63 characters. New catalog and Config directories use `0700`;
Named Config files use `0600`.

Host-side operations reject symlinks, unexpected entries, unsafe ancestors,
and oversized files. `metadata.json` is `0600`, limited to 16 KiB, and
preserves unknown top-level sections when updating a known one.

Deletion requires explicit names or an explicit select-all action; an empty
selection never means all. Safe invalid or incomplete Config directories may
be deleted, while unsafe entries are rejected. Read-only access to a missing
Tenant stays empty and creates nothing. Separate processes are not coordinated.
