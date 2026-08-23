# Keep shared Console UI primitives native

## Status

Accepted

The Console owns its small set of shared UI primitives using native HTML,
semantic CSS variables, and CSS Modules. A general visual or headless UI
framework adds runtime weight, leaks its contracts into application code, and
still requires substantial adaptation to preserve AIBox's compact visual
language; Ant Design therefore no longer forms part of the Console foundation,
superseding [ADR 0018](0018-console-visual-system.md).

Focused libraries may continue to provide specialized behavior such as code
editing, Markdown rendering, icons, or lossless JSON handling. They must not
introduce a competing token source or take ownership of domain structures such
as topology, catalogs, Session conversation, diagnostics, or dialogs.
