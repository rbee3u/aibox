import type {
  ConfigAuthData,
  ConfigCustomProvider,
  ConfigFileData,
  ConfigFileInput,
  ConfigFileTarget,
  ConfigVisualOption,
} from "@/api/configs";
import { decodeBase64, encodeBase64 } from "@/shared/lib/encoding";
import type { CodingAgentKind } from "@/domain/codingAgent";
import type { TenantSelection } from "@/domain/tenant";
import {
  comparableProvider,
  proxyValueIsValid,
  requestProxyRoute,
  splitRequestProxyValue,
} from "@/features/configs/configCatalog";
import { namedConfigName, type ConfigSelection } from "@/features/configs/route";

export function visualOptionAvailable(
  field: ConfigVisualOption,
  fields: ConfigVisualOption[],
): boolean {
  const condition = field.visible_when;
  return (
    !condition ||
    fields.some(
      (parent) =>
        parent.path === condition.path && parent.included && parent.value === condition.value,
    )
  );
}

export function omitUnavailableOptions(fields: ConfigVisualOption[]): ConfigVisualOption[] {
  return fields.map((field) =>
    visualOptionAvailable(field, fields) ? field : { ...field, included: false },
  );
}

export interface ConfigFileSnapshotModel {
  editor: string;
  textEditable: boolean;
  visualOptions: ConfigVisualOption[] | null;
  customProvider: ConfigCustomProvider | null;
  auth?: { mode: ConfigAuthData["mode"]; key: string };
}

export function configFileTarget(
  tenant: TenantSelection,
  agent: CodingAgentKind,
  selection: ConfigSelection,
  file: string,
): ConfigFileTarget {
  return {
    tenant,
    agent,
    current: selection.current,
    config: namedConfigName(selection),
    file,
  };
}

export function configFileSnapshotModel(
  value: ConfigFileData,
  tenant: TenantSelection,
  listen: string | undefined,
  isAuth: boolean,
): ConfigFileSnapshotModel {
  try {
    const editor = new TextDecoder("utf-8", { fatal: true }).decode(
      decodeBase64(value.content_base64),
    );
    let customProvider: ConfigCustomProvider | null = null;
    if (value.custom_provider) {
      const split = splitRequestProxyValue(
        value.custom_provider.base_url,
        requestProxyRoute(tenant, listen),
      );
      customProvider = {
        ...value.custom_provider,
        base_url: value.custom_provider.base_url,
        proxy_routed: split.routed,
      };
    }
    return {
      editor,
      textEditable: true,
      visualOptions: value.visual_options ? omitUnavailableOptions(value.visual_options) : null,
      customProvider,
      ...(isAuth && value.auth
        ? { auth: { mode: value.auth.mode, key: value.auth.api_key ?? "" } }
        : {}),
    };
  } catch {
    return {
      editor: "",
      textEditable: false,
      visualOptions: null,
      customProvider: null,
    };
  }
}

export function configEditorBytes(
  snapshot: ConfigFileData | null,
  textEditable: boolean,
  editor: string,
): Uint8Array | null {
  return snapshot && textEditable ? new TextEncoder().encode(editor) : null;
}

export function configFileDirty({
  mode,
  isAuth,
  snapshot,
  editorBytes,
  visualOptions,
  customProvider,
  authMode,
  authKey,
}: {
  mode: "visual" | "raw";
  isAuth: boolean;
  snapshot: ConfigFileData | null;
  editorBytes: Uint8Array | null;
  visualOptions: ConfigVisualOption[] | null;
  customProvider: ConfigCustomProvider | null;
  authMode: ConfigAuthData["mode"];
  authKey: string;
}): boolean {
  const visualDirty =
    Boolean(snapshot && visualOptions) &&
    JSON.stringify(
      visualOptions?.map(({ path, included, value }) => ({ path, included, value })),
    ) !==
      JSON.stringify(
        snapshot?.visual_options?.map(({ path, included, value }) => ({ path, included, value })),
      );
  const providerDirty = Boolean(
    snapshot &&
    customProvider &&
    JSON.stringify(comparableProvider(customProvider)) !==
      JSON.stringify(comparableProvider(snapshot.custom_provider)),
  );
  const authDirty =
    isAuth &&
    Boolean(snapshot?.auth) &&
    (authMode !== snapshot?.auth?.mode || authKey !== (snapshot?.auth?.api_key ?? ""));
  return mode === "visual"
    ? isAuth
      ? authDirty
      : visualDirty || providerDirty
    : editorBytes !== null &&
        snapshot !== null &&
        encodeBase64(editorBytes) !== snapshot.content_base64;
}

export function configFileCanSave(
  snapshot: ConfigFileData | null,
  textEditable: boolean,
  isAuth: boolean,
  authMode: ConfigAuthData["mode"],
  mode: "visual" | "raw",
): boolean {
  return Boolean(
    snapshot && textEditable && (isAuth ? authMode === "api-key" || mode === "raw" : true),
  );
}

export function configFileInput({
  snapshot,
  editorBytes,
  mode,
  isAuth,
  visualOptions,
  customProvider,
  authKey,
}: {
  snapshot: ConfigFileData;
  editorBytes: Uint8Array;
  mode: "visual" | "raw";
  isAuth: boolean;
  visualOptions: ConfigVisualOption[] | null;
  customProvider: ConfigCustomProvider | null;
  authKey: string;
}): ConfigFileInput {
  return {
    revision: snapshot.revision,
    contentBase64: encodeBase64(editorBytes),
    ...(mode === "visual" && !isAuth && visualOptions
      ? {
          visualOptions: omitUnavailableOptions(visualOptions).map(({ path, included, value }) => ({
            path,
            included,
            value,
          })),
        }
      : {}),
    ...(mode === "visual" && !isAuth && customProvider
      ? {
          customProvider: {
            included: customProvider.included,
            name: customProvider.name,
            base_url: customProvider.base_url,
            proxy_routed: Boolean(customProvider.proxy_routed),
          },
        }
      : {}),
    ...(mode === "visual" && isAuth ? { visualAuth: { included: true, value: authKey } } : {}),
  };
}

/** Native paths the Custom provider aggregate edits, for marking its own inputs. */
export const customProviderPaths = {
  name: "model_providers.custom.name",
  baseUrl: "model_providers.custom.base_url",
} as const;

/** Why a save was refused, and which fields — by native path — to mark. */
export interface SaveFailure {
  message: string;
  paths: string[];
}

/**
 * The checks a Visual save runs before asking the Service, each naming the
 * field it is about so the form can mark it. Server refusals arrive with no
 * field and are reported with an empty path list.
 */
export function visualSaveFailure(
  fields: ConfigVisualOption[] | null,
  provider: ConfigCustomProvider | null,
  route: string | null,
): SaveFailure | null {
  if (provider?.included) {
    if (!provider.name.trim()) {
      return { message: "Custom provider name is required.", paths: [customProviderPaths.name] };
    }
    const upstream = splitRequestProxyValue(provider.base_url, route).upstream;
    if (!upstream.trim()) {
      return { message: "Base URL is required.", paths: [customProviderPaths.baseUrl] };
    }
    if (!proxyValueIsValid(upstream)) {
      return {
        message: "Base URL must be a valid HTTP or HTTPS URL.",
        paths: [customProviderPaths.baseUrl],
      };
    }
  }
  for (const field of fields ?? []) {
    if (!field.included) continue;
    const value = typeof field.value === "string" ? field.value : null;
    if (field.required && field.value_kind !== "bool" && (value === null || !value.trim())) {
      return { message: `${field.label} is required.`, paths: [field.path] };
    }
    if (field.request_proxy_route && value !== null) {
      const upstream = splitRequestProxyValue(value, route).upstream;
      if (!proxyValueIsValid(upstream)) {
        return {
          message: `${field.label} must be a valid HTTP or HTTPS URL.`,
          paths: [field.path],
        };
      }
    }
  }
  return null;
}
