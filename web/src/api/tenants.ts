import type { TenantRow } from "@/api/core";
import type {
  ComponentKind,
  ComponentRow,
  LatestEntry,
  LatestSnapshot,
} from "@/api/generated/wire";
import type { Operation } from "@/api/operations";
import type { ControlApi } from "@/api/transport";
import { tenantBody, tenantQuery } from "@/api/tenantSelection";
import type { TenantSelection } from "@/domain/tenant";

export type { ComponentKind, ComponentRow };
export type ComponentLatestEntry = LatestEntry;
export type ComponentLatestSnapshot = LatestSnapshot;

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
  ): Promise<ComponentMutationResult>;
}

export type ComponentMutationResult =
  | { kind: "operation"; operation: Operation }
  | { kind: "completed"; value: Record<string, unknown> };

function isOperation(value: Operation | Record<string, unknown>): value is Operation {
  return (
    typeof value === "object" &&
    value !== null &&
    "id" in value &&
    typeof value.id === "string" &&
    "state" in value &&
    typeof value.state === "string"
  );
}

export function listTenantsRequest(client: ControlApi) {
  return (signal?: AbortSignal) => client.get<TenantRow[]>("/_aibox/api/tenants", signal);
}

export function tenantsApi(client: ControlApi): TenantApi {
  return {
    listTenants: listTenantsRequest(client),
    listComponents: async (tenant, signal) => {
      const rows = await client.get<ComponentRow[]>(
        `/_aibox/api/components?${tenantQuery(tenant)}`,
        signal,
      );
      return Array.isArray(rows) ? rows : [];
    },
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
    mutateComponent: async (tenant, component, install, version) => {
      const value = await client.post<Operation | Record<string, unknown>>(
        `/_aibox/api/components/${install ? "install" : "remove"}`,
        { ...tenantBody(tenant), component, version },
      );
      if (isOperation(value)) {
        return { kind: "operation", operation: value };
      }
      return { kind: "completed", value };
    },
  };
}
