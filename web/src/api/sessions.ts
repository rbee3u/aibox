import type { TenantRow } from "@/api/core";
import type { AgentKind } from "@/domain/agent";
import type {
  ConversationMessage,
  ConversationNotice,
  SessionDetailMeta,
  SessionDetailStats,
  SessionDetailFrame,
  SessionListData,
  SessionListRow,
  ToolActivity,
  TranscriptEvidence,
  TranscriptEvidenceSummary,
} from "@/api/generated/wire";
import { HttpError } from "@/api/httpError";
import { listTenantsRequest } from "@/api/tenants";
import type { ControlApi } from "@/api/transport";
import { tenantBody, tenantQuery } from "@/api/tenantSelection";
import type { TenantSelection } from "@/domain/tenant";

export type SessionRow = SessionListRow;
export type {
  ConversationMessage,
  ConversationNotice,
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
    agent: AgentKind,
    signal?: AbortSignal,
  ): Promise<SessionListData>;
  streamSessionDetail(
    tenant: TenantSelection,
    agent: AgentKind,
    id: string,
    handlers: SessionDetailHandlers,
    signal?: AbortSignal,
  ): Promise<void>;
  loadSessionEvidence(
    tenant: TenantSelection,
    agent: AgentKind,
    id: string,
    entry: string,
    snapshot: string,
    signal?: AbortSignal,
  ): Promise<TranscriptEvidence>;
  deleteSessions(
    tenant: TenantSelection,
    agent: AgentKind,
    ids: string[],
  ): Promise<{ deleted: number }>;
}

function sessionSourceQuery(tenant: TenantSelection, agent: AgentKind, id?: string) {
  const query = tenantQuery(tenant);
  query.set("agent", agent);
  if (id !== undefined) query.set("id", id);
  return query;
}

function sessionDetailPath(tenant: TenantSelection, agent: AgentKind, id: string): string {
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
      switch (record.type) {
        case "message":
          handlers.onMessage(record.message);
          break;
        case "tool_activity":
          handlers.onTool(record.tool_activity);
          break;
        case "evidence":
          handlers.onEvidence(record.evidence);
          break;
        case "meta":
          handlers.onMeta(record.meta);
          break;
        case "complete":
          complete = true;
          handlers.onComplete(record.stats, record.warnings);
          break;
        case "error":
          throw new Error(record.error);
      }
    },
    signal,
  );
  if (!complete) throw new Error("Session detail stream ended before completion");
}

/**
 * An evidence read is pinned to the snapshot its detail stream reported; the
 * Service refuses it once the Transcript has changed, so the caller re-reads.
 */
export function isTranscriptConflict(cause: unknown): boolean {
  return cause instanceof HttpError && cause.status === 409;
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
