# Use a supervised Docker Filesystem Sandbox

AIBox uses Docker as the Filesystem Sandbox and execution boundary for Runs,
Debug Shells, and container-based Component installation, while keeping the
Request Proxy host-side. It validates untrusted mounted state and supervises
Docker client and container lifetimes, but provides no cross-process lock.
