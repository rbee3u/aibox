# Remove Component deep links from Tenant navigation

## Status

Accepted

Tenant Component rows are no longer addressable through a `component` query
parameter. Overview Component nodes and attention items navigate to the owning
Tenant with only `tenant=<selection>`. When the Tenant page receives a historic
Component link, it preserves the Tenant selection and replaces the URL without
the obsolete parameter.

This keeps the URL contract aligned with the interaction model: Component rows
are ordinary diagnostic list items rather than a second selection mode. Details
are expanded explicitly in place, so navigation remains stable while the user
can inspect more than one Component at a time. The trade-off is that a historic
link can no longer open one row already expanded, but it avoids two competing
selection states, removes misleading row-level selection semantics, and makes
Overview attention navigation useful at Tenant scope.
