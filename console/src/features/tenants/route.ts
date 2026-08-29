import type { TenantRow } from "@/api/core";
import type { TenantSelectionKey } from "@/domain/tenant";

export function tenantKeyOf(row: TenantRow): TenantSelectionKey {
  return row.kind === "host" ? "host" : `managed:${row.name}`;
}

/**
 * Chooses the Tenant to show when the URL names none: the protected Default
 * Managed Tenant, then any Managed Tenant, then the Host Tenant.
 */
export function fallbackTenantKey(rows: TenantRow[]): TenantSelectionKey | null {
  const fallback =
    rows.find((row) => row.kind === "managed" && row.name === "default") ??
    rows.find((row) => row.kind === "managed") ??
    rows.find((row) => row.kind === "host");
  return fallback ? tenantKeyOf(fallback) : null;
}

/** Tenants uses only `tenant`; historical `component` values are dropped. */
export function tenantLocation(key: TenantSelectionKey | null): URLSearchParams {
  const query = new URLSearchParams();
  if (key) query.set("tenant", key);
  return query;
}
