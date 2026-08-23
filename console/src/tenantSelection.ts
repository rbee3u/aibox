export const DNS_LABEL_PATTERN = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/;

export type TenantSelection = { kind: "host" } | { kind: "managed"; name: string };
export type TenantSelectionKey = "host" | `managed:${string}`;

export function parseTenantSelectionKey(value: string | null): TenantSelectionKey | null {
  if (value === "host") return "host";
  if (value?.startsWith("managed:") && DNS_LABEL_PATTERN.test(value.slice(8))) {
    return value as TenantSelectionKey;
  }
  return null;
}

export function tenantSelectionValue(tenant: TenantSelection): TenantSelectionKey {
  return tenant.kind === "host" ? "host" : `managed:${tenant.name}`;
}

export function tenantSelectionFromKey(key: TenantSelectionKey): TenantSelection {
  return key === "host" ? { kind: "host" } : { kind: "managed", name: key.slice(8) };
}

export function tenantQuery(tenant: TenantSelection): URLSearchParams {
  return new URLSearchParams({ tenant: tenantSelectionValue(tenant) });
}

export function tenantBody(tenant: TenantSelection): { tenant: TenantSelectionKey } {
  return { tenant: tenantSelectionValue(tenant) };
}
