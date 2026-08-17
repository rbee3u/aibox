# Record Last Application and derive Config Drift without reconciliation

After every Config Field has been applied successfully, aibox records the
Named Config name and application timestamp in the strict `last_application`
section of
`$AIBOX_ROOT/<agent>/<tenant-or-__host>/metadata.json`. The catalog-root file
is a small, host-only, aibox-owned container for typed observational sections;
updating Last Application preserves unknown top-level sections. This Last
Application lets the Console derive Config Drift by comparing the current
Named Config projection with live Current Config.

The states are Untracked, Clean, Dirty, Source missing, and Comparison error.
The record creates no active binding: Runs still consume Current Config only,
Named Config changes never apply automatically, aibox performs no
reconciliation, and deleting a Tenant also deletes its observational record.
Deleting only the source Named Config retains the record so the Console can
report Source missing. No legacy application-record layout is read or migrated.
