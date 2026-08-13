# Tenants

A Tenant is a persistent identity that scopes native Coding Agent state,
Named Configs, Tenant Components, and Sessions. Managed Tenants are runnable
and aibox-managed. The Host Tenant is management-only and refers to the real
host Home.

## Everyday Use

The default Managed Tenant is named `default`. aibox resolves and validates the
Workspace and Extra Mounts, then confirms that the selected image exists,
before initializing it. Invalid Run inputs and a missing image therefore leave
a missing Tenant absent. After those checks pass, initialization happens before
Docker starts the Coding Agent, so the Tenant remains initialized if Docker
startup later fails or the Coding Agent exits nonzero:

```sh
aibox run
aibox run --tenant work
aibox run --agent claude --tenant work
```

Managed Tenant lifecycle commands are explicit and idempotent:

```sh
aibox tenant create work
aibox tenant list
aibox tenant delete work
aibox tenant delete work test
aibox tenant delete --all
```

Deletion removes the Tenant Home and both Coding Agents' Named Config catalogs,
including credentials, settings, Sessions, caches, Named Configs, and local
toolchains. It does not delete the shared Docker image or any Workspace. aibox
asks before deleting a Tenant that has stored data; non-interactive callers must
use `--yes`. A selected name with nothing stored is a silent no-op. There is no
deletion backup.

An empty deletion selection is an error. `--all` cannot be combined with
explicit names.

## Storage Layout

The default root is `$HOME/.aibox`. `AIBOX_ROOT` selects another location; a
relative value is resolved from the launch directory.

> **Use a dedicated directory for `AIBOX_ROOT`.** aibox deliberately has no
> ownership marker. Tenant deletion removes selected subtrees from this root,
> so pointing it at a general-purpose directory creates avoidable data-loss
> risk.

```text
$AIBOX_ROOT/
  claude/
    <tenant>/
      <config>/
        settings.json
    __host/                        # Host Tenant uses the key __host
      ...
  codex/
    <tenant>/
      <config>/
        config.toml
        auth.json
    __host/
      ...
  traffic/
    <UTC-time>-<upstream-host>-<uuid-v7>/   # `active-` prefix until terminal
      request.json
      request.body
      response.json
      response.body
      response.events.jsonl          # optional best-effort unencoded SSE index
      summary.json
  tenants/
    <tenant>/                      # complete Managed Tenant Home
      .gitconfig
      .codex/
        config.toml                # optional native configuration
        auth.json                  # optional native credentials
        sessions/YYYY/MM/DD/...    # optional Session Transcripts
      .claude/
        settings.json              # optional native configuration
        statusline.sh              # optional status-line Component
        projects/...               # optional Session Transcripts
      .cargo/                      # optional Tenant-local Rust
      .rustup/
      .goroot/                     # optional Tenant-local Go
      .gopath/
```

`tenants/<tenant>` is both the Managed Tenant's authoritative existence marker
and the directory mounted at `/home/aibox`. Nothing beneath `claude/` or
`codex/` is mounted into a Run. Newly created aibox root, collection, Agent
Named Config catalog, Named Config directory, Tenant Home, and native
`.codex`/`.claude` state directories are mode `0700`. Every Named Config file
is mode `0600`; the baseline `.gitconfig` is mode `0644`. Native configuration,
credential, and Component entries in the tree above appear on demand.
Transcript entries appear only after the corresponding Coding Agent creates
Sessions.

`traffic/` is a flat, global collection rather than Tenant data. Every Traffic
Record is a direct child so total-count and deletion scans use one directory
level; there is deliberately no `YYYY/MM/DD` partition. Tenant creation and
deletion never create or remove Traffic Records. See
[Traffic Proxy](sandbox.md#traffic-proxy) for the data and cleanup contract.

Managed Tenant and Named Config names are 1–63 character lowercase DNS labels:
only `[a-z0-9-]` is accepted, and the first and last character must be a letter
or digit. `__host` is an internal storage key and cannot collide with a Managed
Tenant name. `host` remains a valid Managed Tenant name.

Managed Tenant listing ignores lifecycle staging entries, invalid names, files,
and symlinks. Named Config listing applies the equivalent rules described in
[Configs](configs.md). Explicitly selected structural paths reject symlinks and
unexpected entries.

## Lifecycle Recovery

Creation builds the initial Home under `$creating-<tenant>` and publishes it by
rename. Deletion first renames the Home to `$deleting-<tenant>`, then removes
the Home and Named Config catalogs. Repeating the same create or delete command
safely finishes interrupted work.

A missing Managed Tenant is an empty read scope: `config list` and
`session list` are empty, and every Component is not installed. Read-only
commands and completion do not create it. `tenant create`, `run`,
`config create`, `config edit --current`, and `component install` may initialize
it. Targeted `config get --current` fails for a missing Managed Tenant without
creating it.

## Tenant Home Initialization

Initialization creates `.codex/`, `.claude/`, and `.gitconfig` when missing.
The Git configuration rewrites common GitHub SSH clone URLs to HTTPS because
Tenant Homes do not inherit host SSH keys. Status lines and toolchains are
optional Tenant Components and are never part of this baseline.

Existing regular files are preserved. Managed Tenants do not inherit the
host's Git identity, SSH keys, or credential helpers. Configure them in the
Tenant Home or grant narrowly scoped Extra Mounts.

## Host Tenant

The Host Tenant uses the real `$HOME/.codex` or `$HOME/.claude` state and is
selected explicitly:

```sh
aibox config --host list
aibox config --host get --current
aibox config --host edit --current
aibox session --host list
aibox session --host --agent claude list
```

Its aibox-owned Named Config catalog lives under `$AIBOX_ROOT/<agent>/__host/`;
native Current Config and Sessions remain in the real host Home. aibox does not
install the Managed Tenant `.gitconfig` or a toolchain there, but Host
statusline Components can explicitly modify the native Claude and Codex files.

Host Tenant operations expose or modify real host state: `config get` prints
credentials without redaction, Config Application and `config edit --current`
change real Current Config, and confirming Application after a Host Named Config
edit does the same. Global Credential Propagation reads Host Codex Current
Config as its default source, and Session deletion permanently removes real
transcripts. Config operations have no backup or rollback. The Host Tenant
cannot Run, cannot be created or deleted, and does not appear in `tenant list`.

`--tenant host` selects the ordinary runnable Managed Tenant named `host`.
`--host` selects the Host Tenant. They are never aliases and are mutually
exclusive.

## Sessions

Session browsing reads Coding Agent Transcripts directly on the host and never
starts Docker. Omitting the subcommand is the same as `list`; a full Session id
or unique suffix selects one Transcript:

```sh
aibox session
aibox session --agent claude list
aibox session get 458cbf92d123
aibox session delete 458cbf92d123
aibox session delete --all --yes
```

`list` shows newest Sessions first. Canonically formatted UUID ids appear as
their final 12 hexadecimal characters; other ids appear in full. A rare shared
suffix remains visible on every matching row and must be disambiguated with a
longer suffix or full id. Titles use a Coding Agent-generated title when
available, otherwise the first recognized typed prompt. A Transcript with no
recognized typed prompt is still listed and included by `delete --all`. `get`
prints only the best-effort typed-prompt view, not the complete Transcript.

Malformed JSONL or unsupported user-like records produce warnings and make
`list` or `get` exit nonzero without hiding otherwise readable data. Deletion
does not parse Transcript contents, but it still requires a complete, safe
filesystem traversal. If part of a Transcript tree cannot be traversed, `list`
may show readable rows and exit nonzero; `get` and `delete` fail without acting
on a partial view. Symlinked Tenant Homes, Coding Agent state directories,
Transcript roots, and Transcript files are rejected.

Transcript files are streamed rather than loaded whole, but one JSONL record
is limited to 64 MiB. An oversized record makes `list` or `get` fail for that
Transcript instead of buffering unbounded container-written input; deletion
remains format-independent.

Deletion requires explicit ids or `--all`, asks before each selected
Transcript unless `--yes` is used, and is irreversible. Host Tenant Session
commands operate on the real host Transcripts described above.

## Concurrency

aibox does not provide cross-process locks for Tenant or Named Config
operations. Avoid changing the same Tenant and Coding Agent from multiple aibox
processes at once. Interrupted Tenant lifecycle work is resumable.
[Configs](configs.md) describes how Config Application and Config edits behave
when they are interrupted.

One aibox process supports only one active container operation: a Run or a
toolchain installation.

## Tenant Components

A Tenant Component is optional native state installed into a Managed Tenant's
Tenant Home or the Host Tenant's real Host Home. Statusline Components support
both, while Rust and Go toolchains support Managed Tenants only. Components are
not tracked in a separate registry. List the fixed catalog and its state
without starting Docker or creating a missing Tenant:

```sh
aibox component list
aibox component list --tenant work
aibox component --host list
```

Components report `installed`, `incomplete`, `modified`, `unmanaged`, or
`not-installed` as applicable. `incomplete` means recognizable aibox-managed
state is partial or unhealthy and can be repaired by installation.
Symlinked, malformed, or unexpectedly typed owned paths are errors rather than
installation states.

### Status Lines

Install each Coding Agent integration explicitly:

```sh
aibox component install claude-statusline
aibox component install codex-statusline --tenant work
aibox component --host install claude-statusline
```

`claude-statusline` writes `.claude/statusline.sh` and sets
`settings.json.statusLine` to run `bash ~/.claude/statusline.sh`.
`codex-statusline` sets `tui.status_line` and
`tui.status_line_use_colors = false` in `.codex/config.toml`. Installation
replaces those values with the version bundled into aibox while preserving
unrelated native configuration. Repeating an installation is safe and updates
modified content. Config Fields exclude status-line paths, so applying a
Config preserves installed status-line configuration without coordination
between the two commands.

Both integrations use this native field order: model with reasoning, current
directory, Git branch, context-window size, and context used. The Claude script
renders the same compact form, for example
`gpt-5.6-sol xhigh · /workspace · dev · 258K window · Context 54% used`.
It abbreviates the Home directory as `~`, omits unavailable fields, and always
renders plain text. Codex statuslines are explicitly configured without native
colors. Changing the bundled definition marks an older installation as
`modified` until it is explicitly reinstalled.

Status-line inspection, installation, and removal cap the native Current Config
file at 16 MiB because they parse and rewrite it in memory. Reduce a larger file
outside aibox before managing that status-line Component.

Remove a Component explicitly:

```sh
aibox component remove claude-statusline
aibox component remove rust --tenant work --yes
```

Any existing Component state can be removed directly after confirmation;
`--yes` skips confirmation and is required in a non-interactive shell.
Status-line removal deletes only the Claude script when applicable and the
Component's native configuration keys. Host removal follows the same rules and
changes only the real statusline files owned by the selected Component.

### Rust and Go

The shared image includes Python and Node.js, but not Rust or Go. Their paths
are configured for persistent Tenant-local installation:

- Rust binaries and caches use `$HOME/.cargo`; toolchains use `$HOME/.rustup`.
  aibox installs stable toolchains through [rustup](https://rustup.rs/).
- Go uses `$HOME/.goroot` for the SDK and `$HOME/.gopath` for commands, modules,
  and build caches.

Install the latest stable release or select an exact stable `X.Y.Z` release:

```sh
aibox component install rust
aibox component install rust@1.90.0
aibox component install go
aibox component install go@1.25.6 --tenant work
```

When installation work is needed, the selected aibox image must already exist
locally. An explicitly requested healthy version is skipped before the image
check. The installer container mounts only the Tenant Home, keeps normal
network access, and uses the same cleanup and Linux uid/gid handling as a Run.
Rust resolves releases from the official stable channel; Go uses
official release metadata and verifies the archive SHA-256.

Health checks require the actual SDK executable and executable permission; a
Rust proxy alone is not a healthy toolchain. An exact healthy installed version
is skipped. For a different supported stable version, aibox removes the old SDK
before installing the requested one; an installation failure can therefore
temporarily leave no SDK, and repeating the command repairs that state. Rust
installation preserves `.cargo` user state. Rust removal deletes `.rustup` and
known rustup proxies while preserving Cargo caches and unrelated commands. Go
installation and removal preserve `.gopath`; removal deletes only `.goroot`. A
recognizable nightly, RC, or custom SDK is `unmanaged`: installation refuses to
replace it, while explicit removal still deletes only the Component-owned SDK
paths.

The corresponding binary directories are already on `PATH`. See
[Sandbox and Mounts](sandbox.md) before sharing a toolchain or credentials
through Extra Mounts.
