# Keep the public integration surface CLI-only

aibox exposes a CLI application entry point rather than embedding-oriented
dispatch APIs. The first `--` is split before command parsing and only `run`
receives the remaining Coding Agent arguments verbatim, preserving native Agent
CLI compatibility and keeping internal orchestration free to evolve. ADR 0010
adds a Console-internal Control API and deprecates management commands without
making that API or the library an external integration surface.
