# Use a supervised Docker Filesystem Sandbox

AIBox uses Docker to bound host filesystem access for Runs, Debug Shells, and
Component installation; the Request Proxy stays host-side. Cleanup supervises
both Docker client and container lifetimes because a client exit can leave its
container running. Container operations are serialized only within one process.
