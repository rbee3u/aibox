import type { CodingAgentKind, TenantRow } from "@/api/core";
import type { ApplicationStatus, ConfigCatalogEntry } from "@/api/configs";
import type { Operation } from "@/api/operations";
import { sessionSummaryRequest, type SessionSummaryData } from "@/api/sessions";
import type { ComponentRow } from "@/api/tenants";
import type { ControlApi } from "@/api/transport";
import type { TenantSelection } from "@/api/tenantSelection";

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

export function overviewApi(client: ControlApi): OverviewApi {
  return {
    loadOverview: (signal) => client.get<OverviewData>("/_aibox/api/overview", signal),
    loadTopology: (signal) => client.get<TopologyData>("/_aibox/api/topology", signal),
    loadSessionSummary: sessionSummaryRequest(client),
    buildImage: (force) => client.post<Operation>("/_aibox/api/operations/build", { force }),
  };
}
