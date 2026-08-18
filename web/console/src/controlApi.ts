export type Agent = "codex" | "claude";
export type Scope = { scope: "host" } | { scope: "managed"; tenant: string };

export interface Bootstrap {
  version: string;
  csrf_token: string;
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
  version: string;
  listen: string;
  uptime_seconds: number;
  aibox_root: string;
  docker: string;
  docker_error: string | null;
  runtime_image: string;
  image_available: boolean;
  managed_tenants: number;
  request_records: number;
  request_bytes: number;
  operation: Operation | null;
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
}

export interface SessionRow {
  id: string;
  display_id: string;
  start_ts: string;
  title: string;
  warnings: string[];
}

export interface SessionListData {
  sessions: SessionRow[];
  warnings: string[];
  partial: boolean;
}

export interface Prompt {
  timestamp: string;
  text: string;
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

  async streamSession(
    path: string,
    onPrompt: (prompt: Prompt) => void,
    signal?: AbortSignal,
  ): Promise<{ id: string; warnings: string[] }> {
    const response = await this.fetchImpl.call(window, path, { cache: "no-store", signal });
    if (!response.ok || !response.body) {
      throw new ApiError(await errorMessage(response), response.status);
    }
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let pending = "";
    let complete: { id: string; warnings: string[] } | null = null;
    while (true) {
      const chunk = await reader.read();
      pending += decoder.decode(chunk.value, { stream: !chunk.done });
      const lines = pending.split("\n");
      pending = lines.pop() ?? "";
      for (const line of lines) {
        if (!line) continue;
        const record = JSON.parse(line) as
          | { type: "prompt"; prompt: Prompt }
          | { type: "complete"; id: string; warnings: string[] }
          | { type: "error"; error: string };
        if (record.type === "prompt") onPrompt(record.prompt);
        if (record.type === "complete") {
          complete = { id: record.id, warnings: record.warnings };
        }
        if (record.type === "error") throw new Error(record.error);
      }
      if (chunk.done) break;
    }
    if (!complete) throw new Error("Session stream ended before completion");
    return complete;
  }
}

export function scopeQuery(scope: Scope): URLSearchParams {
  return new URLSearchParams(
    scope.scope === "host" ? { scope: "host" } : { scope: "managed", tenant: scope.tenant },
  );
}

export function scopeBody(scope: Scope): Record<string, string> {
  return scope.scope === "host" ? { scope: "host" } : { scope: "managed", tenant: scope.tenant };
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
