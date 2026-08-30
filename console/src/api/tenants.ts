import type { TenantRow } from "@/api/core";
import type {
  ComponentKind as GeneratedComponentKind,
  ComponentRow as GeneratedComponentRow,
  ComponentStatusWire as GeneratedComponentStatus,
  LatestEntry as GeneratedLatestEntry,
  LatestSnapshot as GeneratedLatestSnapshot,
  OperationSnapshot,
} from "@/api/generated/wire";
import type { Operation } from "@/api/operations";
import type { ControlApi } from "@/api/transport";
import { tenantBody, tenantQuery } from "@/api/tenantSelection";
import type { TenantSelection } from "@/domain/tenant";

export type ComponentKind = GeneratedComponentKind;
export type ComponentStatus = GeneratedComponentStatus;
export type ComponentRow = GeneratedComponentRow;
export type ComponentLatestEntry = GeneratedLatestEntry;
export type ComponentLatestSnapshot = GeneratedLatestSnapshot;

export function decodeComponentRow(value: GeneratedComponentRow): ComponentRow {
  return value;
}

function latestSnapshot(value: GeneratedLatestSnapshot | null): ComponentLatestSnapshot | null {
  return value;
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
  ): Promise<ComponentMutationResult>;
}

export type ComponentMutationResult =
  | { kind: "operation"; operation: Operation }
  | { kind: "completed"; value: Record<string, unknown> };

function isOperation(value: OperationSnapshot | Record<string, unknown>): value is Operation {
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
    listComponents: (tenant, signal) =>
      client
        .get<GeneratedComponentRow[]>(`/_aibox/api/components?${tenantQuery(tenant)}`, signal)
        .then((rows) => (Array.isArray(rows) ? rows.map(decodeComponentRow) : [])),
    latestComponents: (signal) =>
      client
        .get<GeneratedLatestSnapshot | null>("/_aibox/api/components/latest", signal)
        .then((value) => latestSnapshot(value)),
    checkLatestComponents: () =>
      client
        .post<GeneratedLatestSnapshot>("/_aibox/api/components/latest/check", {})
        .then((snapshot) => latestSnapshot(snapshot)!),
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
      const value = await client.post<OperationSnapshot | Record<string, unknown>>(
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
