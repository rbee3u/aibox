# Keep the Console visual system AIBox-owned

## Status

Superseded by [ADR 0019](0019-native-console-ui-primitives.md).

The Console uses Ant Design 6 as a themed interaction foundation for shared buttons, inputs, checkboxes, tooltips, and related primitives, while AIBox semantic tokens, CSS Modules, Lucide icons, and domain-specific structures remain authoritative for its visual identity and layout. This avoids continuing to reimplement mature control behavior without turning the Console into a generic Ant Design application: topology, catalogs, Session conversation, CodeMirror, diagnostics, and the behavior-tested native dialog stay AIBox-owned, and external fonts or CDN assets are not introduced.

The light and dark token sets are equal first-class themes, with the initial `system` preference following the operating system. Ant Design-generated styles receive the request-scoped Console CSP nonce, routine validation remains socket-free and screenshot-independent, and the generated JavaScript may grow by at most 250 KiB gzip from the pre-library baseline of 365,536 bytes.
