import type { TenantRow } from "@/api/core";
import type { TenantSelectionValue } from "@/domain/tenant";

export function tenantSelectionValueOf(row: TenantRow): TenantSelectionValue {
  return row.kind === "host" ? "host" : `managed:${row.name}`;
}

/**
 * Chooses the Tenant to show when the URL names none: the protected Default
 * Managed Tenant, then any Managed Tenant, then the Host Tenant.
 */
export function fallbackTenantSelectionValue(rows: TenantRow[]): TenantSelectionValue | null {
  const fallback =
    rows.find((row) => row.kind === "managed" && row.name === "default") ??
    rows.find((row) => row.kind === "managed") ??
    rows.find((row) => row.kind === "host");
  return fallback ? tenantSelectionValueOf(fallback) : null;
}

export function tenantLocation(key: TenantSelectionValue | null): URLSearchParams {
  const query = new URLSearchParams();
  if (key) query.set("tenant", key);
  return query;
}
