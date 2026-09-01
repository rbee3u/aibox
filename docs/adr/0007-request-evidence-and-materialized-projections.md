# Preserve Request evidence with materialized projections

AIBox preserves application-visible Request evidence and exposes bounded
materialized projections for inspection. The `summary` projection is the core
lifecycle and list view, while the SSE event index is optional and rebuildable;
neither replaces the raw evidence or conflates Request State, Request Outcome,
and Request Assessment.
