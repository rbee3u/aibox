import type { TenantRow } from "@/api/core";
import { tenantSelectionValue, type TenantSelectionValue } from "@/domain/tenant";

/**
 * Chooses the Tenant to show when the URL names none: the protected Default
 * Managed Tenant, then any Managed Tenant, then the Host Tenant.
 */
export function fallbackTenantSelectionValue(rows: TenantRow[]): TenantSelectionValue | null {
  const fallback =
    rows.find((row) => row.kind === "managed" && row.name === "default") ??
    rows.find((row) => row.kind === "managed") ??
    rows.find((row) => row.kind === "host");
  return fallback ? tenantSelectionValue(fallback) : null;
}
