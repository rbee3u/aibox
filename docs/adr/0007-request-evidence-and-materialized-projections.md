# Preserve Request evidence with materialized projections

AIBox preserves raw Request evidence alongside bounded inspection projections:
the lifecycle Summary and an optional, rebuildable SSE index. State, Outcome,
and Assessment remain distinct. SSE observation is independent of Store-owned
index persistence, so replay needs no writer and index failures cannot redefine
protocol evidence or interrupt forwarding.
