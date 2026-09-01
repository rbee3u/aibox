# Keep AIBox application-only

AIBox exposes an application surface through `console`, `run`, and `debug`;
management stays inside the foreground Service and embedded Console. Runs and
Debug Shells are transient and have no Run History or Run-to-Session mapping,
so the library exposes no embedding-oriented management or execution model.
