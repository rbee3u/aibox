# Keep the public integration surface CLI-only

aibox exposes a CLI application entry point rather than embedding-oriented
dispatch APIs, and each command owns its selectors and arguments. The first
`--` is split before command parsing and only `run` receives the remaining
Coding Agent arguments verbatim, preserving native Agent CLI compatibility and
keeping internal orchestration free to evolve.
