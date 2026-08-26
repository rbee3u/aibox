import { useMemo } from "react";
import type { ComponentProps, ElementType } from "react";
import { vi } from "vitest";
import { composeControlApi, type ConnectedControlApi } from "@/api/connect";
import type { Bootstrap, TenantRow } from "@/api/core";
import type { Operation } from "@/api/operations";
import { sessionDetailPath, type SessionListData, type SessionRow } from "@/api/sessions";
import type { ComponentLatestSnapshot } from "@/api/tenants";
import { ControlApi } from "@/api/transport";
import { ConfigPage as ConfigPageView } from "@/features/configs/ConfigPage";
import { OperationPanel as OperationPanelView } from "@/app/OperationPanel";
import { SessionPage as SessionPageView } from "@/features/sessions/SessionPage";
import { TenantPage as TenantPageView } from "@/features/tenants/TenantPage";

export interface TestControlApi {
  bootstrap?: Partial<Bootstrap>;
  get?: ReturnType<typeof vi.fn>;
  post?: ReturnType<typeof vi.fn>;
  streamSessionDetail?: ReturnType<typeof vi.fn>;
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

/**
 * Session detail arrives as an NDJSON stream, so tests substitute the composed
 * endpoint rather than transport reads. The substitute receives the real
 * request path so path assertions stay meaningful.
 */
export function composeTestApi(testApi: TestControlApi): ConnectedControlApi {
  const composed = composeControlApi(materializeControlApi(testApi));
  const stream = testApi.streamSessionDetail;
  if (!stream) return composed;
  return {
    ...composed,
    sessions: {
      ...composed.sessions,
      streamSessionDetail: (tenant, agent, id, handlers, signal) =>
        (stream as (...args: unknown[]) => Promise<void>)(
          sessionDetailPath(tenant, agent, id),
          handlers,
          signal,
        ),
    },
  };
}

export type TestPageProps<T extends ElementType> = Omit<ComponentProps<T>, "api" | "search"> & {
  api: TestControlApi;
};

export function TenantPage(
  props: TestPageProps<typeof TenantPageView> & {
    search?: string;
  },
) {
  const { api, search = window.location.search, ...pageProps } = props;
  return <TenantPageView {...pageProps} api={composeTestApi(api).tenants} search={search} />;
}
export function ConfigPage(props: TestPageProps<typeof ConfigPageView>) {
  const { api, ...pageProps } = props;
  return (
    <ConfigPageView
      {...pageProps}
      api={composeTestApi(api).configs}
      search={window.location.search}
    />
  );
}
export function SessionPage(
  props: TestPageProps<typeof SessionPageView> & {
    search?: string;
  },
) {
  const { api, search = window.location.search, ...pageProps } = props;
  const sessionApi = useMemo(() => composeTestApi(api).sessions, [api]);
  return <SessionPageView {...pageProps} api={sessionApi} search={search} />;
}
export function OperationPanel(
  props: Omit<ComponentProps<typeof OperationPanelView>, "api"> & { api: TestControlApi },
) {
  const { api, ...panelProps } = props;
  return <OperationPanelView {...panelProps} api={composeTestApi(api).operations} />;
}
export const firstSession = {
  id: "11111111-1111-1111-1111-111111111111",
  display_id: "111111111111",
  start_ts: "2026-08-17T09:00:00Z",
  title: "First prompt",
  latest_message: "First prompt",
  message_count: 1,
  tool_count: 0,
  warnings: [],
} satisfies SessionRow;
export const secondSession = {
  id: "22222222-2222-2222-2222-222222222222",
  display_id: "222222222222",
  start_ts: "2026-08-17T08:00:00Z",
  title: "Second prompt",
  latest_message: "Second prompt",
  message_count: 1,
  tool_count: 0,
  warnings: [],
} satisfies SessionRow;
export const thirdSession = {
  id: "33333333-3333-3333-3333-333333333333",
  display_id: "333333333333",
  start_ts: "2026-08-17T07:00:00Z",
  title: "New prompt",
  latest_message: "New prompt",
  message_count: 1,
  tool_count: 0,
  warnings: [],
} satisfies SessionRow;
export const tenants = [
  {
    kind: "host",
    name: null,
    display_name: "Host Tenant",
    home: "/home/test",
    exists: true,
  },
  {
    kind: "managed",
    name: "default",
    display_name: "default",
    home: "/aibox/tenants/default",
    exists: true,
  },
  {
    kind: "managed",
    name: "work",
    display_name: "work",
    home: "/aibox/tenants/work",
    exists: true,
  },
] satisfies TenantRow[];
export const tenantRows = [
  {
    kind: "host",
    name: null,
    display_name: "Host Tenant",
    home: "/home/test",
    exists: true,
  },
  {
    kind: "managed",
    name: "default",
    display_name: "default",
    home: "/home/test/.aibox/tenants/default",
    exists: true,
  },
  {
    kind: "managed",
    name: "work",
    display_name: "work",
    home: "/var/lib/aibox/tenants/work",
    exists: true,
  },
] satisfies TenantRow[];
export const activeOperation: Operation = {
  id: "operation-active",
  kind: "Install Rust toolchain",
  state: "running",
  started_at: "2026-08-19T01:00:00Z",
  ended_at: null,
  result: null,
  first_sequence: 0,
  next_sequence: 0,
  logs: [],
};
export type SessionRowFixture = Omit<
  SessionRow,
  "latest_message" | "message_count" | "tool_count"
> &
  Partial<Pick<SessionRow, "latest_message" | "message_count" | "tool_count">>;

export function list(sessions: SessionRowFixture[], warnings: string[] = []): SessionListData {
  const rows = sessions.map((session) => ({
    latest_message: session.title,
    message_count: 0,
    tool_count: 0,
    ...session,
  }));
  return {
    sessions: rows,
    warnings,
    partial: warnings.length > 0 || rows.some((session) => session.warnings.length > 0),
  };
}
export function fakeApi({
  sessions = () => list([firstSession, secondSession]),
  post = vi.fn().mockResolvedValue({ deleted: 1 }),
  streamSessionDetail = vi.fn().mockImplementation(
    (
      _path: string,
      handlers: {
        onComplete: (
          stats: {
            start_ts: string;
            last_event_ts: string;
            observed_duration_ms: number;
            message_count: number;
            tool_count: number;
            entry_count: number;
            malformed_count: number;
            unsupported_count: number;
            hidden_internal_count: number;
            file_size: number;
            snapshot: string;
          },
          warnings: string[],
        ) => void;
      },
    ) => {
      handlers.onComplete(
        {
          start_ts: firstSession.start_ts,
          last_event_ts: firstSession.start_ts,
          observed_duration_ms: 0,
          message_count: 0,
          tool_count: 0,
          entry_count: 0,
          malformed_count: 0,
          unsupported_count: 0,
          hidden_internal_count: 0,
          file_size: 0,
          snapshot: "0:0",
        },
        [],
      );
      return Promise.resolve();
    },
  ),
}: {
  sessions?: (path: string, signal?: AbortSignal) => Promise<SessionListData> | SessionListData;
  post?: ReturnType<typeof vi.fn>;
  streamSessionDetail?: ReturnType<typeof vi.fn>;
} = {}) {
  const get = vi.fn((path: string, signal?: AbortSignal) => {
    if (path === "/_aibox/api/tenants") return Promise.resolve(tenants);
    if (path.startsWith("/_aibox/api/sessions?")) return Promise.resolve(sessions(path, signal));
    return Promise.reject(new Error(`Unexpected GET ${path}`));
  });
  const api: TestControlApi = {
    bootstrap: { version: "test", csrf_token: "token" },
    get,
    post,
    streamSessionDetail,
  };
  return { api, get, post, streamSessionDetail };
}
export function sessionQuery(path: string): URLSearchParams {
  return new URL(path, "http://aibox.test").searchParams;
}
export function tenantApi({
  rows = tenantRows,
  components = [],
  latest = null,
  post = vi.fn().mockResolvedValue({ deleted: 1 }),
}: {
  rows?: TenantRow[];
  components?: Array<{
    kind: string;
    supports_version: boolean;
    status: string | null;
    version: string | null;
    error: string | null;
  }>;
  latest?: ComponentLatestSnapshot | null;
  post?: ReturnType<typeof vi.fn>;
} = {}) {
  const get = vi.fn((path: string) => {
    if (path === "/_aibox/api/tenants") return Promise.resolve(rows);
    if (path.startsWith("/_aibox/api/components?")) return Promise.resolve(components);
    if (path === "/_aibox/api/components/latest") return Promise.resolve(latest);
    return Promise.reject(new Error(`Unexpected GET ${path}`));
  });
  const api: TestControlApi = {
    bootstrap: { version: "test", csrf_token: "token" },
    get,
    post,
  };
  return { api, get, post };
}
