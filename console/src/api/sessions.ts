import type { CodingAgentKind, TenantRow } from "@/api/core";
import { listTenantsRequest } from "@/api/tenants";
import type { ControlApi } from "@/api/transport";
import { tenantBody, tenantQuery, type TenantSelection } from "@/api/tenantSelection";

export interface SessionRow {
  id: string;
  display_id: string;
  start_ts: string;
  title: string;
  latest_message: string;
  message_count: number;
  tool_count: number;
  warnings: string[];
}

export interface SessionListData {
  sessions: SessionRow[];
  warnings: string[];
  partial: boolean;
}

export interface SessionSummaryData {
  count: number;
  warnings: string[];
  partial: boolean;
}

export interface ConversationMessage {
  entry_ids: string[];
  role: "user" | "assistant";
  timestamp: string;
  text: string;
}

export interface ToolActivity {
  entry_ids: string[];
  call_id: string | null;
  timestamp: string;
  name: string;
  status: "started" | "completed" | "failed" | "incomplete" | "unknown";
  summary: string;
}

export interface TranscriptEvidenceSummary {
  entry_id: string;
  line: number;
  timestamp: string;
  native_type: string;
  role: string | null;
  content_types: string[];
  status: string;
  preview: string;
}

export interface SessionDetailMeta {
  id: string;
  title: string;
  start_ts: string;
  transcript_path: string;
  cwd: string | null;
  model_provider: string | null;
  cli_version: string | null;
}

export interface SessionDetailStats {
  start_ts: string;
  last_event_ts: string;
  message_count: number;
  tool_count: number;
  entry_count: number;
  malformed_count: number;
  unsupported_count: number;
  hidden_internal_count: number;
  observed_duration_ms: number | null;
  file_size: number;
  snapshot: string;
}

export interface TranscriptEvidence {
  entry_id: string;
  encoding: "utf-8" | "base64";
  content: string;
  snapshot: string;
}

export interface SessionDetailHandlers {
  onMessage: (message: ConversationMessage) => void;
  onTool: (tool: ToolActivity) => void;
  onEvidence: (evidence: TranscriptEvidenceSummary) => void;
  onMeta: (meta: SessionDetailMeta) => void;
  onComplete: (stats: SessionDetailStats, warnings: string[]) => void;
}

type SessionDetailFrame =
  | { type: "message"; message: ConversationMessage }
  | { type: "tool_activity"; tool_activity: ToolActivity }
  | { type: "evidence"; evidence: TranscriptEvidenceSummary }
  | { type: "meta"; meta: SessionDetailMeta }
  | { type: "complete"; stats: SessionDetailStats; warnings: string[] }
  | { type: "error"; error: string };

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

function sessionScopeQuery(tenant: TenantSelection, agent: CodingAgentKind, id?: string) {
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
  return `/_aibox/api/sessions/detail?${sessionScopeQuery(tenant, agent, id)}`;
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
        `/_aibox/api/sessions?${sessionScopeQuery(tenant, agent)}`,
        signal,
      ),
    streamSessionDetail: (tenant, agent, id, handlers, signal) =>
      streamSessionDetail(client, sessionDetailPath(tenant, agent, id), handlers, signal),
    loadSessionEvidence: (tenant, agent, id, entry, snapshot, signal) => {
      const query = sessionScopeQuery(tenant, agent, id);
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
      `/_aibox/api/sessions/summary?${sessionScopeQuery(tenant, agent)}`,
      signal,
    );
}
