# Generate the Console wire contract from Rust

Rust serialization types and route declarations generate the internal Control
API's TypeScript bindings and route manifest, avoiding a second schema to
maintain. Handwritten Console adapters own HTTP calls and wire conversion.
Recursive export from endpoint root types keeps nested domain types private
without a parallel export inventory.
