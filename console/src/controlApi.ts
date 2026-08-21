export type Agent = "codex" | "claude";
export type TenantSelection = { kind: "host" } | { kind: "managed"; name: string };

export interface Bootstrap {
  version: string;
  csrf_token: string;
  listen?: string;
}

export interface OperationLog {
  sequence: number;
  message: string;
}

export interface Operation {
  id: string;
  kind: string;
  state: "running" | "succeeded" | "failed" | "cancelled";
  started_at: string;
  ended_at: string | null;
  result: string | null;
  first_sequence: number;
  next_sequence: number;
  logs: OperationLog[];
}

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
  agent: Agent;
  current_config: TopologyCurrentConfig;
  named_configs: TopologyNamedConfigs;
  application: ApplicationStatus;
}

export interface TopologyComponents {
  entries: ComponentRow[];
  error?: string;
}

export interface TopologyTenant extends TenantRow {
  agents: TopologyAgent[];
  components: TopologyComponents;
}

export interface TopologyData {
  tenants: TopologyTenant[];
}

export interface SessionSummaryData {
  count: number;
  warnings: string[];
  partial: boolean;
}

export interface TenantRow {
  kind: "host" | "managed";
  name: string | null;
  display_name: string;
  home: string;
  exists: boolean;
}

export interface ComponentRow {
  kind: string;
  supports_version: boolean;
  status: string | null;
  version: string | null;
  error: string | null;
}

export interface ConfigCatalogEntry {
  name: string;
  state: "ready" | "incomplete" | "invalid";
  detail?: string;
  warnings?: string[];
}

export interface LastApplication {
  applied: string;
  applied_at: string;
}

export interface ApplicationStatus {
  last_application: LastApplication | null;
  drift: "untracked" | "clean" | "dirty" | "source-missing" | "comparison-error";
  detail?: string;
}

export interface ConfigListData {
  named_configs: string[];
  configs: ConfigCatalogEntry[];
  files: string[];
  application: ApplicationStatus;
  credential_propagation_available: boolean;
}

export interface ConfigFileData {
  file: string;
  exists: boolean;
  revision: string;
  content_base64: string;
  visual?: ConfigVisualField[];
  visual_provider?: ConfigVisualProvider;
  visual_error?: string;
  warnings?: string[];
  auth?: ConfigAuthData;
  linked_file?: ConfigLinkedFileData;
}

export interface ConfigLinkedFileData {
  file: string;
  exists: boolean;
  revision: string;
  content_base64: string;
}

export interface ConfigVisualProvider {
  included: boolean;
  name: string;
  base_url: string;
  request_proxy_route: boolean;
  proxy_routed?: boolean;
}

export interface ConfigAuthData {
  mode: "chatgpt" | "api-key";
  api_key: string | null;
  extra_fields: boolean;
  warnings: string[];
}

export interface ConfigVisualField {
  path: string;
  label: string;
  description: string;
  group: string;
  value_kind: "string" | "bool";
  enum_values: string[];
  sensitive: boolean;
  required?: boolean;
  request_proxy_route?: boolean;
  included: boolean;
  value?: string | boolean;
  proxy_routed?: boolean;
}

export interface SessionRow {
  id: string;
  display_id: string;
  start_ts: string;
  title: string;
  latest_message?: string;
  message_count?: number;
  tool_count?: number;
  warnings: string[];
}

export interface SessionListData {
  sessions: SessionRow[];
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
  call_id?: string | null;
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
  role?: string | null;
  content_types: string[];
  status: string;
  preview: string;
}

export interface SessionDetailMeta {
  id: string;
  title: string;
  start_ts: string;
  transcript_path: string;
  cwd?: string | null;
  model_provider?: string | null;
  cli_version?: string | null;
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
  observed_duration_ms?: number | null;
  file_size: number;
  snapshot: string;
}

export interface TranscriptEvidence {
  entry_id: string;
  encoding: "utf-8" | "base64";
  content: string;
  snapshot: string;
}

export interface PropagationOutcome {
  status: "updated" | "unchanged" | "conflict" | "newer" | "invalid" | "failed";
  last_refresh?: string;
  target_last_refresh?: string;
  source_last_refresh?: string;
  reason?: string;
}

export interface PropagationPreview {
  plan_id: string;
  preview: {
    entries: Array<{ label: string; outcome: PropagationOutcome }>;
    updates: number;
  };
}

export interface PropagationReport {
  entries: Array<{ label: string; outcome: PropagationOutcome }>;
  counts: Record<string, number>;
}

export class ApiError extends Error {
  readonly status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

async function errorMessage(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as { error?: unknown };
    if (typeof body.error === "string") return body.error;
  } catch {
    // Fall through to the HTTP status when the server did not return JSON.
  }
  return `${response.status} ${response.statusText}`;
}

export class ControlApi {
  readonly bootstrap: Bootstrap;
  private readonly fetchImpl: typeof fetch;

  constructor(bootstrap: Bootstrap, fetchImpl: typeof fetch = fetch) {
    this.bootstrap = bootstrap;
    this.fetchImpl = fetchImpl;
  }

  static async connect(fetchImpl: typeof fetch = fetch): Promise<ControlApi> {
    const response = await fetchImpl.call(window, "/_aibox/api/bootstrap", { cache: "no-store" });
    if (!response.ok) throw new ApiError(await errorMessage(response), response.status);
    return new ControlApi((await response.json()) as Bootstrap, fetchImpl);
  }

  async get<T>(path: string, signal?: AbortSignal): Promise<T> {
    const response = await this.fetchImpl.call(window, path, { cache: "no-store", signal });
    if (!response.ok) throw new ApiError(await errorMessage(response), response.status);
    return (await response.json()) as T;
  }

  async post<T>(path: string, body: object = {}): Promise<T> {
    const response = await this.fetchImpl.call(window, path, {
      method: "POST",
      cache: "no-store",
      headers: {
        "Content-Type": "application/json",
        "X-Aibox-Csrf": this.bootstrap.csrf_token,
      },
      body: JSON.stringify(body),
    });
    if (!response.ok) throw new ApiError(await errorMessage(response), response.status);
    return (await response.json()) as T;
  }

  async streamSessionDetail(
    path: string,
    handlers: {
      onMessage: (message: ConversationMessage) => void;
      onTool: (tool: ToolActivity) => void;
      onEvidence: (evidence: TranscriptEvidenceSummary) => void;
      onMeta: (meta: SessionDetailMeta) => void;
      onComplete: (stats: SessionDetailStats, warnings: string[]) => void;
    },
    signal?: AbortSignal,
  ): Promise<void> {
    const response = await this.fetchImpl.call(window, path, { cache: "no-store", signal });
    if (!response.ok || !response.body) {
      throw new ApiError(await errorMessage(response), response.status);
    }
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let pending = "";
    let complete = false;
    while (true) {
      const chunk = await reader.read();
      pending += decoder.decode(chunk.value, { stream: !chunk.done });
      const lines = pending.split("\n");
      pending = lines.pop() ?? "";
      for (const line of lines) {
        if (!line) continue;
        const record = JSON.parse(line) as
          | { type: "message"; message: ConversationMessage }
          | { type: "tool_activity"; tool_activity: ToolActivity }
          | { type: "evidence"; evidence: TranscriptEvidenceSummary }
          | { type: "meta"; meta: SessionDetailMeta }
          | { type: "complete"; stats: SessionDetailStats; warnings: string[] }
          | { type: "error"; error: string };
        if (record.type === "message") handlers.onMessage(record.message);
        if (record.type === "tool_activity") handlers.onTool(record.tool_activity);
        if (record.type === "evidence") handlers.onEvidence(record.evidence);
        if (record.type === "meta") handlers.onMeta(record.meta);
        if (record.type === "complete") {
          complete = true;
          handlers.onComplete(record.stats, record.warnings);
        }
        if (record.type === "error") throw new Error(record.error);
      }
      if (chunk.done) break;
    }
    if (!complete) throw new Error("Session detail stream ended before completion");
  }

  async loadSessionEvidence(path: string, signal?: AbortSignal): Promise<TranscriptEvidence> {
    const response = await this.fetchImpl.call(window, path, { cache: "no-store", signal });
    if (!response.ok) throw new ApiError(await errorMessage(response), response.status);
    return (await response.json()) as TranscriptEvidence;
  }
}

export function tenantSelectionValue(tenant: TenantSelection): string {
  return tenant.kind === "host" ? "host" : `managed:${tenant.name}`;
}

export function tenantQuery(tenant: TenantSelection): URLSearchParams {
  return new URLSearchParams({ tenant: tenantSelectionValue(tenant) });
}

export function tenantBody(tenant: TenantSelection): Record<string, string> {
  return { tenant: tenantSelectionValue(tenant) };
}

export function decodeBase64(value: string): Uint8Array {
  const binary = window.atob(value);
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

export function encodeBase64(value: Uint8Array): string {
  let binary = "";
  for (let index = 0; index < value.length; index += 0x8000) {
    binary += String.fromCharCode(...value.subarray(index, index + 0x8000));
  }
  return window.btoa(binary);
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}
