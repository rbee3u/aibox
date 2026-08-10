# Materialize the Traffic list projection and Record Assessment

Traffic Record format v2 makes `summary.json` the complete list projection and
persists a Record Assessment derived from independent Traffic Outcome, HTTP,
Provider Error, and diagnostic evidence. The list reads only this projection;
detail remains strict over raw metadata and Body entries. This avoids reparsing
potentially large or malformed container-writable evidence on every poll and
keeps failure presentation consistent across list, detail header, and
Diagnostics.

The Assessment is a display classification, not a replacement for its evidence:
Active takes temporary visual precedence, terminal findings remain separately
available, and one prioritized primary finding supplies the compact label. This
schema break deliberately provides no v1 reader, migration, backfill, or
read-time repair; incomplete or inconsistent v2 projections are invalid rather
than silently reconstructed from raw files.
