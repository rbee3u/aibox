import type { TenantRow } from "@/api/core";
import type { CodingAgentKind } from "@/domain/codingAgent";
import type { ApplicationStatus, ConfigCatalogEntry } from "@/api/configs";
import type {
  OverviewResponse as GeneratedOverviewResponse,
  TopologyResponse as GeneratedTopologyResponse,
} from "@/api/generated/wire";
import type { Operation } from "@/api/operations";
import { sessionSummaryRequest, type SessionSummaryData } from "@/api/sessions";
import { decodeComponentRow, type ComponentRow } from "@/api/tenants";
import type { ControlApi } from "@/api/transport";
import type { TenantSelection } from "@/domain/tenant";

export interface OverviewData {
  service: {
    version: string;
    listen: string;
    uptime_seconds: number;
    aibox_root: string;
  };
  docker: {
    status: "available" | "unavailable";
    error: string | null;
  };
  runtime_image: {
    reference: string;
    status: "built" | "missing" | "unknown";
    id: string | null;
    created_at: string | null;
    size_bytes: number | null;
    detail: string | null;
  };
  managed_tenants: number;
  host_available: boolean;
  requests: {
    total: number;
    active: number;
    warning: number;
    error: number;
    bytes: number;
  };
}

export interface TopologyCurrentConfig {
  present_files: number;
  expected_files: number;
  error?: string;
}

export interface TopologyNamedConfigs {
  entries: ConfigCatalogEntry[];
  error?: string;
}

export interface TopologyAgent {
  agent: CodingAgentKind;
  current_config: TopologyCurrentConfig;
  named_configs: TopologyNamedConfigs;
  application: ApplicationStatus;
}

export interface TopologyComponents {
  entries: ComponentRow[];
  error?: string;
}

export type TopologyTenant = TenantRow & {
  agents: TopologyAgent[];
  components: TopologyComponents;
};

export interface TopologyData {
  tenants: TopologyTenant[];
}

export interface OverviewApi {
  loadOverview(signal?: AbortSignal): Promise<OverviewData>;
  loadTopology(signal?: AbortSignal): Promise<TopologyData>;
  loadSessionSummary(
    tenant: TenantSelection,
    agent: CodingAgentKind,
    signal?: AbortSignal,
  ): Promise<SessionSummaryData>;
  buildImage(force: boolean): Promise<Operation>;
}

function oneOf<T extends string>(value: string, values: readonly T[], label: string): T {
  if (values.includes(value as T)) return value as T;
  throw new Error(`Unsupported ${label}: ${value}`);
}

function overviewData(value: GeneratedOverviewResponse): OverviewData {
  return {
    ...value,
    docker: {
      status: oneOf(value.docker.status, ["available", "unavailable"], "Docker status"),
      error: value.docker.error,
    },
    runtime_image: {
      ...value.runtime_image,
      status: oneOf(
        value.runtime_image.status,
        ["built", "missing", "unknown"],
        "Runtime Image status",
      ),
    },
  };
}

function tenantRow(value: GeneratedTopologyResponse["tenants"][number]): TenantRow {
  if (value.kind === "host") {
    return {
      kind: "host",
      name: null,
      display_name: value.display_name,
      home: value.home,
      exists: value.exists,
    };
  }
  if (value.kind === "managed" && value.name !== null) {
    return {
      kind: "managed",
      name: value.name,
      display_name: value.display_name,
      home: value.home,
      exists: value.exists,
    };
  }
  throw new Error(`Unsupported Tenant kind: ${value.kind}`);
}

function topologyData(value: GeneratedTopologyResponse): TopologyData {
  return {
    tenants: value.tenants.map((tenant) => ({
      ...tenantRow(tenant),
      agents: tenant.agents.map((agent) => ({
        agent: agent.agent,
        current_config: {
          present_files: agent.current_config.present_files,
          expected_files: agent.current_config.expected_files,
          ...(agent.current_config.error === null ? {} : { error: agent.current_config.error }),
        },
        named_configs: {
          entries: agent.named_configs.entries,
          ...(agent.named_configs.error === null ? {} : { error: agent.named_configs.error }),
        },
        application: agent.application,
      })),
      components: {
        entries: tenant.components.entries.map(decodeComponentRow),
        ...(tenant.components.error === null ? {} : { error: tenant.components.error }),
      },
    })),
  };
}

export function overviewApi(client: ControlApi): OverviewApi {
  return {
    loadOverview: (signal) =>
      client.get<GeneratedOverviewResponse>("/_aibox/api/overview", signal).then(overviewData),
    loadTopology: (signal) =>
      client.get<GeneratedTopologyResponse>("/_aibox/api/topology", signal).then(topologyData),
    loadSessionSummary: sessionSummaryRequest(client),
    buildImage: (force) => client.post<Operation>("/_aibox/api/operations/build", { force }),
  };
}
