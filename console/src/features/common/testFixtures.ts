import type { TenantRow } from "@/api/core";

/**
 * The Tenant catalog most Console page tests assume: a Host Tenant plus the
 * protected `default` Managed Tenant and one ordinary `work` Tenant.
 *
 * Tenants page tests deliberately use their own rows instead of these, because
 * that page abbreviates a Tenant Home under the Host Home to `~/...` and must
 * show an unrelated path in full. Home paths are meaningless here, so keep any
 * test that depends on their shape out of this fixture.
 */
export const TENANT_ROWS = [
  {
    kind: "host",
    name: null,
    display_name: "Host Tenant",
    home: "/home/test",
    exists: true,
  },
  {
    kind: "managed",
    name: "default",
    display_name: "default",
    home: "/aibox/tenants/default",
    exists: true,
  },
  {
    kind: "managed",
    name: "work",
    display_name: "work",
    home: "/aibox/tenants/work",
    exists: true,
  },
] satisfies TenantRow[];
