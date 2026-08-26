# Keep Component update observations explicit and transient

AIBox checks authoritative release sources only through an explicit Component
Update Check and keeps the resulting Latest Releases as one Service-wide
in-memory snapshot. Sharing the snapshot avoids repeating Tenant-independent
network requests, while refusing to persist it preserves native-state-derived
Components and prevents observational release data from becoming desired state,
automatic reconciliation, or a Component registry; statuslines instead compare
their native state with the Component Definition built into AIBox.
