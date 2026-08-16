# Use Docker as the Filesystem Sandbox

aibox uses one shared Docker runtime as both Coding Agents' Filesystem Sandbox
instead of relying on their native permission modes; a Run exposes only its
Workspace, Tenant Home, and explicit Extra Mounts, while network and credential
authority remain outside that boundary. Because the container can modify every
writable mount, host-side operations treat Tenant state as untrusted and
validate its structure without following container-controlled paths.
