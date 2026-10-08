# Keep Management use cases independent of Service

Management owns cross-domain use cases and process-local state; Service owns
HTTP, composition, and process lifecycle. Coordinators receive only required
capabilities, keeping transport out of workflows without a framework or separate
crate. Request and Management retain their own drain policies.
