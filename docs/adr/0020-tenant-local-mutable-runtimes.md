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
