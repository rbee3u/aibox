# Tenants, Sessions, And Components

The Console Tenants module owns persistent identity lifecycle. The public
`aibox run` command selects a Managed Tenant with `--tenant`; it does not create
or select the Host Tenant.

## Tenant Identity

A **Managed Tenant** is an aibox-managed, runnable identity with a Tenant Home.
The **Default Managed Tenant** is the protected Managed Tenant named `default`.
After taking the Service Lock, `aibox serve` creates or repairs its Tenant Home
baseline and fails before listening if the baseline cannot be established
safely. A validated Run can still initialize a missing Managed Tenant when no
Service has done so, even when Docker or the Coding Agent later exits nonzero. A
Managed Tenant named `host` is ordinary and runnable.

The Default Managed Tenant cannot be selected for deletion. An explicit delete
request for `default` is rejected, and deleting all Managed Tenants preserves
`default`. If an external process removes it, aibox does not watch for that
change; the next Service startup repairs the baseline.

The **Host Tenant** is a management-only view backed by the real Host Home. It
is selected by the Console for host-side Config, Session, and statusline work.
It cannot Run and is never included in the Managed Tenant list or deletion
selection. `host` and the Host Tenant are distinct identities.

Tenant Homes, Named Config catalogs, and Request Records live below the
dedicated `$AIBOX_ROOT` (default `$HOME/.aibox`). Do not point that root at a
general-purpose directory: deletion is structurally scoped but removes selected
Tenant and catalog subtrees. A Managed Tenant exists exactly when
`tenants/<name>` is a real directory.

Names are lowercase DNS labels of 1-63 characters. New root, collection,
catalog, Tenant Home, and boundary directories use mode `0700`. Existing Host
Home modes are never changed. Interrupted create/delete staging is recovered by
the next lifecycle operation; aibox does not promise cross-process locking.

## Direct Layout

```text
$AIBOX_ROOT/
  tenants/<name>/                 # Managed Tenant Home
  claude/<name>/                  # Tenant-and-Agent Named Config catalog
  codex/<name>/
  claude/__host/                  # Host Tenant catalog
  codex/__host/
  requests/<record>/              # global Request Records
```

Only `tenants/<name>` subtrees may be mounted from inside `$AIBOX_ROOT`. Named
Config catalogs, Request Records, and lifecycle staging remain host-only.
Unknown collection entries are ignored during listing; explicitly selected
unsafe entries are rejected.

## Sessions

The Console Sessions module reads native Coding Agent Transcripts directly on
the host and never starts Docker. A Session is discovered from its Transcript,
independently of any Run. Claude transcripts are `.claude/projects/**/*.jsonl`;
Codex transcripts are `rollout-*.jsonl` below `.codex/sessions/`.

Canonical UUID ids are shown by their final 12 hexadecimal characters. Other ids
are shown in full. A full id or unique suffix selects one Transcript; duplicate
or ambiguous suffixes are rejected.

List rows use the newest timestamp first and show a native title when available,
otherwise the first readable Conversation Message. Each row also exposes the
newest readable message preview, Tenant and Coding Agent source, start time, and
a quiet warning indicator when parsing was partial. Transcripts with no readable
conversation remain visible and are included in bulk deletion.

Session detail is a progressive NDJSON projection of the native Transcript. It
keeps Conversation Messages in native order, places user messages on the right
and Coding Agent replies on the left. Consecutive Tool Activity and Transcript
Evidence stay at their original position but are visually grouped into one
collapsed activity disclosure. The Console reads a single evidence entry on
demand only after the user expands it and rechecks the Transcript snapshot; a
changed snapshot requires refreshing the detail view.

Claude typed or external user content becomes a user Conversation Message,
Coding Agent reply text becomes an assistant Conversation Message, and
`tool_use`/`tool_result` become
Tool Activity. Codex wrapper-filtered user content, assistant `message` or
`agent_message`, function calls, and custom tool calls use the same projection.
Reasoning and thinking are counted as hidden internal diagnostics and their
raw text is never exposed. Unknown, injected, system, unsupported, and
malformed entries remain visible as diagnostic evidence or warnings without
being presented as conversation text. Message bodies are plain text and keep
their line breaks.

The detail header shows Tenant, Coding Agent, start and last-event times,
observed duration, message/tool counts, warnings, and expandable Transcript
facts such as the full id, relative path, file size, working directory, model
provider, CLI version, and parsing counts. Refresh is manual; it does not tail
the file or move the scroll position automatically.

Malformed JSONL and unsupported user-like records produce warnings and make the
list or detail operation nonzero without hiding otherwise readable rows.
Deletion is format-independent but strict: it validates the complete safe
filesystem view before removing any selected Transcript. A partial traversal,
symlinked Home, Agent state directory, Transcript root, or Transcript file makes
detail and deletion fail without acting on a partial view. Read-only discovery
of a missing Managed Tenant returns an empty view and creates nothing.

Deletion always names explicit Session ids or selects `all` in the Console; an
empty selection never means all. The operation is irreversible and the Console
requires a confirmation step before removing existing Transcripts.

## Tenant Components

The Console Components module derives state from native files and never uses a
separate Component registry. Tenant initialization installs no Components.

The fixed catalog is:

- `claude-statusline`, a script plus native Claude `settings.json.statusLine`;
- `codex-statusline`, native Codex `tui.status_line` values;
- `rust`, a Managed Tenant-local stable Rust toolchain;
- `go`, a Managed Tenant-local stable Go toolchain.

Host Tenant Components contain only the two statuslines. Rust and Go are
Managed Tenant-only and install through a cleanup-aware container that mounts
only the selected Tenant Home. Statusline installation edits native Current
Config directly and preserves unrelated settings. Config Application does not
own or remove statusline fields.

Inspection reports `installed`, `incomplete`, `modified`, `unmanaged`, or
`not-installed`. Partial recognizable state is `incomplete` and can be repaired
by installation. Unmanaged toolchains are not claimed or replaced
automatically. Explicit removal deletes only a Component's owned paths, keeps
Cargo and GOPATH user state, and confirms before deleting existing state.

The shared Runtime Image must already exist before a Rust or Go installation
needs to start its installer. It is always the fixed `aibox:latest` image; build
it with `aibox build` or from Console Overview.

## Concurrency

Tenant lifecycle can recover its own interrupted filesystem work, but aibox does
not coordinate separate processes editing the same Tenant or Coding Agent state.
One process supports only one active container operation: a Run or a toolchain
installation. See [Configs](configs.md) for per-file application and edit
behavior.
