# Generate the Console wire contract from Rust

Rust serialization types are the sole source for Control API DTOs, NDJSON and
SSE frames, and committed JSON contract samples. An explicit ignored export
test uses `ts-rs` during development to update read-only TypeScript bindings;
ordinary `cargo test` compiles the derives without writing the workspace, and
a separate check regenerates into a temporary directory and compares bytes.
Hand-written `api/<domain>.ts` adapters continue to own HTTP paths and convert
wire values into narrow feature-facing ports, while a minimal Console
`domain/` owns only cross-feature identity and invariants.

A generated HTTP client and runtime schema validator were rejected because the
Control API remains Console-internal and compiled together with its only
client. Fully hand-maintained TypeScript mirrors were rejected because fixture
drift had already allowed impossible Request format and Config propagation
shapes into tests. This decision changes neither routes nor serialized JSON;
wire values are converted to validated internal commands after decoding.

The exported surface includes every Control request/response DTO plus NDJSON
and SSE frames. Closed Component kinds/statuses are generated as TypeScript
unions, while their serialized strings remain unchanged. A test-only route
manifest exports stable semantic keys, methods, and path templates for adapter
tests without generating a production client. `make console-contract-check`
exports bindings, route descriptions, and JSON samples to a temporary
directory and compares committed bytes. `make
console-assets-check` independently builds the Console to a temporary output,
enforces the bundle budget, and compares the embedded HTML, CSS, and JavaScript.
Both checks run under `make console-check`, so generated contracts and the Rust
binary's embedded assets cannot drift silently.
