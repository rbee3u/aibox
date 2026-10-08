# Own Config drafts in one editing scope

One Tenant, Agent, and Config scope owns drafts and save coordination, independent
of mounted file views. A local session publishes immutable snapshots to React,
so sequential saves observe earlier successes and dirty state and comparisons
share one source. This avoids duplicate draft registries or global editing state.
