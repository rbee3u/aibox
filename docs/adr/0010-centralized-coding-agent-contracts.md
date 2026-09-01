# Centralize Coding Agent contracts

AIBox centralizes shared Coding Agent paths, native Config files, templates, and
invocation through `AgentKind`, while each Agent module owns its Config Field
table and templates. Transcript parsing remains in Session backends because it
is native Session behavior rather than a shared launch contract.
