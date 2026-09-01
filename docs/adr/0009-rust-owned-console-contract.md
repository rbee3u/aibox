# Generate the Console wire contract from Rust

Rust serialization types and route declarations define the internal Control API
contract, which generates TypeScript bindings and a route manifest. Handwritten
TypeScript adapters keep HTTP calls and wire conversion local to the Console,
avoiding a second runtime schema or client contract to maintain.
