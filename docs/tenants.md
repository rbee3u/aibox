# Tenants, Sessions, and Components

The Console Tenants module owns persistent identity lifecycle. Public `run` and
`debug` commands select only Managed Tenants; the Host Tenant is
management-only.

## Tenant Identity and Layout

A **Managed Tenant** is runnable and has an AIBox-owned Tenant Home. The
protected Default Managed Tenant is named `default`; a Managed Tenant named
`host` is ordinary and runnable.

The **Host Tenant** is a separate Console view backed by the real Host Home. It
can manage host Configs, Sessions, and statuslines, but cannot Run or Debug and
never appears in Managed Tenant listing or deletion.

After acquiring `$AIBOX_ROOT/.service.lock`, `aibox console` creates or repairs
the Default Tenant baseline and fails before listening if that cannot be done
safely. A validated Run or Debug Shell may initialize another missing Managed
Tenant after Runtime Image preflight even if Docker or the invoked process then
fails.

Managed Tenant names are lowercase DNS labels of 1–63 characters. New managed
directories use `0700`; existing Host Home modes never change. Interrupted
lifecycle staging is recoverable, but separate processes are not coordinated.
`AIBOX_ROOT` defaults to `$HOME/.aibox` and must be dedicated to AIBox.

```text
$AIBOX_ROOT/
  tenants/<name>/       # Managed Tenant Home
  claude/<name>/        # Named Config catalog
  codex/<name>/
  claude/__host/        # Host Tenant catalog
  codex/__host/
  requests/             # global Request collection
```

A Managed Tenant exists only when `tenants/<name>` is a real directory. Only
that subtree may be mounted from inside `$AIBOX_ROOT`. Unknown collection
entries are ignored during listing; explicitly selected unsafe entries fail.

Deletion requires explicit Tenants or an explicit select-all action. An empty
selection never means all. `default` remains protected, and deletion is
irreversible.

## Sessions

The Console discovers native Coding Agent Transcripts on the host without
starting Docker:

| Agent | Transcript location |
| --- | --- |
| Claude | `.claude/projects/**/*.jsonl` |
| Codex | `.codex/sessions/**/rollout-*.jsonl` |

Canonical UUIDs display their final 12 hexadecimal characters. A full id or
unique suffix selects one Transcript; duplicate or ambiguous suffixes fail.
List rows use the newest timestamp and available native title or first readable
message. Transcripts without readable conversation remain visible and
deletable.

Detail streams a best-effort projection in native order. User and assistant
text becomes Conversation Messages; function and custom tool records become
Tool Activity; unsupported, injected, system, malformed, and diagnostic records
remain Transcript Evidence or warnings. Internal reasoning text is not
exposed.

Malformed JSONL and unsupported user-like records warn and make listing or
detail nonzero without hiding otherwise readable Transcripts. Missing Managed
Tenant state returns an empty read-only view and creates nothing.

Container-writable paths are untrusted. Listing may return safe rows alongside
traversal warnings, but detail and deletion fail on a partial filesystem view
or any symlinked Home, Agent state, Transcript root, or Transcript file.
Deletion is format-independent, irreversible, and requires explicit ids or an
explicit select-all action.

## Tenant Components

Components are optional native capabilities derived directly from Tenant
files; Tenant initialization installs none.

| Component | Scope | Notes |
| --- | --- | --- |
| `codex`, `claude` | Managed | Native Agent executables |
| `node` | Managed | Tenant-local Node.js and npm |
| `python` | Managed | uv/uvx, stable CPython, pip, and venv |
| `rust`, `go` | Managed | Tenant-local stable toolchains |
| `claude-statusline`, `codex-statusline` | Managed and Host | Agent-native statuslines |

Runtime and toolchain installers use the shared Runtime Image with only the
selected Tenant Home mounted. The image must already be built. Components are
independent; installing or updating one never changes another.

Versioned Components have one active version. Omitting a version lets the
native source select its stable release; exact `X.Y.Z` installs that release.
`Check for updates` refreshes native inspection and a Service-wide in-memory
Latest Release snapshot. It never polls, stores desired state, or updates
automatically. Downgrade requires Remove followed by exact install.

Inspection reports `installed`, `incomplete`, `modified`, `unmanaged`, or
`not-installed`. Installation may repair recognizable incomplete state.
Unmanaged state is neither replaced nor removable through the Console.

Removal confirms before deleting existing owned state. It deletes only the
Component launcher and owned release paths, preserving Configs, credentials,
Sessions, Workspace environments, package configuration, caches, user tools,
Cargo, and GOPATH outside those paths.

A Run requires its selected Agent to be `installed`, invokes the Tenant-local
launcher by absolute path, and never installs or falls back to the Runtime
Image. A Debug Shell requires no Component.

### Tenant Environment

Run and Debug start login Bash and let its first applicable login profile load.
AIBox then restores `HOME=/home/aibox`, inspects Components, and supplies only
missing defaults owned by Components reported as `installed`:

| Owner | Defaults |
| --- | --- |
| Node | `NPM_CONFIG_PREFIX` |
| Claude | `DISABLE_AUTOUPDATER` |
| Python | `UV_PYTHON_INSTALL_DIR`, `UV_PYTHON_BIN_DIR`, `UV_MANAGED_PYTHON`, `UV_PYTHON_DOWNLOADS` |
| Rust | `CARGO_HOME`, `RUSTUP_HOME` |
| Go | `GOROOT`, `GOPATH` |

`UV_MANAGED_PYTHON` is omitted when it or `UV_NO_MANAGED_PYTHON` is already
set. Codex owns no default. Known non-installed states are quiet; an inspection
error warns and skips only that Component.

User values win, including explicit empty values and values for absent
Components. PATH additionally receives only existing, missing Tenant-local
binary directories, preserving candidate and existing order. Candidates are
inserted before the last exact `/usr/local/bin`, or appended when absent. An
explicit empty path-owner value suppresses its candidate; an unset owner uses
its HOME-local path, while a nonempty value selects its custom directory.

AIBox creates no environment file and modifies no profile. Debug enters Bash
in `/home/aibox` without rereading profiles or rc files and mounts no Workspace
or Extra Mount. Environment changes do not hot-reload into active containers.

One AIBox process supports only one active Run, Debug Shell, or Component
installation. Config edits and applications commit one file at a time without
rollback; see [Configs](configs.md).
