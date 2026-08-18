# Keep the public integration surface application-only

aibox exposes a CLI application entry point rather than embedding-oriented
dispatch APIs. The first `--` is split before command parsing and only `run`
receives the remaining Coding Agent arguments verbatim, preserving native Agent
CLI compatibility and keeping internal orchestration free to evolve. Tenant,
Component, Config, and Session management belongs to the Console-internal
Control API; no compatibility command tree or library dispatch API is retained.
