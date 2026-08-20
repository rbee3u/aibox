# Use Named-only Visual Config Editing

The Configs detail view provides a Visual Editor only for complete, safe Named
Config main files with valid native content, while incomplete Named Configs,
Current Config, and Codex `auth.json` remain Raw Editor-only. Visual fields come
from the fixed `AgentKind` Config Field contract; Include off means the field is
omitted and therefore removed by the next explicit Config Application. Raw
editing keeps native JSON/TOML syntax highlighting and backend diagnostics, but
Current Config retains its arbitrary-byte save semantics. Visual writes patch
the native file through the Rust model so unrelated TOML comments/order and
unrelated native settings remain intact.
