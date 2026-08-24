# Keep the public integration surface application-only

aibox exposes a CLI application entry point rather than embedding-oriented
dispatch APIs. Its narrow public commands are `console`, `run`, and `debug`;
Debug opens only a selected Managed Tenant Home and is not a second management
command tree. The first `--` is split before command parsing and only `run`
receives the remaining Coding Agent arguments verbatim, preserving native Agent
CLI compatibility and keeping internal orchestration free to evolve. Runtime
Image, Tenant, Component, Config, and Session management belongs to the
Console-internal Control API; no compatibility command tree or library dispatch
API is retained.
