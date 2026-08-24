import { HttpError, readHttpError } from "./httpError";
import { tenantBody, tenantQuery, type TenantSelection } from "./tenantSelection";
import type { BodyKind, EventTimingIndex, RequestDetail, RequestList, RequestsApi } from "./types";

export { tenantSelectionValue } from "./tenantSelection";
export type { TenantSelection } from "./tenantSelection";

export type CodingAgentKind = "codex" | "claude";

export interface Bootstrap {
  version: string;
  csrf_token: string;
  listen: string;
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
  agent: CodingAgentKind;
  current_config: TopologyCurrentConfig;
  named_configs: TopologyNamedConfigs;
  application: ApplicationStatus;
}

export interface TopologyComponents {
  entries: ComponentRow[];
  error?: string;
}

export type TopologyTenant = TenantRow & {
  agents: TopologyAgent[];
  components: TopologyComponents;
};

export interface TopologyData {
  tenants: TopologyTenant[];
}

export interface SessionSummaryData {
  count: number;
  warnings: string[];
  partial: boolean;
}

interface TenantRowBase {
  display_name: string;
  home: string;
  exists: boolean;
}

export type TenantRow =
  | (TenantRowBase & { kind: "host"; name: null })
  | (TenantRowBase & { kind: "managed"; name: string });

export type ComponentKind =
  "node" | "codex" | "claude" | "python" | "claude-statusline" | "codex-statusline" | "rust" | "go";
export type ComponentStatus =
  "not-installed" | "installed" | "incomplete" | "modified" | "unmanaged";

export interface ComponentRow {
  kind: ComponentKind;
  supports_version: boolean;
  status: ComponentStatus | null;
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
  visual_options?: ConfigVisualOption[];
  custom_provider?: ConfigCustomProvider;
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

export interface ConfigCustomProvider {
  included: boolean;
  name: string;
  base_url: string;
  request_proxy_route: boolean;
  proxy_routed: boolean;
}

export interface ConfigAuthData {
  mode: "chatgpt" | "api-key";
  api_key: string | null;
  extra_fields: boolean;
  warnings: string[];
}

export interface ConfigVisualOption {
  path: string;
  label: string;
  description: string;
  group: string;
  value_kind: "string" | "bool";
  enum_values: string[];
  sensitive: boolean;
  required: boolean;
  request_proxy_route: boolean;
  included: boolean;
  value?: string | boolean;
  proxy_routed: boolean;
}

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

export type PropagationOutcome =
  | { status: "updated" | "unchanged" }
  | { status: "conflict"; last_refresh: string }
  | { status: "newer"; target_last_refresh: string; source_last_refresh: string }
  | { status: "invalid" | "failed"; reason: string };

export interface PropagationPreview {
  plan_id: string;
  preview: {
    entries: Array<{ label: string; outcome: PropagationOutcome }>;
    updates: number;
  };
}

export interface PropagationReport {
  entries: Array<{ label: string; outcome: PropagationOutcome }>;
}

export interface SessionDetailHandlers {
  onMessage: (message: ConversationMessage) => void;
  onTool: (tool: ToolActivity) => void;
  onEvidence: (evidence: TranscriptEvidenceSummary) => void;
  onMeta: (meta: SessionDetailMeta) => void;
  onComplete: (stats: SessionDetailStats, warnings: string[]) => void;
}

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

export interface TenantApi {
  listTenants(signal?: AbortSignal): Promise<TenantRow[]>;
  listComponents(tenant: TenantSelection, signal?: AbortSignal): Promise<ComponentRow[]>;
  createTenant(name: string): Promise<void>;
  deleteTenants(names: string[]): Promise<void>;
  mutateComponent(
    tenant: TenantSelection,
    component: ComponentKind,
    install: boolean,
    version: string | null,
  ): Promise<Operation | object>;
}

export interface ConfigFileTarget {
  tenant: TenantSelection;
  agent: CodingAgentKind;
  current: boolean;
  config: string | null;
  file: string;
}

export interface ConfigApi {
  bootstrap: Bootstrap;
  listTenants(signal?: AbortSignal): Promise<TenantRow[]>;
  listConfigs(
    tenant: TenantSelection,
    agent: CodingAgentKind,
    signal?: AbortSignal,
  ): Promise<ConfigListData>;
  revealConfigFile(target: ConfigFileTarget): Promise<ConfigFileData>;
  diagnoseConfigFile(
    target: ConfigFileTarget,
    contentBase64: string,
  ): Promise<{
    diagnostics: Array<{ severity?: string; message: string; line: number; column: number }>;
  }>;
  saveConfigFile(
    target: ConfigFileTarget,
    input: {
      revision: string;
      contentBase64: string;
      visualOptions?: Array<{ path: string; included: boolean; value?: string | boolean }>;
      customProvider?: { included: boolean; name: string; base_url: string; proxy_routed: boolean };
      visualAuth?: { included: boolean; value: string };
    },
  ): Promise<ConfigFileData>;
  createConfig(tenant: TenantSelection, agent: CodingAgentKind, name: string): Promise<void>;
  applyConfig(tenant: TenantSelection, agent: CodingAgentKind, name: string): Promise<void>;
  deleteConfigs(tenant: TenantSelection, agent: CodingAgentKind, names: string[]): Promise<void>;
  previewCredentialPropagation(): Promise<PropagationPreview>;
  executeCredentialPropagation(planId: string): Promise<PropagationReport>;
}

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

export interface OperationApi {
  current(): Promise<Operation | null>;
  cancel(id: string): Promise<void>;
  subscribe(handlers: {
    onConnection: (state: "connected" | "reconnecting") => void;
    onOperation: (operation: Operation | null, gap: boolean) => void;
  }): () => void;
}

export interface ConnectedControlApi {
  bootstrap: Bootstrap;
  overview: OverviewApi;
  tenants: TenantApi;
  configs: ConfigApi;
  sessions: SessionApi;
  requests: RequestsApi;
  operations: OperationApi;
}

export { HttpError as ApiError } from "./httpError";

export class ControlApi {
  readonly bootstrap: Bootstrap;
  private readonly fetchImpl: typeof fetch;

  constructor(bootstrap: Bootstrap, fetchImpl: typeof fetch = fetch) {
    this.bootstrap = bootstrap;
    this.fetchImpl = fetchImpl;
  }

  static async connect(fetchImpl: typeof fetch = fetch): Promise<ControlApi> {
    const response = await fetchImpl.call(window, "/_aibox/api/bootstrap", { cache: "no-store" });
    if (!response.ok) throw new HttpError(await readHttpError(response), response.status);
    return new ControlApi((await response.json()) as Bootstrap, fetchImpl);
  }

  async get<T>(path: string, signal?: AbortSignal): Promise<T> {
    const response = await this.getResponse(path, signal);
    return (await response.json()) as T;
  }

  async getResponse(path: string, signal?: AbortSignal): Promise<Response> {
    const response = await this.fetchImpl.call(window, path, { cache: "no-store", signal });
    if (!response.ok) throw new HttpError(await readHttpError(response), response.status);
    return response;
  }

  async post<T>(path: string, body: object = {}, signal?: AbortSignal): Promise<T> {
    const response = await this.fetchImpl.call(window, path, {
      method: "POST",
      cache: "no-store",
      headers: {
        "Content-Type": "application/json",
        "X-Aibox-Csrf": this.bootstrap.csrf_token,
      },
      body: JSON.stringify(body),
      signal,
    });
    if (!response.ok) throw new HttpError(await readHttpError(response), response.status);
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
      throw new HttpError(await readHttpError(response), response.status);
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
    if (!response.ok) throw new HttpError(await readHttpError(response), response.status);
    return (await response.json()) as TranscriptEvidence;
  }
}

export function composeControlApi(client: ControlApi): ConnectedControlApi {
  const listTenants = (signal?: AbortSignal) =>
    client.get<TenantRow[]>("/_aibox/api/tenants", signal);
  const configTargetBody = (target: ConfigFileTarget) => ({
    ...tenantBody(target.tenant),
    agent: target.agent,
    current: target.current,
    config: target.config,
    file: target.file,
  });
  const requestPath = (id: string) => `/_aibox/api/requests/${encodeURIComponent(id)}`;

  return {
    bootstrap: client.bootstrap,
    overview: {
      loadOverview: (signal) => client.get<OverviewData>("/_aibox/api/overview", signal),
      loadTopology: (signal) => client.get<TopologyData>("/_aibox/api/topology", signal),
      loadSessionSummary: (tenant, agent, signal) => {
        const query = tenantQuery(tenant);
        query.set("agent", agent);
        return client.get<SessionSummaryData>(`/_aibox/api/sessions/summary?${query}`, signal);
      },
      buildImage: (force) => client.post<Operation>("/_aibox/api/operations/build", { force }),
    },
    tenants: {
      listTenants,
      listComponents: (tenant, signal) =>
        client.get<ComponentRow[]>(`/_aibox/api/components?${tenantQuery(tenant)}`, signal),
      createTenant: async (name) => {
        await client.post("/_aibox/api/tenants", { name });
      },
      deleteTenants: async (names) => {
        await client.post("/_aibox/api/tenants/delete", {
          names,
          all: false,
          confirmation: names.length === 1 ? names[0] : "",
        });
      },
      mutateComponent: (tenant, component, install, version) =>
        client.post<Operation | object>(
          `/_aibox/api/components/${install ? "install" : "remove"}`,
          {
            ...tenantBody(tenant),
            component,
            version,
          },
        ),
    },
    configs: {
      bootstrap: client.bootstrap,
      listTenants,
      listConfigs: (tenant, agent, signal) => {
        const query = tenantQuery(tenant);
        query.set("agent", agent);
        return client.get<ConfigListData>(`/_aibox/api/configs?${query}`, signal);
      },
      revealConfigFile: (target) =>
        client.post<ConfigFileData>("/_aibox/api/configs/reveal", configTargetBody(target)),
      diagnoseConfigFile: (target, contentBase64) =>
        client.post("/_aibox/api/configs/diagnose", {
          ...configTargetBody(target),
          content_base64: contentBase64,
        }),
      saveConfigFile: (target, input) =>
        client.post<ConfigFileData>("/_aibox/api/configs/save", {
          ...configTargetBody(target),
          revision: input.revision,
          content_base64: input.contentBase64,
          ...(input.visualOptions ? { visual_options: input.visualOptions } : {}),
          ...(input.customProvider ? { custom_provider: input.customProvider } : {}),
          ...(input.visualAuth ? { visual_auth: input.visualAuth } : {}),
        }),
      createConfig: async (tenant, agent, config) => {
        await client.post("/_aibox/api/configs/create", { ...tenantBody(tenant), agent, config });
      },
      applyConfig: async (tenant, agent, config) => {
        await client.post("/_aibox/api/configs/apply", { ...tenantBody(tenant), agent, config });
      },
      deleteConfigs: async (tenant, agent, configs) => {
        await client.post("/_aibox/api/configs/delete", {
          ...tenantBody(tenant),
          agent,
          configs,
          all: false,
          confirmation: configs.length === 1 ? configs[0] : "",
        });
      },
      previewCredentialPropagation: () =>
        client.post<PropagationPreview>("/_aibox/api/configs/propagate-auth/preview"),
      executeCredentialPropagation: (planId) =>
        client.post<PropagationReport>("/_aibox/api/configs/propagate-auth/execute", {
          plan_id: planId,
        }),
    },
    sessions: {
      listTenants,
      listSessions: (tenant, agent, signal) => {
        const query = tenantQuery(tenant);
        query.set("agent", agent);
        return client.get<SessionListData>(`/_aibox/api/sessions?${query}`, signal);
      },
      streamSessionDetail: (tenant, agent, id, handlers, signal) => {
        const query = tenantQuery(tenant);
        query.set("agent", agent);
        query.set("id", id);
        return client.streamSessionDetail(`/_aibox/api/sessions/detail?${query}`, handlers, signal);
      },
      loadSessionEvidence: (tenant, agent, id, entry, snapshot, signal) => {
        const query = tenantQuery(tenant);
        query.set("agent", agent);
        query.set("id", id);
        query.set("entry", entry);
        query.set("snapshot", snapshot);
        return client.loadSessionEvidence(`/_aibox/api/sessions/evidence?${query}`, signal);
      },
      deleteSessions: (tenant, agent, ids) =>
        client.post("/_aibox/api/sessions/delete", {
          ...tenantBody(tenant),
          agent,
          ids,
          all: false,
          confirmation: "",
        }),
    },
    requests: {
      listRequests: (page = 1, signal) => {
        const query = page === 1 ? "" : `?page=${page}`;
        return client.get<RequestList>(`/_aibox/api/requests${query}`, signal);
      },
      getRequest: (id, signal) => client.get<RequestDetail>(requestPath(id), signal),
      loadBody: async (id, kind: BodyKind, offset, signal) => {
        const response = await client.getResponse(
          `${requestPath(id)}/${kind}-body?offset=${offset}`,
          signal,
        );
        const bytes = new Uint8Array(await response.arrayBuffer());
        const header = response.headers.get("X-Aibox-Request-Next-Offset");
        const fallbackOffset = offset + bytes.length;
        const advertisedOffset = header === null ? null : Number(header);
        const nextOffset =
          advertisedOffset !== null &&
          Number.isSafeInteger(advertisedOffset) &&
          advertisedOffset === fallbackOffset
            ? advertisedOffset
            : fallbackOffset;
        return { bytes, nextOffset };
      },
      loadDecodedBody: async (id, kind: BodyKind, signal) => {
        const response = await client.getResponse(
          `${requestPath(id)}/${kind}-body-decoded`,
          signal,
        );
        return new Uint8Array(await response.arrayBuffer());
      },
      loadEventTimings: (id, afterSequence, signal) =>
        client.get<EventTimingIndex>(
          `${requestPath(id)}/response-event-timings?after_sequence=${afterSequence}`,
          signal,
        ),
      deleteRequests: (ids, signal) =>
        client
          .post<{ deleted: number }>("/_aibox/api/requests/delete", { ids }, signal)
          .then((value) => value.deleted),
    },
    operations: {
      current: () =>
        client
          .get<{ operation: Operation | null }>("/_aibox/api/operations/current")
          .then((value) => value.operation),
      cancel: async (id) => {
        await client.post(`/_aibox/api/operations/${encodeURIComponent(id)}/cancel`);
      },
      subscribe: (handlers) => {
        const source = new EventSource("/_aibox/api/operations/events");
        source.addEventListener("open", () => handlers.onConnection("connected"));
        source.addEventListener("error", () => handlers.onConnection("reconnecting"));
        source.addEventListener("operation", (event) => {
          const value = JSON.parse((event as MessageEvent<string>).data) as {
            operation: Operation | null;
            gap: boolean;
          };
          handlers.onOperation(value.operation, value.gap);
        });
        return () => source.close();
      },
    },
  };
}

export async function connectControlApi(fetchImpl: typeof fetch = fetch) {
  return composeControlApi(await ControlApi.connect(fetchImpl));
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
