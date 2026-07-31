# Tenants

A Tenant is a persistent identity that scopes native Coding Agent state,
Providers, and Sessions. Managed Tenants are runnable and aibox-managed. The
Host Tenant is management-only and refers to the real host Home.

## Everyday Use

The default Managed Tenant is named `default`. A successful first Run
initializes it automatically:

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
including credentials, settings, Sessions, caches, Providers, Active Provider
state, and local toolchains. It does not delete the shared Docker image or any
Workspace. aibox asks before each deletion; non-interactive callers must use
`--yes`. There is no deletion backup.

An empty deletion selection is an error. `--all` cannot be combined with
explicit names.

## Storage Layout

The default root is `$HOME/.aibox`. `AIBOX_ROOT` selects another location; a
relative value is resolved from the launch directory.

```text
$AIBOX_ROOT/
  claude/
    <tenant>/
      .metadata.json
      <provider>/
        .metadata.json
        settings.json
        auth.json
    __host/                        # Host Tenant uses the key __host
      ...
  codex/
    <tenant>/
      .metadata.json
      <provider>/
        .metadata.json
        config.toml
        auth.json
    __host/
      ...
  tenants/
    <tenant>/                      # complete Managed Tenant Home
      .gitconfig
      .codex/
        config.toml
        auth.json
        sessions/YYYY/MM/DD/...
      .claude/
        settings.json
        statusline.sh                 # optional Claude status-line Component
        projects/...
      .cargo/                      # optional Tenant-local Rust
      .rustup/
      .goroot/                     # optional Tenant-local Go
      .gopath/
```

`tenants/<tenant>` is both the Managed Tenant's authoritative existence marker
and the directory mounted at `/home/aibox`. Nothing beneath `claude/` or
`codex/` is mounted into a Run. Agent/Tenant `.metadata.json` files are mode
`0600`.

Managed Tenant and Provider names match `[0-9A-Za-z-]+`. Underscores are not
accepted. `__host` is an internal storage key and cannot collide with a Managed
Tenant name. `host` remains a valid Managed Tenant name.

Collection listing ignores incomplete staging entries, invalid names, files,
and other unrecognized entries. Explicitly selected structural paths must be
real directories rather than symlinks.

## Lifecycle Recovery

Creation builds the initial Home under `$creating-<tenant>` and publishes it by
rename. Deletion first renames the Home to `$deleting-<tenant>`, then removes
the Home and Agent metadata. Repeating the same create or delete command safely
finishes interrupted work.

A missing Managed Tenant is an empty read scope: Provider list and Session list
are empty, Provider status is inactive, and every Component is not installed.
Read-only commands and completion do not create it. `run`, `provider create`,
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
aibox provider --host list
aibox session --host list
aibox session --host --agent claude list
```

Its aibox-owned Provider metadata lives under `$AIBOX_ROOT/<agent>/__host/`;
native Agent Configuration and Sessions remain in the real host Home. aibox
does not install the Managed Tenant `.gitconfig` or any Tenant Component there.

Host Tenant operations can modify real host state: Provider activation changes
real Agent Configuration, and Session deletion permanently removes real
transcripts. The Host Tenant cannot Run, cannot be created or deleted, and does
not appear in `tenant list`.

`--tenant host` selects the ordinary runnable Managed Tenant named `host`.
`--host` selects the Host Tenant. They are never aliases and are mutually
exclusive.

## Concurrency

aibox does not provide cross-process locks for Tenant or Provider operations.
Avoid changing the same Tenant and Coding Agent from multiple aibox processes
at once. Interrupted lifecycle and Provider operations are resumable, but that
recovery is not a multi-process coordination mechanism.

One aibox process supports only one active Run or toolchain installation.

## Tenant Components

A Tenant Component is optional native state installed into one Managed Tenant
Home. Components are unavailable to the Host Tenant and are not tracked in a
separate registry. List the fixed catalog and its state without starting
Docker or creating a missing Tenant:

```sh
aibox component list
aibox component list --tenant work
```

Status-line Components report `installed`, `modified`, or `not-installed`.
Toolchains report `installed <version>`, `unmanaged`, or `not-installed`.
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

Before changing Agent Configuration, installation completes an interrupted
Provider transaction for that Coding Agent. It does not change Provider source
or Active Provider metadata. Installing after activation therefore changes the
native working file under the normal Provider rules; later activation or
deactivation may replace it from the recorded base.

### Rust And Go

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

The selected aibox image must already exist locally. The installer container
mounts only the Tenant Home, keeps normal network access, and uses the same
cleanup and Linux uid/gid handling as an Agent Run. Rust resolves releases from
the official stable channel; Go uses official release metadata and verifies the
archive SHA-256.

An exact healthy installed version is skipped. For a different supported
stable version, aibox removes the old SDK before installing the requested one;
an installation failure can therefore temporarily leave no SDK, and repeating
the command repairs that state. Rust preserves `.cargo`, rustup, and unrelated
toolchains. Go preserves `.gopath`. A recognizable nightly, RC, or custom SDK is
`unmanaged` and must be handled manually before aibox will install over it.

The corresponding binary directories are already on `PATH`. See
[Sandbox and Mounts](sandbox.md) before sharing a toolchain or credentials
through Extra Mounts.
