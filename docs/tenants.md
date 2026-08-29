# Tenants, Sessions, And Components

The Console Tenants module owns persistent identity lifecycle. The public
`aibox run` and `aibox debug` commands select a Managed Tenant with `--tenant`;
neither can select the Host Tenant.

## Tenant Identity

A **Managed Tenant** is an AIBox-managed, runnable identity with a Tenant Home.
The **Default Managed Tenant** is the protected Managed Tenant named `default`.
After taking the Service Lock, `aibox console` creates or repairs its Tenant Home
baseline and fails before listening if the baseline cannot be established
safely. A validated Run or Debug Shell can still initialize a missing Managed
Tenant when no Service has done so, even when Docker or the invoked process
later exits nonzero. A Managed Tenant named `host` is ordinary and runnable.

The Default Managed Tenant cannot be selected for deletion. An explicit delete
request for `default` is rejected, and deleting all Managed Tenants preserves
`default`. If an external process removes it, AIBox does not watch for that
change; the next Service startup repairs the baseline.

The **Host Tenant** is a management-only view backed by the real Host Home. It
is selected by the Console for host-side Config, Session, and statusline work.
It cannot Run or open a Debug Shell and is never included in the Managed Tenant
list or deletion selection. `host` and the Host Tenant are distinct identities.

Tenant Homes, Named Config catalogs, and Requests live below the
dedicated `$AIBOX_ROOT` (default `$HOME/.aibox`). Do not point that root at a
general-purpose directory: deletion is structurally scoped but removes selected
Tenant and catalog subtrees. A Managed Tenant exists exactly when
`tenants/<name>` is a real directory.

Names are lowercase DNS labels of 1-63 characters. New root, collection,
catalog, Tenant Home, and boundary directories use mode `0700`. Existing Host
Home modes are never changed. Interrupted create/delete staging is recovered by
the next lifecycle operation; AIBox does not promise cross-process locking.

## Direct Layout

```text
$AIBOX_ROOT/
  tenants/<name>/                 # Managed Tenant Home
  claude/<name>/                  # Tenant-and-Agent Named Config catalog
  codex/<name>/
  claude/__host/                  # Host Tenant catalog
  codex/__host/
  requests/<request>/             # global Requests
```

Only `tenants/<name>` subtrees may be mounted from inside `$AIBOX_ROOT`. Named
Config catalogs, Requests, and lifecycle staging remain host-only.
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

- `codex`, the official standalone OpenAI Codex executable;
- `claude`, the official native Claude Code executable;
- `claude-statusline`, a script plus native Claude `settings.json.statusLine`;
- `codex-statusline`, native Codex `tui.status_line` values;
- `node`, a Tenant-local Node.js runtime and npm installation;
- `python`, a Tenant-local Python toolchain containing uv/uvx, one active
  stable CPython, pip, and venv;
- `rust`, a Managed Tenant-local stable Rust toolchain;
- `go`, a Managed Tenant-local stable Go toolchain.

Host Tenant Components contain only the two statuslines. Runtime and toolchain
Components are Managed Tenant-only and install through a cleanup-aware
container that mounts only the selected Tenant Home. Statusline installation
edits native Current Config directly and preserves unrelated settings. Config
Application does not own or remove statusline fields.

Versioned Components have one active version. An empty version selects the
current stable release; `X.Y.Z` installs, upgrades, or downgrades to that exact
release. Updating does not rebuild the Runtime Image. Node.js stores releases
under `.node`, Codex uses its official `.codex/packages/standalone` layout, and
Claude uses its official `.local/share/claude/versions` layout. Python stores
uv releases, uv-managed CPython releases, and atomic generations under
`.python`; the displayed Component version is CPython's version, not uv's.
Codex and Claude are native executables and do not depend on Node. Python,
Node, Rust, and Go are independent Components and never install one another.
Building a native Node addon with node-gyp requires installing Python
separately. Old Python and uv releases remain until full Python Component
removal so existing virtual environments can retain immutable interpreter
paths.

`Check for updates` explicitly refreshes the selected Tenant's native
Component inspection and asks the Service to observe each versioned
Component's authoritative release source. The resulting Latest Release
snapshot is shared across Tenants for the life of the Service and is never
written to a Tenant, metadata, or browser storage. Individual unavailable or
unparseable sources remain visible without discarding successful results.
There is no polling, desired version, or automatic update.

The Console offers Update only when both installed and Latest Release versions
are comparable stable `X.Y.Z` values and the Latest Release is higher. An equal
or lower Latest Release is informational. Before the first check, installation
can still let the native installer resolve its current stable release. Exact
versions move through the explicit specific-version dialog; downgrading an
installed Component requires Remove followed by an exact installation.
Statusline Components have no release version: healthy state matches the
current AIBox Component Definition, while `modified` state exposes a Definition
update.

An empty Python version first obtains the current stable uv and then installs
the latest stable CPython known to that uv release. An exact `X.Y.Z` selects
that stable CPython release. Users can explicitly run `uv python install
X.Y.Z` to add another managed interpreter, but it does not create another
Component or change the active Component generation. Normal `uv run` and `uv
venv` operations cannot download an interpreter implicitly.

Inspection reports `installed`, `incomplete`, `modified`, `unmanaged`, or
`not-installed`. Partial recognizable state is `incomplete` and can be repaired
by installation. Unmanaged runtime state is not claimed or replaced
automatically. Unmanaged state has no Console removal action. Explicit removal
deletes only a Component's launcher and owned release paths and confirms before
deleting existing state. Python removal deletes `.python` and its AIBox-owned
launchers but preserves uv configuration/cache/tools, pip user state, and
Workspace `.venv` directories. Other removals keep Coding Agent configuration,
credentials, Transcripts, npm user state, Cargo, and GOPATH.

The shared Runtime Image must already exist before a runtime or toolchain
installer can start. It is always the fixed `aibox:latest` image and is built
only from Console Overview. After upgrading from an image that contained
Node.js, Python/uv, and the Coding Agents, rebuild it once there to remove those
old copies. No image-owned Python or Agent is copied into Tenant Homes;
explicitly install every required Component in each Tenant after the rebuild.

### Tenant Environment

A Run or Debug Shell starts login Bash and lets Bash read the first applicable
user login profile using native semantics. AIBox never creates or modifies
`.bash_profile` or `.bashrc`, and `.bashrc` is loaded only when the user's login
profile chooses to source it.

After the login profile, the current `aibox` binary restores the structural
`HOME=/home/aibox`. It snapshots native Component state before Docker starts
and supplies a missing default only when the owning Component is `installed`:
Node owns `NPM_CONFIG_PREFIX`; Claude owns `DISABLE_AUTOUPDATER`; Python owns
`UV_PYTHON_INSTALL_DIR`, `UV_PYTHON_BIN_DIR`, `UV_MANAGED_PYTHON`, and
`UV_PYTHON_DOWNLOADS`; Rust owns `CARGO_HOME` and `RUSTUP_HOME`; Go owns
`GOROOT` and `GOPATH`. `UV_MANAGED_PYTHON` is omitted when either it or
`UV_NO_MANAGED_PYTHON` is already set. Codex owns no Tenant Environment
default. Known missing, incomplete, or unmanaged Component state is quiet. An
inspection error warns and skips only that Component without blocking Run or
Debug.

Explicit user values, including empty values and values for an absent
Component, are preserved and exported. Existing Tenant-local binary
directories absent from PATH are added in stable order, independently of
Component status, so preserved Cargo, GOPATH, and npm user tools remain
available after Component removal. Each missing candidate is inserted
immediately before the last exact `/usr/local/bin` PATH segment; when that
anchor is absent, it is appended. A nonempty path-owner variable selects its
custom directory; when it is truly unset, the corresponding HOME-local
candidate is considered instead; an explicit empty value suppresses that
candidate. Existing entries, duplicates, relative entries, and empty segments
are left in place. Login-profile entries before the anchor retain higher
command-resolution priority, while added Tenant-local directories take
priority over the anchored system path.

A Run then invokes the selected Agent by its absolute Tenant-local launcher
path. A missing, incomplete, or unmanaged selected Agent Component stops the
Run with an instruction to resolve it in the Console; there is no first-Run
install or Runtime Image fallback. A Debug Shell requires no Component and,
after composing the same exported environment, enters Bash in `/home/aibox`
without rereading profile or rc files. It mounts no Workspace or Extra Mount,
but retains network access and writable access to all Tenant state.

## Concurrency

Tenant lifecycle can recover its own interrupted filesystem work, but AIBox does
not coordinate separate processes editing the same Tenant or Coding Agent state.
One process supports only one active container operation: a Run, Debug Shell,
or Component installation. See [Configs](configs.md) for per-file application
and edit behavior.
