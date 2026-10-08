import type {
  OverviewResponse,
  TopologyAgent,
  TopologyComponents,
  TopologyNamedConfigs,
  TopologyResponse,
  TopologyTenant,
} from "@/api/generated/wire";
import type { Operation } from "@/api/operations";
import type { ControlApi } from "@/api/transport";

export type OverviewData = OverviewResponse;
export type TopologyData = TopologyResponse;
export type { TopologyAgent, TopologyComponents, TopologyNamedConfigs, TopologyTenant };

export interface OverviewApi {
  loadOverview(signal?: AbortSignal): Promise<OverviewData>;
  loadTopology(signal?: AbortSignal): Promise<TopologyData>;
  buildImage(force: boolean): Promise<Operation>;
  loadRequestsCount?(signal?: AbortSignal): Promise<number>;
}

export function overviewApi(client: ControlApi): OverviewApi {
  return {
    loadOverview: (signal) => client.get<OverviewData>("/_aibox/api/overview", signal),
    loadTopology: (signal) => client.get<TopologyData>("/_aibox/api/topology", signal),
    buildImage: (force) => client.post<Operation>("/_aibox/api/operations/build", { force }),
    loadRequestsCount: async (signal) => {
      const response = await client.get<{ total: number }>("/_aibox/api/requests?limit=1", signal);
      return response.total;
    },
  };
}
