# Keep mutable runtimes in Tenant Homes

aibox keeps application language runtimes and Coding Agent executables as
native Managed Tenant Components instead of pinning them in the fixed Runtime
Image. This includes Node.js, the Python toolchain aggregate (uv, one active
CPython, pip, and venv), Codex, and Claude Code, alongside the existing Rust and
Go toolchains. The Runtime Image remains a stable OS substrate with shell,
build, download, and diagnostic capabilities but no callable application
language runtime.

Each Component has an explicit per-Tenant lifecycle and can be upgraded without
rebuilding the image. Language Components do not imply or automatically install
one another. The trade-offs are an explicit first installation in every Tenant,
network-dependent installation, and retention of old Tenant-local releases when
stable absolute paths must remain valid. Adopting this decision requires one
image rebuild to remove former image-owned copies; it does not change ADR 0014's
fixed `aibox:latest` tag.

The Tenant Environment is composed by the current aibox binary at Run or Debug
Shell startup rather than persisted in each Tenant Home. User login-profile
values retain priority, and only the structural `/home/aibox` identity is
restored unconditionally. A Component contributes its missing environment
defaults only when native inspection reports it as installed; inspection
failure skips that Component without blocking execution. PATH remains based on
existing Tenant-local binary directories rather than Component ownership, so
user tool directories deliberately retained by removal remain usable.
