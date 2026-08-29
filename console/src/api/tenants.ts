import type { TenantRow } from "@/api/core";
import type {
  ComponentRow as GeneratedComponentRow,
  LatestEntry as GeneratedLatestEntry,
  LatestSnapshot as GeneratedLatestSnapshot,
  OperationSnapshot,
} from "@/api/generated/wire";
import type { Operation } from "@/api/operations";
import type { ControlApi } from "@/api/transport";
import { tenantBody, tenantQuery } from "@/api/tenantSelection";
import type { TenantSelection } from "@/domain/tenant";

export type ComponentKind =
  "node" | "codex" | "claude" | "python" | "claude-statusline" | "codex-statusline" | "rust" | "go";

export type ComponentStatus =
  "not-installed" | "installed" | "incomplete" | "modified" | "unmanaged";

export type ComponentRow = Omit<GeneratedComponentRow, "kind" | "status"> & {
  kind: ComponentKind;
  supports_version: boolean;
  status: ComponentStatus | null;
};

export type ComponentLatestEntry = Omit<GeneratedLatestEntry, "kind"> & {
  kind: ComponentKind;
};

export type ComponentLatestSnapshot = Omit<GeneratedLatestSnapshot, "entries"> & {
  entries: ComponentLatestEntry[];
};

const COMPONENT_KINDS = new Set<ComponentKind>([
  "node",
  "codex",
  "claude",
  "python",
  "claude-statusline",
  "codex-statusline",
  "rust",
  "go",
]);

const COMPONENT_STATUSES = new Set<ComponentStatus>([
  "not-installed",
  "installed",
  "incomplete",
  "modified",
  "unmanaged",
]);

function componentKind(value: string): ComponentKind {
  if (COMPONENT_KINDS.has(value as ComponentKind)) return value as ComponentKind;
  throw new Error(`Unsupported Component kind: ${value}`);
}

function componentStatus(value: string | null): ComponentStatus | null {
  if (value === null) return null;
  if (COMPONENT_STATUSES.has(value as ComponentStatus)) return value as ComponentStatus;
  throw new Error(`Unsupported Component status: ${value}`);
}

export function decodeComponentRow(value: GeneratedComponentRow): ComponentRow {
  return { ...value, kind: componentKind(value.kind), status: componentStatus(value.status) };
}

function latestSnapshot(value: GeneratedLatestSnapshot | null): ComponentLatestSnapshot | null {
  if (value === null) return null;
  if (!Array.isArray(value.entries)) return value as unknown as ComponentLatestSnapshot;
  return {
    ...value,
    entries: value.entries.map((entry) => ({ ...entry, kind: componentKind(entry.kind) })),
  };
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
        .then((rows) =>
          Array.isArray(rows) ? rows.map(decodeComponentRow) : (rows as unknown as ComponentRow[]),
        ),
    latestComponents: (signal) =>
      client
        .get<GeneratedLatestSnapshot | null>("/_aibox/api/components/latest", signal)
        .then((value) =>
          value === null || (typeof value === "object" && Array.isArray(value.entries))
            ? latestSnapshot(value)
            : (value as unknown as ComponentLatestSnapshot),
        ),
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
