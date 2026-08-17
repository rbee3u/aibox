# Apply Named Configs without retained state

Named Config and Current Config are distinct objects: a Named Config defines a
fixed schema, while Current Config remains the complete native configuration
shared with the Coding Agent and user. Config Application projects the Named
Config once without activation, rollback, or automatic Run-time reapplication,
preserving native ownership outside the fixed fields. ADR 0013 adds
observational Last Application and Config Drift while retaining this one-shot
behavior.
