import type { TenantRow } from "@/api/core";
import type { Operation } from "@/api/operations";
import type { ControlApi } from "@/api/transport";
import { tenantBody, tenantQuery, type TenantSelection } from "@/api/tenantSelection";

export type ComponentKind =
  "node" | "codex" | "claude" | "python" | "claude-statusline" | "codex-statusline" | "rust" | "go";

export type ComponentStatus =
  "not-installed" | "installed" | "incomplete" | "modified" | "unmanaged";

export interface ComponentRow {
  kind: ComponentKind;
  supports_version: boolean;
  status: ComponentStatus | null;
  version: string | null;
  error: string | null;
}

export interface ComponentLatestEntry {
  kind: ComponentKind;
  state: "available" | "unavailable";
  version: string | null;
  source: string;
  error: string | null;
}

export interface ComponentLatestSnapshot {
  checked_at: string;
  entries: ComponentLatestEntry[];
}

export interface TenantApi {
  listTenants(signal?: AbortSignal): Promise<TenantRow[]>;
  listComponents(tenant: TenantSelection, signal?: AbortSignal): Promise<ComponentRow[]>;
  latestComponents(signal?: AbortSignal): Promise<ComponentLatestSnapshot | null>;
  checkLatestComponents(): Promise<ComponentLatestSnapshot>;
  createTenant(name: string): Promise<void>;
  deleteTenants(names: string[]): Promise<void>;
  mutateComponent(
    tenant: TenantSelection,
    component: ComponentKind,
    install: boolean,
    version: string | null,
  ): Promise<Operation | object>;
}

export function listTenantsRequest(client: ControlApi) {
  return (signal?: AbortSignal) => client.get<TenantRow[]>("/_aibox/api/tenants", signal);
}

export function tenantsApi(client: ControlApi): TenantApi {
  return {
    listTenants: listTenantsRequest(client),
    listComponents: (tenant, signal) =>
      client.get<ComponentRow[]>(`/_aibox/api/components?${tenantQuery(tenant)}`, signal),
    latestComponents: (signal) =>
      client.get<ComponentLatestSnapshot | null>("/_aibox/api/components/latest", signal),
    checkLatestComponents: () =>
      client.post<ComponentLatestSnapshot>("/_aibox/api/components/latest/check", {}),
    createTenant: async (name) => {
      await client.post("/_aibox/api/tenants", { name });
    },
    deleteTenants: async (names) => {
      await client.post("/_aibox/api/tenants/delete", {
        names,
        all: false,
        confirmation: names.length === 1 ? names[0] : "",
      });
    },
    mutateComponent: (tenant, component, install, version) =>
      client.post<Operation | object>(`/_aibox/api/components/${install ? "install" : "remove"}`, {
        ...tenantBody(tenant),
        component,
        version,
      }),
  };
}
