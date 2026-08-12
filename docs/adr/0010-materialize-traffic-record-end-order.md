# Materialize Traffic Record end order in directory names

Status: accepted

Traffic Record UUIDs are stable identities, while their direct-child directory
names are mutable materialized ordering hints. A Record begins under an
`active-` name derived from its start time and, after its terminal Summary is
committed, is renamed to a name derived from its Traffic End Time. This makes
reverse ASCII basename order match the viewer's active-first and terminal
end-time order without adding metadata or a schema migration. Summary remains
the lifecycle authority: `active-` means only that a terminal directory name
was not materialized, so an interrupted process can leave a non-terminal Record
and a rename failure can leave a terminal Record under that prefix. Rename and
directory-sync failures are warnings and never replace the real Traffic Outcome.

This deliberately accepts a non-transactional boundary between the terminal
Summary and directory rename. Readers support both sides of that boundary and
safe older unprefixed names, but do not infer state from names or migrate
existing Records. Same-process namespace synchronization prevents path-based
operations from racing a rename or deletion; aibox still promises no
cross-process coordination.
