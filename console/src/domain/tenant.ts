export const DNS_LABEL_PATTERN = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/;

export type TenantSelection = { kind: "host" } | { kind: "managed"; name: string };
export type TenantSelectionValue = "host" | `managed:${string}`;

export function parseManagedTenantName(value: string): string | null {
  return DNS_LABEL_PATTERN.test(value) ? value : null;
}

export function parseTenantSelectionValue(value: string | null): TenantSelectionValue | null {
  if (value === "host") return "host";
  if (value?.startsWith("managed:")) {
    const name = parseManagedTenantName(value.slice("managed:".length));
    if (name) return `managed:${name}`;
  }
  return null;
}

export function tenantSelectionValue(selection: TenantSelection): TenantSelectionValue {
  return selection.kind === "host" ? "host" : `managed:${selection.name}`;
}

export function tenantSelectionFromValue(key: TenantSelectionValue): TenantSelection {
  return key === "host" ? { kind: "host" } : { kind: "managed", name: key.slice(8) };
}
