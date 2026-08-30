import type { TenantRow } from "@/api/core";
import type { CodingAgentKind } from "@/domain/codingAgent";
import type {
  ConversationMessage,
  SessionDetailMeta,
  SessionDetailStats,
  SessionDiscoverySummary,
  SessionDetailFrame,
  SessionListData,
  SessionListRow,
  ToolActivity,
  TranscriptEvidence,
  TranscriptEvidenceSummary,
} from "@/api/generated/wire";
import { listTenantsRequest } from "@/api/tenants";
import type { ControlApi } from "@/api/transport";
import { tenantBody, tenantQuery } from "@/api/tenantSelection";
import type { TenantSelection } from "@/domain/tenant";

export type SessionRow = SessionListRow;
export type SessionSummaryData = SessionDiscoverySummary;
export type {
  ConversationMessage,
  SessionDetailMeta,
  SessionDetailStats,
  SessionListData,
  ToolActivity,
  TranscriptEvidence,
  TranscriptEvidenceSummary,
};

export interface SessionDetailHandlers {
  onMessage: (message: ConversationMessage) => void;
  onTool: (tool: ToolActivity) => void;
  onEvidence: (evidence: TranscriptEvidenceSummary) => void;
  onMeta: (meta: SessionDetailMeta) => void;
  onComplete: (stats: SessionDetailStats, warnings: string[]) => void;
}

export type { SessionDetailFrame };

export interface SessionApi {
  listTenants(signal?: AbortSignal): Promise<TenantRow[]>;
  listSessions(
    tenant: TenantSelection,
    agent: CodingAgentKind,
    signal?: AbortSignal,
  ): Promise<SessionListData>;
  streamSessionDetail(
    tenant: TenantSelection,
    agent: CodingAgentKind,
    id: string,
    handlers: SessionDetailHandlers,
    signal?: AbortSignal,
  ): Promise<void>;
  loadSessionEvidence(
    tenant: TenantSelection,
    agent: CodingAgentKind,
    id: string,
    entry: string,
    snapshot: string,
    signal?: AbortSignal,
  ): Promise<TranscriptEvidence>;
  deleteSessions(
    tenant: TenantSelection,
    agent: CodingAgentKind,
    ids: string[],
  ): Promise<{ deleted: number }>;
}

function sessionSourceQuery(tenant: TenantSelection, agent: CodingAgentKind, id?: string) {
  const query = tenantQuery(tenant);
  query.set("agent", agent);
  if (id !== undefined) query.set("id", id);
  return query;
}

export function sessionDetailPath(
  tenant: TenantSelection,
  agent: CodingAgentKind,
  id: string,
): string {
  return `/_aibox/api/sessions/detail?${sessionSourceQuery(tenant, agent, id)}`;
}

async function streamSessionDetail(
  client: ControlApi,
  path: string,
  handlers: SessionDetailHandlers,
  signal?: AbortSignal,
): Promise<void> {
  let complete = false;
  await client.streamNdjson<SessionDetailFrame>(
    path,
    (record) => {
      if (record.type === "message") handlers.onMessage(record.message);
      if (record.type === "tool_activity") handlers.onTool(record.tool_activity);
      if (record.type === "evidence") handlers.onEvidence(record.evidence);
      if (record.type === "meta") handlers.onMeta(record.meta);
      if (record.type === "complete") {
        complete = true;
        handlers.onComplete(record.stats, record.warnings);
      }
      if (record.type === "error") throw new Error(record.error);
    },
    signal,
  );
  if (!complete) throw new Error("Session detail stream ended before completion");
}

export function sessionsApi(client: ControlApi): SessionApi {
  return {
    listTenants: listTenantsRequest(client),
    listSessions: (tenant, agent, signal) =>
      client.get<SessionListData>(
        `/_aibox/api/sessions?${sessionSourceQuery(tenant, agent)}`,
        signal,
      ),
    streamSessionDetail: (tenant, agent, id, handlers, signal) =>
      streamSessionDetail(client, sessionDetailPath(tenant, agent, id), handlers, signal),
    loadSessionEvidence: (tenant, agent, id, entry, snapshot, signal) => {
      const query = sessionSourceQuery(tenant, agent, id);
      query.set("entry", entry);
      query.set("snapshot", snapshot);
      return client.get<TranscriptEvidence>(`/_aibox/api/sessions/evidence?${query}`, signal);
    },
    deleteSessions: (tenant, agent, ids) =>
      client.post("/_aibox/api/sessions/delete", {
        ...tenantBody(tenant),
        agent,
        ids,
        all: false,
        confirmation: "",
      }),
  };
}

export function sessionSummaryRequest(client: ControlApi) {
  return (tenant: TenantSelection, agent: CodingAgentKind, signal?: AbortSignal) =>
    client.get<SessionSummaryData>(
      `/_aibox/api/sessions/summary?${sessionSourceQuery(tenant, agent)}`,
      signal,
    );
}
