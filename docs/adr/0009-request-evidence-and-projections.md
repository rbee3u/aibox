# Preserve raw Request evidence with materialized projections

A Request captures application-visible HTTP semantics, raw bodies, and
timing evidence for one request attempt rather than pretending to be a wire
capture. Its materialized Summary supplies the stable list and protocol
projection, including Request Assessment, while strict detail reads retain the
raw files as diagnostic evidence; this avoids repeatedly parsing large bodies
without duplicating parsed payloads or weakening recording fidelity. Format v4
uses `request_id`; format v3 is neither read nor migrated, so its collection is
cleared manually before upgrading.
