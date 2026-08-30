import type { Mock } from "vitest";
import { composeControlApi, type ConnectedControlApi } from "@/api/connect";
import type { Bootstrap } from "@/api/core";
import { ControlApi } from "@/api/transport";

export interface TestControlApi {
  bootstrap?: Partial<Bootstrap>;
  get?: Mock;
  post?: Mock;
}

export function materializeControlApi(testApi: TestControlApi): ControlApi {
  const client = new ControlApi({
    version: testApi.bootstrap?.version ?? "test",
    csrf_token: testApi.bootstrap?.csrf_token ?? "token",
    listen: testApi.bootstrap?.listen ?? "127.0.0.1:3000",
  });
  Object.assign(client, { get: testApi.get, post: testApi.post });
  return client;
}

export function composeTestApi(testApi: TestControlApi): ConnectedControlApi {
  return composeControlApi(materializeControlApi(testApi));
}

export { ControlApi };
