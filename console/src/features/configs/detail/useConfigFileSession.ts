import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  ConfigApi,
  ConfigAuthData,
  ConfigCustomProvider,
  ConfigFileData,
  ConfigVisualOption,
} from "@/api/configs";
import { encodeBase64 } from "@/shared/lib/encoding";
import type { CodingAgentKind } from "@/domain/codingAgent";
import type { TenantSelection } from "@/domain/tenant";
import {
  proxyValueIsValid,
  requestProxyRoute,
  splitRequestProxyValue,
} from "@/features/configs/configCatalog";
import type { ConfigFileController } from "@/features/configs/detail/configFileController";
import {
  configEditorBytes,
  configFileCanSave,
  configFileDirty,
  configFileInput,
  configFileSnapshotModel,
  configFileTarget,
} from "@/features/configs/detail/configFileModel";
import {
  codeMirrorAvailable,
  useCodeMirrorEditor,
  type RawDiagnostic,
} from "@/features/configs/detail/useCodeMirrorEditor";
import type { ConfigSelection } from "@/features/configs/route";
import { messageOf } from "@/shared/lib/errors";

export interface ConfigFileSessionOptions {
  api: ConfigApi;
  tenant: TenantSelection;
  agent: CodingAgentKind;
  selection: ConfigSelection;
  file: string;
  mode: "visual" | "raw";
  operationBusy: boolean;
  onControllerChange: (file: string, controller: ConfigFileController | null) => void;
  onError: (message: string | null) => void;
  onRevealRetryChange: (file: string, retry: (() => void) | null) => void;
  onSaved: () => void;
  onBeforeSave?: (customProvider: boolean) => boolean;
  onLinkedFileSaved?: (file: string) => void;
  onVisualAvailable?: (available: boolean) => void;
}

export function useConfigFileSession({
  api,
  tenant,
  agent,
  selection,
  file,
  mode,
  operationBusy,
  onControllerChange,
  onError,
  onRevealRetryChange,
  onSaved,
  onBeforeSave,
  onLinkedFileSaved,
  onVisualAvailable,
}: ConfigFileSessionOptions) {
  const [snapshot, setSnapshot] = useState<ConfigFileData | null>(null);
  const [editor, setEditor] = useState("");
  const [visualOptions, setVisualOptions] = useState<ConfigVisualOption[] | null>(null);
  const [customProvider, setCustomProvider] = useState<ConfigCustomProvider | null>(null);
  const [textEditable, setTextEditable] = useState(true);
  const [rawDiagnostics, setRawDiagnostics] = useState<RawDiagnostic[]>([]);
  const [authMode, setAuthMode] = useState<ConfigAuthData["mode"]>("api-key");
  const [authKey, setAuthKey] = useState("");
  const [loading, setLoading] = useState(false);
  const [feedback, setFeedback] = useState<"idle" | "saving" | "saved">("idle");
  const [reloadNonce, setReloadNonce] = useState(0);
  const diagnoseTimer = useRef<number | null>(null);
  const diagnoseGeneration = useRef(0);
  const loadGeneration = useRef(0);
  const isAuth = file === "auth.json";
  const target = useMemo(
    () => configFileTarget(tenant, agent, selection, file),
    [agent, file, selection, tenant],
  );

  const diagnose = useCallback(
    (value: string) => {
      if (diagnoseTimer.current !== null) window.clearTimeout(diagnoseTimer.current);
      const generation = ++diagnoseGeneration.current;
      diagnoseTimer.current = window.setTimeout(() => {
        void api
          .diagnoseConfigFile(target, encodeBase64(new TextEncoder().encode(value)))
          .then((result) => {
            if (generation === diagnoseGeneration.current)
              setRawDiagnostics(Array.isArray(result.diagnostics) ? result.diagnostics : []);
          })
          .catch(() => {
            if (generation === diagnoseGeneration.current) setRawDiagnostics([]);
          });
      }, 250);
    },
    [api, target],
  );

  const setFromSnapshot = useCallback(
    (value: ConfigFileData) => {
      diagnoseGeneration.current += 1;
      if (diagnoseTimer.current !== null) window.clearTimeout(diagnoseTimer.current);
      const model = configFileSnapshotModel(value, tenant, api.bootstrap?.listen, isAuth);
      setEditor(model.editor);
      setTextEditable(model.textEditable);
      setRawDiagnostics([]);
      setVisualOptions(model.visualOptions);
      setCustomProvider(model.customProvider);
      if (model.auth) {
        setAuthMode(model.auth.mode);
        setAuthKey(model.auth.key);
      }
    },
    [api.bootstrap?.listen, isAuth, tenant],
  );

  useEffect(() => {
    const generation = ++loadGeneration.current;
    // A new file identity starts a fresh lifecycle before its external snapshot is loaded.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setLoading(true);
    setSnapshot(null);
    setEditor("");
    setVisualOptions(null);
    setCustomProvider(null);
    setRawDiagnostics([]);
    void api
      .revealConfigFile(target)
      .then((value) => {
        if (loadGeneration.current !== generation) return;
        onRevealRetryChange(file, null);
        setFromSnapshot(value);
        setSnapshot(value);
        if (!isAuth) onVisualAvailable?.(Boolean(value.visual_options && !value.visual_error));
      })
      .catch((cause) => {
        if (loadGeneration.current !== generation) return;
        onRevealRetryChange(file, () => setReloadNonce((value) => value + 1));
        onError(messageOf(cause));
      })
      .finally(() => {
        if (loadGeneration.current === generation) setLoading(false);
      });
    return () => {
      loadGeneration.current += 1;
      diagnoseGeneration.current += 1;
      if (diagnoseTimer.current !== null) window.clearTimeout(diagnoseTimer.current);
      onRevealRetryChange(file, null);
    };
  }, [
    api,
    file,
    isAuth,
    onError,
    onRevealRetryChange,
    onVisualAvailable,
    setFromSnapshot,
    target,
    reloadNonce,
  ]);

  useEffect(() => {
    if (mode === "raw" && snapshot && textEditable) diagnose(editor);
  }, [diagnose, editor, mode, snapshot, textEditable]);

  const editorBytes = useMemo(
    () => configEditorBytes(snapshot, textEditable, editor),
    [editor, snapshot, textEditable],
  );
  const dirty = configFileDirty({
    mode,
    isAuth,
    snapshot,
    editorBytes,
    visualOptions,
    customProvider,
    authMode,
    authKey,
  });
  const canSave = configFileCanSave(snapshot, textEditable, isAuth, authMode, mode);

  const save = useCallback(async (): Promise<boolean> => {
    if (operationBusy || !snapshot || !editorBytes || !canSave) return false;
    if (
      mode === "visual" &&
      !isAuth &&
      customProvider?.included &&
      onBeforeSave &&
      !onBeforeSave(true)
    )
      return false;
    if (mode === "visual" && !isAuth && visualOptions) {
      for (const field of visualOptions) {
        if (!field.included || !field.request_proxy_route || typeof field.value !== "string")
          continue;
        const split = splitRequestProxyValue(
          field.value,
          requestProxyRoute(tenant, api.bootstrap?.listen),
        );
        if (!proxyValueIsValid(split.upstream)) {
          onError(`${field.label} must contain a valid HTTP or HTTPS upstream URL.`);
          return false;
        }
      }
    }
    if (mode === "visual" && isAuth && snapshot.auth?.extra_fields) {
      if (!window.confirm("Replace the extra native credential fields with an API-key object?"))
        return false;
    }
    if (mode === "visual" && !isAuth && customProvider?.included) {
      if (!customProvider.name.trim() || !customProvider.base_url.trim()) {
        onError("Custom provider name and base URL must not be empty.");
        return false;
      }
      const upstream = splitRequestProxyValue(
        customProvider.base_url,
        requestProxyRoute(tenant, api.bootstrap?.listen),
      ).upstream;
      if (!proxyValueIsValid(upstream)) {
        onError("Custom provider base URL must contain a valid HTTP or HTTPS URL.");
        return false;
      }
    }
    setFeedback("saving");
    try {
      const value = await api.saveConfigFile(
        target,
        configFileInput({
          snapshot,
          editorBytes,
          mode,
          isAuth,
          visualOptions,
          customProvider,
          authKey,
        }),
      );
      setFromSnapshot(value);
      setSnapshot(value);
      if (value.linked_file) onLinkedFileSaved?.(value.linked_file.file);
      setFeedback("saved");
      onError(null);
      onSaved();
      window.setTimeout(() => setFeedback("idle"), 4000);
      return true;
    } catch (cause) {
      setFeedback("idle");
      onError(messageOf(cause));
      return false;
    }
  }, [
    api,
    authKey,
    canSave,
    customProvider,
    editorBytes,
    isAuth,
    mode,
    onBeforeSave,
    onError,
    onLinkedFileSaved,
    onSaved,
    operationBusy,
    setFromSnapshot,
    snapshot,
    target,
    tenant,
    visualOptions,
  ]);

  const restore = useCallback(() => {
    if (!snapshot) return;
    setFromSnapshot(snapshot);
    onError(null);
  }, [onError, setFromSnapshot, snapshot]);

  useEffect(() => {
    onControllerChange(
      file,
      snapshot
        ? { dirty, canSave, save, restore, reload: () => setReloadNonce((value) => value + 1) }
        : null,
    );
    return () => onControllerChange(file, null);
  }, [canSave, dirty, file, onControllerChange, restore, save, snapshot]);

  const updateEditor = useCallback(
    (value: string) => {
      setEditor(value);
      diagnose(value);
    },
    [diagnose],
  );
  const { parentRef: rawEditorParent } = useCodeMirrorEditor({
    enabled: codeMirrorAvailable && mode === "raw" && Boolean(snapshot) && textEditable,
    file,
    document: editor,
    diagnostics: rawDiagnostics,
    onChange: updateEditor,
  });
  const updateVisualOption = useCallback((path: string, update: Partial<ConfigVisualOption>) => {
    setVisualOptions(
      (fields) =>
        fields?.map((field) => (field.path === path ? { ...field, ...update } : field)) ?? null,
    );
  }, []);
  const updateCustomProvider = useCallback((update: Partial<ConfigCustomProvider>) => {
    setCustomProvider((provider) => (provider ? { ...provider, ...update } : provider));
  }, []);

  return {
    authKey,
    authMode,
    canSave,
    customProvider,
    dirty,
    editor,
    feedback,
    isAuth,
    loading,
    rawDiagnostics,
    rawEditorParent,
    save,
    setAuthKey,
    setAuthMode,
    snapshot,
    textEditable,
    updateCustomProvider,
    updateEditor,
    updateVisualOption,
    useCodeMirror: codeMirrorAvailable,
    visualOptions,
  };
}
