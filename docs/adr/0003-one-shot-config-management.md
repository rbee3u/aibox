# Keep Config management explicit and one-shot

AIBox treats Config Application and Credential Propagation as separate,
explicit, one-time operations. They do not create persistent bindings,
automatic reconciliation, or rollback state, while Last Application and Config
Drift remain observations rather than active configuration.
