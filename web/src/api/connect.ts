import { configsApi, type ConfigApi } from "@/api/configs";
import type { Bootstrap } from "@/api/core";
import { operationsApi, type OperationApi } from "@/api/operations";
import { overviewApi, type OverviewApi } from "@/api/overview";
import { requestsApi, type RequestsApi } from "@/api/requests";
import { sessionsApi, type SessionApi } from "@/api/sessions";
import { tenantsApi, type TenantApi } from "@/api/tenants";
import { ControlApi } from "@/api/transport";

export interface ConnectedControlApi {
  bootstrap: Bootstrap;
  overview: OverviewApi;
  tenants: TenantApi;
  configs: ConfigApi;
  sessions: SessionApi;
  requests: RequestsApi;
  operations: OperationApi;
}

export function composeControlApi(client: ControlApi): ConnectedControlApi {
  return {
    bootstrap: client.bootstrap,
    overview: overviewApi(client),
    tenants: tenantsApi(client),
    configs: configsApi(client),
    sessions: sessionsApi(client),
    requests: requestsApi(client),
    operations: operationsApi(client),
  };
}

export async function connectControlApi(fetchImpl: typeof fetch = fetch) {
  return composeControlApi(await ControlApi.connect(fetchImpl));
}
