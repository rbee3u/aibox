import { tenantSelectionValue, type TenantSelection } from "@/domain/tenant";

export type { TenantSelection } from "@/domain/tenant";

export function tenantQuery(tenant: TenantSelection): URLSearchParams {
  return new URLSearchParams({ tenant: tenantSelectionValue(tenant) });
}

export function tenantBody(tenant: TenantSelection): { tenant: string } {
  return { tenant: tenantSelectionValue(tenant) };
}
