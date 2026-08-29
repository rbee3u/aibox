# Use Docker as the Filesystem Sandbox

AIBox uses one shared Docker runtime as the Coding Agents' and Debug Shell's
Filesystem Sandbox instead of relying on native permission modes; a Run exposes
only its Workspace, Tenant Home, and explicit Extra Mounts, while Debug exposes
only its Tenant Home. Network and credential authority remain outside that
boundary. Because the container can modify every writable mount, host-side
operations treat Tenant state as untrusted and validate its structure without
following container-controlled paths.
