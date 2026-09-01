# Configs

The Console Configs module manages Named Configs and Current Config. A Run uses
only Current Config from its selected Managed Tenant; it never reads or
reapplies a Named Config.

## Model and Native Files

Every Config belongs to one Tenant and Coding Agent. The Host Tenant operates
directly on the real Host Home, including unredacted credentials.

- A **Named Config** is a reusable definition of the fixed Config Fields owned
  by `AgentKind`.
- **Current Config** is the native file set read by the Coding Agent.
- **Config Application** is an explicit, one-shot projection of a Named Config
  into Current Config.
- **Config Drift** compares Current Config with the source recorded by Last
  Application; it never reconciles or reapplies anything.

| Agent | Named Config files | Native role |
| --- | --- | --- |
| Claude | `settings.json` | JSON settings, including `ANTHROPIC_*` values |
| Codex | `config.toml`, `auth.json` | TOML settings and one complete JSON auth object |

Codex `auth.json` is one complete Config Field and is replaced as a whole. The
Agent contract defines file order, built-in templates, empty content, and
supported fields. Named Config directories contain no management metadata.
Templates use native unrestricted, non-interactive settings and credential
placeholders that must be reviewed before application.

## Create, Reveal, and Edit

Creating a Named Config copies its Agent template. Reveal displays every byte,
including credentials without redaction.

A safe, complete Named Config main file opens in the **Visual Editor** when its
native content is valid. Its model enforces required and optional fields and
declared enum values. An existing unknown enum string can be preserved but not
newly created in Visual mode. Codex custom-provider fields form one optional
aggregate; enabling one supplies safe placeholders without overwriting
credentials.

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
