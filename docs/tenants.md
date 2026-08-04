# Tenants

A Tenant is a persistent identity that scopes native Coding Agent state,
Agent Profiles, Tenant Components, and Sessions. Managed Tenants are runnable
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

Deletion removes the Tenant Home and both Coding Agents' Tenant-local metadata,
including credentials, settings, Sessions, caches, Agent Profiles, Active Agent
Profile state, and local toolchains. It does not delete the shared Docker image
or any Workspace. aibox asks before each deletion; non-interactive callers must
use `--yes`. There is no deletion backup.

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
      .metadata.json               # Active Profile/transaction state, if any
      <profile>/
        .metadata.json
        settings.json
        auth.json
    __host/                        # Host Tenant uses the key __host
      ...
  codex/
    <tenant>/
      .metadata.json               # Active Profile/transaction state, if any
      <profile>/
        .metadata.json
        config.toml
        auth.json
    __host/
      ...
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
`codex/` is mounted into a Run. Agent/Tenant `.metadata.json` files are mode
`0600`. Newly created aibox root, collection, Agent/Tenant metadata directory,
Agent Profile directory, Tenant Home, and native `.codex`/`.claude` state
directories are mode `0700`. Every Agent Profile file is mode `0600`; the
baseline `.gitconfig` is mode `0644`. Native configuration, credential, and
Component entries in the tree above appear on demand. Transcript entries appear
only after the corresponding Coding Agent creates Sessions.

Managed Tenant and Agent Profile names are 1–63 character lowercase DNS labels:
only `[a-z0-9-]` is accepted, and the first and last character must be a letter
or digit. `__host` is an internal storage key and cannot collide with a Managed
Tenant name. `host` remains a valid Managed Tenant name.

Managed Tenant listing ignores lifecycle staging entries, invalid names, files,
and symlinks. Agent Profile listing likewise ignores invalid names and unsafe
entry types, and also hides incomplete profiles. Explicitly selected structural
paths are rejected unless they are real directories rather than symlinks.

## Lifecycle Recovery

Creation builds the initial Home under `$creating-<tenant>` and publishes it by
rename. Deletion first renames the Home to `$deleting-<tenant>`, then removes
the Home and Agent metadata. Repeating the same create or delete command safely
finishes interrupted work.

A missing Managed Tenant is an empty read scope: `profile list` and
`session list` are empty, `profile status` is inactive, and every Component is
not installed.
Read-only commands and completion do not create it. `run`, `profile create`,
and `component install` may initialize it.

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
aibox profile --host list
aibox session --host list
aibox session --host --agent claude list
```

Its aibox-owned Agent Profile metadata lives under `$AIBOX_ROOT/<agent>/__host/`;
native Agent Configuration and Sessions remain in the real host Home. aibox
does not install the Managed Tenant `.gitconfig` or any Tenant Component there.

Host Tenant operations can modify real host state: Agent Profile activation
changes real Agent Configuration, and Session deletion permanently removes real
transcripts. The Host Tenant cannot Run, cannot be created or deleted, and does
not appear in `tenant list`.

`--tenant host` selects the ordinary runnable Managed Tenant named `host`.
`--host` selects the Host Tenant. They are never aliases and are mutually
exclusive.

## Sessions

Session browsing reads Coding Agent Transcripts directly on the host and never
starts Docker. Omitting the subcommand is the same as `list`; a full Session id
or unique prefix selects one Transcript:

```sh
aibox session
aibox session --agent claude list
aibox session get 3f2a
aibox session delete 3f2a
aibox session delete --all --yes
```

`list` shows newest Sessions first using a Coding Agent-generated title when
available, otherwise the first recognized typed prompt. A Transcript with no
recognized typed prompt is still listed and included by `delete --all`.
`get` prints only the best-effort typed-prompt view, not the complete
Transcript.

Malformed JSONL or unsupported user-like records produce warnings and make
`list` or `get` exit nonzero without hiding otherwise readable data. Deletion
does not parse Transcript contents, but it still requires a complete, safe
filesystem traversal. If part of a Transcript tree cannot be traversed, `list`
may show readable rows and exit nonzero; `get` and `delete` fail without acting
on a partial view. Symlinked Tenant Homes, Coding Agent state directories,
Transcript roots, and Transcript files are rejected.

Deletion requires explicit ids or `--all`, asks before each selected
Transcript unless `--yes` is used, and is irreversible. Host Tenant Session
commands operate on the real host Transcripts described above.

## Concurrency

aibox does not provide cross-process locks for Tenant or Agent Profile
operations. Avoid changing the same Tenant and Coding Agent from multiple aibox
processes at once. Interrupted lifecycle and Agent Profile operations are
resumable, but that recovery is not a multi-process coordination mechanism.

One aibox process supports only one active container operation: a Run or a
toolchain installation.

## Tenant Components

A Tenant Component is optional native state installed into one Managed Tenant
Home. Components are unavailable to the Host Tenant and are not tracked in a
separate registry. List the fixed catalog and its state without starting
Docker or creating a missing Tenant:

```sh
aibox component list
aibox component list --tenant work
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
```

`claude-statusline` writes `.claude/statusline.sh` and sets
`settings.json.statusLine` to run `bash ~/.claude/statusline.sh`.
`codex-statusline` owns `tui.status_line` and `tui.status_line_use_colors` in
`.codex/config.toml`. Installation replaces those owned values with the version
bundled into aibox while preserving unrelated native configuration. Repeating
an installation is safe and updates modified content.

Before changing Agent Configuration, installation or removal completes an
interrupted Agent Profile transaction for that Coding Agent. It does not change
Agent Profile source or Active Agent Profile metadata. Status-line
configuration is independently owned by the Component and survives Agent
Profile activation, reconciliation, switching, and deactivation. Installation
and activation both reject an overlap with the other owner.

Remove a Component explicitly:

```sh
aibox component remove claude-statusline
aibox component remove rust --tenant work --yes
```

Installed and incomplete state can be removed directly. Modified or unmanaged
state requires `--discard-changes`; `--yes` skips confirmation. Status-line
removal deletes only the Claude script when applicable and the Component's
native configuration keys.

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
network access, and uses the same cleanup and Linux uid/gid handling as an
Agent Run. Rust resolves releases from the official stable channel; Go uses
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
replace it, and removal requires `--discard-changes`.

The corresponding binary directories are already on `PATH`. See
[Sandbox and Mounts](sandbox.md) before sharing a toolchain or credentials
through Extra Mounts.
