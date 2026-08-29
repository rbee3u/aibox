import type { Bootstrap, TenantRow } from "@/api/core";
import type { CodingAgentKind } from "@/domain/codingAgent";
import type {
  ApplicationStatus,
  AuthPropagationPreviewResponse,
  AuthPropagationReport,
  ConfigCatalogEntry,
  ConfigFileResponse as GeneratedConfigFileResponse,
  ConfigListResponse as GeneratedConfigListResponse,
  ConfigAuthResponse as GeneratedConfigAuthResponse,
  LinkedConfigFileResponse as GeneratedLinkedConfigFileResponse,
  CustomProviderState as GeneratedCustomProviderState,
  VisualConfigOptionState as GeneratedVisualConfigOptionState,
  DiagnoseConfigResponse,
  LastApplication,
  PropagationOutcome,
} from "@/api/generated/wire";
import { listTenantsRequest } from "@/api/tenants";
import type { ControlApi } from "@/api/transport";
import { tenantBody, tenantQuery } from "@/api/tenantSelection";
import type { TenantSelection } from "@/domain/tenant";

export type { ApplicationStatus, ConfigCatalogEntry, LastApplication, PropagationOutcome };

export type ConfigListData = GeneratedConfigListResponse;

export type ConfigLinkedFileData = GeneratedLinkedConfigFileResponse;

export type ConfigCustomProvider = GeneratedCustomProviderState;

export type ConfigAuthData = Omit<GeneratedConfigAuthResponse, "mode"> & {
  mode: "chatgpt" | "api-key";
};

export type ConfigVisualOption = Omit<GeneratedVisualConfigOptionState, "value" | "value_kind"> & {
  value_kind: "string" | "bool";
  value?: string | boolean;
  proxy_routed: boolean;
};

export type ConfigFileData = Omit<
  GeneratedConfigFileResponse,
  "visual_options" | "custom_provider" | "visual_error" | "warnings" | "auth" | "linked_file"
> & {
  visual_options?: ConfigVisualOption[];
  custom_provider?: ConfigCustomProvider;
  visual_error?: string;
  warnings?: string[];
  auth?: ConfigAuthData;
  linked_file?: ConfigLinkedFileData;
};

export interface ConfigFileTarget {
  tenant: TenantSelection;
  agent: CodingAgentKind;
  current: boolean;
  config: string | null;
  file: string;
}

export type ConfigFileDiagnostics = DiagnoseConfigResponse;

export interface ConfigFileInput {
  revision: string;
  contentBase64: string;
  visualOptions?: Array<{ path: string; included: boolean; value?: string | boolean }>;
  customProvider?: { included: boolean; name: string; base_url: string; proxy_routed: boolean };
  visualAuth?: { included: boolean; value: string };
}

export type PropagationPreview = AuthPropagationPreviewResponse;

export type PropagationReport = AuthPropagationReport;

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

function visualOption(value: GeneratedVisualConfigOptionState): ConfigVisualOption {
  if (value.value_kind !== "string" && value.value_kind !== "bool") {
    throw new Error(`Unsupported Visual Config value kind: ${value.value_kind}`);
  }
  if (
    value.value !== null &&
    value.value !== undefined &&
    typeof value.value !== "string" &&
    typeof value.value !== "boolean"
  ) {
    throw new Error(`Unsupported Visual Config value for ${value.path}`);
  }
  const { value: rawValue, ...rest } = value;
  return {
    ...rest,
    value_kind: value.value_kind,
    proxy_routed: false,
    ...(rawValue === null || rawValue === undefined ? {} : { value: rawValue }),
  };
}

function authData(value: GeneratedConfigAuthResponse): ConfigAuthData {
  if (value.mode !== "chatgpt" && value.mode !== "api-key") {
    throw new Error(`Unsupported Config auth mode: ${value.mode}`);
  }
  return { ...value, mode: value.mode };
}

function configFileData(value: GeneratedConfigFileResponse): ConfigFileData {
  return {
    file: value.file,
    exists: value.exists,
    revision: value.revision,
    content_base64: value.content_base64,
    visual_options: value.visual_options?.map(visualOption) ?? undefined,
    custom_provider: value.custom_provider ?? undefined,
    visual_error: value.visual_error ?? undefined,
    warnings: value.warnings,
    auth: value.auth ? authData(value.auth) : undefined,
    linked_file: value.linked_file ?? undefined,
  };
}

export function configsApi(client: ControlApi): ConfigApi {
  return {
    bootstrap: client.bootstrap,
    listTenants: listTenantsRequest(client),
    listConfigs: (tenant, agent, signal) => {
      const query = tenantQuery(tenant);
      query.set("agent", agent);
      return client.get<GeneratedConfigListResponse>(`/_aibox/api/configs?${query}`, signal);
    },
    revealConfigFile: (target) =>
      client
        .post<GeneratedConfigFileResponse>("/_aibox/api/configs/reveal", configTargetBody(target))
        .then(configFileData),
    diagnoseConfigFile: (target, contentBase64) =>
      client.post<DiagnoseConfigResponse>("/_aibox/api/configs/diagnose", {
        ...configTargetBody(target),
        content_base64: contentBase64,
      }),
    saveConfigFile: (target, input) =>
      client
        .post<GeneratedConfigFileResponse>("/_aibox/api/configs/save", {
          ...configTargetBody(target),
          revision: input.revision,
          content_base64: input.contentBase64,
          ...(input.visualOptions ? { visual_options: input.visualOptions } : {}),
          ...(input.customProvider ? { custom_provider: input.customProvider } : {}),
          ...(input.visualAuth ? { visual_auth: input.visualAuth } : {}),
        })
        .then(configFileData),
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
