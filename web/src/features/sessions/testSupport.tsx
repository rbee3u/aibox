import { useMemo } from "react";
import type { ComponentProps } from "react";
import { vi } from "vitest";
import type { SessionApi, SessionListData, SessionRow } from "@/api/sessions";
import { SessionPage as SessionPageView } from "@/features/sessions/SessionPage";
import { TENANT_ROWS as tenants } from "@/features/common/testFixtures";
import { useTestLocation } from "@/test/useTestLocation";

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

export function SessionPage(
  props: Omit<ComponentProps<typeof SessionPageView>, "search" | "onLocationChange"> & {
    api: SessionApi;
    search?: string;
    onLocationChange?: ComponentProps<typeof SessionPageView>["onLocationChange"];
  },
) {
  const { api, search, onLocationChange: notify, ...pageProps } = props;
  const location = useTestLocation(search, notify);
  const sessionApi = useMemo(() => api, [api]);
  return (
    <SessionPageView
      api={sessionApi}
      search={location.currentSearch}
      onLocationChange={location.onLocationChange}
      {...pageProps}
    />
  );
}

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

export const completeSessionDetail: SessionApi["streamSessionDetail"] = (
  _tenant,
  _agent,
  _id,
  handlers,
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
};

export function fakeApi({
  sessions = () => Promise.resolve(list([firstSession, secondSession])),
  listTenants = () => Promise.resolve(tenants),
  deleteSessions = () => Promise.reject(new Error("deleteSessions was not configured")),
  loadSessionEvidence = () => Promise.reject(new Error("No Session evidence configured")),
  streamSessionDetail = () => Promise.reject(new Error("streamSessionDetail was not configured")),
}: {
  sessions?: (
    ...args: Parameters<SessionApi["listSessions"]>
  ) => SessionListData | Promise<SessionListData>;
  listTenants?: SessionApi["listTenants"];
  deleteSessions?: SessionApi["deleteSessions"];
  loadSessionEvidence?: SessionApi["loadSessionEvidence"];
  streamSessionDetail?: (
    ...args: Parameters<SessionApi["streamSessionDetail"]>
  ) => void | Promise<void>;
} = {}) {
  const listTenantsSpy = vi.fn<SessionApi["listTenants"]>(listTenants);
  const listSessions = vi.fn<SessionApi["listSessions"]>(async (...args) => sessions(...args));
  const streamDetail = vi.fn<SessionApi["streamSessionDetail"]>(async (...args) => {
    await streamSessionDetail(...args);
  });
  const loadEvidence = vi.fn<SessionApi["loadSessionEvidence"]>(loadSessionEvidence);
  const deleteSessionRows = vi.fn<SessionApi["deleteSessions"]>(deleteSessions);
  const api = {
    listTenants: listTenantsSpy,
    listSessions,
    streamSessionDetail: streamDetail,
    loadSessionEvidence: loadEvidence,
    deleteSessions: deleteSessionRows,
  } satisfies SessionApi;
  return {
    api,
    listTenants: listTenantsSpy,
    listSessions,
    streamSessionDetail: streamDetail,
    loadSessionEvidence: loadEvidence,
    deleteSessions: deleteSessionRows,
  };
}
