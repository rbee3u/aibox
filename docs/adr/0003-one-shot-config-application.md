# Apply Named Configs without retained state

Named Config and Current Config are distinct objects: a Named Config defines a
fixed schema, while Current Config remains the complete native configuration
shared with the Coding Agent and user. Config Application projects the Named
Config once without activation, drift tracking, rollback state, or automatic
Run-time reapplication, preserving native ownership outside the fixed fields.
