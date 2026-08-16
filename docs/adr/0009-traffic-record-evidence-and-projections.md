# Preserve raw Traffic evidence with materialized projections

A Traffic Record captures application-visible HTTP semantics, raw bodies, and
timing evidence for one request attempt rather than pretending to be a wire
capture. Its materialized Summary supplies the stable list and protocol
projection, including Record Assessment, while strict detail reads retain the
raw files as diagnostic evidence; this avoids repeatedly parsing large bodies
without duplicating parsed payloads or weakening recording fidelity.
