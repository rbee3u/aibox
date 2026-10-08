import {
  tenantSelectionValue,
  type TenantSelection,
  type TenantSelectionValue,
} from "@/domain/tenant";

export function tenantLocation(key: TenantSelectionValue | null): URLSearchParams {
  const query = new URLSearchParams();
  if (key) query.set("tenant", key);
  return query;
}

export function tenantComponentLocation(
  tenant: TenantSelection,
  component?: string,
): URLSearchParams {
  const query = tenantLocation(tenantSelectionValue(tenant));
  if (component) query.set("component", component);
  return query;
}
