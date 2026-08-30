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
  requestProxyRoute,
  splitRequestProxyValue,
} from "@/features/configs/configCatalog";
import type { ConfigSelection } from "@/features/configs/route";

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
    config: selection.current ? null : selection.config,
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
      visualOptions: value.visual_options ?? null,
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
          visualOptions: visualOptions.map(({ path, included, value }) => ({
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
