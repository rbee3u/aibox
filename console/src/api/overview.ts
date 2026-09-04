import type { CodingAgentKind } from "@/domain/codingAgent";
import type {
  OverviewResponse,
  TopologyAgent,
  TopologyComponents,
  TopologyNamedConfigs,
  TopologyResponse,
  TopologyTenant,
} from "@/api/generated/wire";
import type { Operation } from "@/api/operations";
import { sessionSummaryRequest, type SessionSummaryData } from "@/api/sessions";
import type { ControlApi } from "@/api/transport";
import type { TenantSelection } from "@/domain/tenant";

/**
 * The Overview and Topology reads, named the way the Console refers to them.
 *
 * Both are aliases rather than restatements. The Rust wire types already close
 * the Docker and Runtime Image status sets, carry the Tenant row as the same
 * discriminated union the Tenants module lists, and mark an omitted `error` key
 * optional, so there is nothing left for this module to narrow or re-derive.
 */
export type OverviewData = OverviewResponse;
export type TopologyData = TopologyResponse;
export type { TopologyAgent, TopologyComponents, TopologyNamedConfigs, TopologyTenant };

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
