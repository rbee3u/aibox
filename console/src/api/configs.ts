import type { Bootstrap, CodingAgentKind, TenantRow } from "@/api/core";
import { listTenantsRequest } from "@/api/tenants";
import type { ControlApi } from "@/api/transport";
import { tenantBody, tenantQuery, type TenantSelection } from "@/api/tenantSelection";

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

export interface ConfigFileTarget {
  tenant: TenantSelection;
  agent: CodingAgentKind;
  current: boolean;
  config: string | null;
  file: string;
}

export interface ConfigFileDiagnostics {
  diagnostics: Array<{ severity?: string; message: string; line: number; column: number }>;
}

export interface ConfigFileInput {
  revision: string;
  contentBase64: string;
  visualOptions?: Array<{ path: string; included: boolean; value?: string | boolean }>;
  customProvider?: { included: boolean; name: string; base_url: string; proxy_routed: boolean };
  visualAuth?: { included: boolean; value: string };
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
  ): Promise<ConfigFileDiagnostics>;
  saveConfigFile(target: ConfigFileTarget, input: ConfigFileInput): Promise<ConfigFileData>;
  createConfig(tenant: TenantSelection, agent: CodingAgentKind, name: string): Promise<void>;
  applyConfig(tenant: TenantSelection, agent: CodingAgentKind, name: string): Promise<void>;
  deleteConfigs(tenant: TenantSelection, agent: CodingAgentKind, names: string[]): Promise<void>;
  previewCredentialPropagation(): Promise<PropagationPreview>;
  executeCredentialPropagation(planId: string): Promise<PropagationReport>;
}

function configTargetBody(target: ConfigFileTarget) {
  return {
    ...tenantBody(target.tenant),
    agent: target.agent,
    current: target.current,
    config: target.config,
    file: target.file,
  };
}

export function configsApi(client: ControlApi): ConfigApi {
  return {
    bootstrap: client.bootstrap,
    listTenants: listTenantsRequest(client),
    listConfigs: (tenant, agent, signal) => {
      const query = tenantQuery(tenant);
      query.set("agent", agent);
      return client.get<ConfigListData>(`/_aibox/api/configs?${query}`, signal);
    },
    revealConfigFile: (target) =>
      client.post<ConfigFileData>("/_aibox/api/configs/reveal", configTargetBody(target)),
    diagnoseConfigFile: (target, contentBase64) =>
      client.post<ConfigFileDiagnostics>("/_aibox/api/configs/diagnose", {
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
  };
}
