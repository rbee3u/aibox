# Model Tenant identity through direct storage

AIBox defines a Managed Tenant by a real directory under the AIBox Root and
keeps the Root dedicated but unmarked. Host Tenant native Coding Agent state
lives in the Host Home, while its AIBox-owned Named Config catalog lives under
the Root in the Agent-specific `__host` catalog; this avoids a registry while
requiring structural checks at filesystem boundaries.
