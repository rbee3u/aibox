import { AlertTriangle, Check, Download, Eye, EyeOff, LoaderCircle, Save } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  ConfigApi,
  ConfigAuthData,
  ConfigCustomProvider,
  ConfigFileData,
  ConfigVisualOption,
} from "@/api/configs";
import type { CodingAgentKind } from "@/domain/codingAgent";
import { decodeBase64, encodeBase64 } from "@/api/encoding";
import type { TenantSelection } from "@/domain/tenant";
import {
  comparableProvider,
  proxyValueIsValid,
  requestProxyRoute,
  splitRequestProxyValue,
} from "@/features/configs/configCatalog";
import type { ConfigFileController } from "@/features/configs/editor/configFileController";
import {
  VisualConfigOptions,
  VisualOptionLabel,
} from "@/features/configs/editor/VisualConfigOptions";
import {
  codeMirrorAvailable,
  useCodeMirrorEditor,
  type RawDiagnostic,
} from "@/features/configs/editor/useCodeMirrorEditor";
import type { ConfigSelection } from "@/features/configs/route";
import { ActionButton } from "@/shared/ui/ActionButton";
import { IconButton } from "@/shared/ui/IconButton";
import { Loading } from "@/shared/ui/ManagementFeedback";
import { TextArea, TextInput } from "@/shared/ui/FormControls";
import { messageOf } from "@/shared/lib/errors";
import styles from "@/features/configs/ConfigPage.module.css";

export function ConfigFilePane({
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
  onRequestRaw,
}: {
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
  onRequestRaw: () => void;
}) {
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
  const [revealed, setRevealed] = useState(false);
  const [reloadNonce, setReloadNonce] = useState(0);
  const diagnoseTimer = useRef<number | null>(null);
  const diagnoseGeneration = useRef(0);
  const loadGeneration = useRef(0);
  const useCodeMirror = codeMirrorAvailable;
  const isAuth = file === "auth.json";
  const currentSelection = selection.current;
  const diagnose = useCallback(
    (value: string) => {
      if (diagnoseTimer.current !== null) window.clearTimeout(diagnoseTimer.current);
      const generation = ++diagnoseGeneration.current;
      diagnoseTimer.current = window.setTimeout(() => {
        void api
          .diagnoseConfigFile(
            {
              tenant,
              agent,
              current: currentSelection,
              config: currentSelection ? null : selection.config,
              file,
            },
            encodeBase64(new TextEncoder().encode(value)),
          )
          .then((result) => {
            if (generation === diagnoseGeneration.current)
              setRawDiagnostics(Array.isArray(result.diagnostics) ? result.diagnostics : []);
          })
          .catch(() => {
            if (generation === diagnoseGeneration.current) setRawDiagnostics([]);
          });
      }, 250);
    },
    [agent, api, currentSelection, file, tenant, selection.config],
  );
  const setFromSnapshot = useCallback(
    (value: ConfigFileData) => {
      diagnoseGeneration.current += 1;
      if (diagnoseTimer.current !== null) window.clearTimeout(diagnoseTimer.current);
      const bytes = decodeBase64(value.content_base64);
      try {
        const content = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
        setEditor(content);
        setTextEditable(true);
        setRawDiagnostics([]);
        setVisualOptions(value.visual_options ?? null);
        setCustomProvider(() => {
          if (!value.custom_provider) return null;
          const route = requestProxyRoute(tenant, api.bootstrap?.listen);
          const split = splitRequestProxyValue(value.custom_provider.base_url, route);
          return {
            ...value.custom_provider,
            base_url: value.custom_provider.base_url,
            proxy_routed: split.routed,
          };
        });
        if (isAuth && value.auth) {
          setAuthMode(value.auth.mode);
          setAuthKey(value.auth.api_key ?? "");
        }
      } catch {
        setEditor("");
        setTextEditable(false);
        setVisualOptions(null);
        setRawDiagnostics([]);
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
      .revealConfigFile({
        tenant,
        agent,
        current: currentSelection,
        config: currentSelection ? null : selection.config,
        file,
      })
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
    agent,
    api,
    currentSelection,
    file,
    isAuth,
    onError,
    onRevealRetryChange,
    onVisualAvailable,
    tenant,
    selection.config,
    setFromSnapshot,
    reloadNonce,
  ]);
  useEffect(() => {
    if (mode === "raw" && snapshot && textEditable) diagnose(editor);
  }, [diagnose, editor, mode, snapshot, textEditable]);
  const editorBytes = useMemo(() => {
    if (!snapshot || !textEditable) return null;
    return new TextEncoder().encode(editor);
  }, [editor, snapshot, textEditable]);
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
  const dirty =
    mode === "visual"
      ? isAuth
        ? authDirty
        : visualDirty || providerDirty
      : editorBytes !== null &&
        snapshot !== null &&
        encodeBase64(editorBytes) !== snapshot.content_base64;
  const canSave = Boolean(
    snapshot && textEditable && (isAuth ? authMode === "api-key" || mode === "raw" : true),
  );
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
        const route = requestProxyRoute(tenant, api.bootstrap?.listen);
        const split = splitRequestProxyValue(field.value, route);
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
      const route = requestProxyRoute(tenant, api.bootstrap?.listen);
      const upstream = splitRequestProxyValue(customProvider.base_url, route).upstream;
      if (!proxyValueIsValid(upstream)) {
        onError("Custom provider base URL must contain a valid HTTP or HTTPS URL.");
        return false;
      }
    }
    setFeedback("saving");
    try {
      const value = await api.saveConfigFile(
        {
          tenant,
          agent,
          current: currentSelection,
          config: currentSelection ? null : selection.config,
          file,
        },
        {
          revision: snapshot.revision,
          contentBase64: encodeBase64(editorBytes),
          ...(mode === "visual" && !isAuth && visualOptions
            ? {
                visualOptions: visualOptions.map(({ path, included, value: fieldValue }) => ({
                  path,
                  included,
                  value: fieldValue,
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
          ...(mode === "visual" && isAuth
            ? { visualAuth: { included: true, value: authKey } }
            : {}),
        },
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
    agent,
    api,
    authKey,
    canSave,
    editorBytes,
    file,
    isAuth,
    mode,
    onError,
    onSaved,
    operationBusy,
    tenant,
    currentSelection,
    selection.config,
    setFromSnapshot,
    snapshot,
    visualOptions,
    customProvider,
    onBeforeSave,
    onLinkedFileSaved,
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
  const applyRawEdit = useCallback(
    (value: string) => {
      setEditor(value);
      diagnose(value);
    },
    [diagnose],
  );
  const { parentRef: rawEditorParent } = useCodeMirrorEditor({
    enabled: useCodeMirror && mode === "raw" && Boolean(snapshot) && textEditable,
    file,
    document: editor,
    diagnostics: rawDiagnostics,
    onChange: applyRawEdit,
  });
  const updateVisualOption = useCallback((path: string, update: Partial<ConfigVisualOption>) => {
    setVisualOptions((fields) => {
      if (!fields) return null;
      const next = fields.map((field) => (field.path === path ? { ...field, ...update } : field));
      return next;
    });
  }, []);
  const updateCustomProvider = useCallback((update: Partial<ConfigCustomProvider>) => {
    setCustomProvider((provider) => (provider ? { ...provider, ...update } : provider));
  }, []);
  if (loading)
    return (
      <div className={styles.configFilePane}>
        <Loading />
      </div>
    );
  if (!snapshot) return <div className={styles.configFilePane} />;
  return (
    <section className={styles.configFilePane} aria-label={`${file} editor`}>
      <div className={styles.editorTools}>
        <div className={styles.fileTitle}>
          <strong>{file}</strong>
          <span>{snapshot.exists ? "Existing file" : "New file"}</span>
        </div>
        {isAuth && mode === "visual" && <span className={styles.authModeBadge}>{authMode}</span>}
        <ActionButton
          tone="primary"
          disabled={operationBusy || !dirty || !canSave}
          onClick={() => void save()}
        >
          {feedback === "saving" ? <LoaderCircle className="spin" size={14} /> : <Save size={14} />}
          <span aria-live="polite">
            {feedback === "saving" ? "Saving…" : feedback === "saved" ? "Saved" : "Save"}
          </span>
        </ActionButton>
      </div>
      {snapshot.warnings && snapshot.warnings.length > 0 && (
        <div className={styles.fileWarnings} role="status">
          {snapshot.warnings.map((warning) => (
            <span key={warning}>
              <AlertTriangle size={14} /> {warning}
            </span>
          ))}
        </div>
      )}
      {mode === "visual" && !isAuth && visualOptions ? (
        <VisualConfigOptions
          fields={visualOptions}
          provider={customProvider ?? undefined}
          onChange={updateVisualOption}
          onProviderChange={updateCustomProvider}
          tenant={tenant}
          listen={api.bootstrap?.listen}
        />
      ) : mode === "visual" && isAuth && snapshot.auth ? (
        <div className={styles.visualEditor}>
          <section className={styles.visualGroup}>
            <header>
              <h3>Credentials</h3>
            </header>
            <div className={styles.authVisualBody}>
              {authMode === "chatgpt" ? (
                <>
                  <div className={styles.authStatus} role="status">
                    <Check size={16} /> ChatGPT credentials are active.
                  </div>
                  <p>Use Raw to inspect the native token object, or switch to an API key.</p>
                  <div className={styles.dialogActions}>
                    <button type="button" onClick={onRequestRaw}>
                      Open Raw
                    </button>
                    <ActionButton
                      tone="primary"
                      onClick={() => {
                        if (!window.confirm("Switch this draft to API-key credentials?")) return;
                        setAuthMode("api-key");
                      }}
                    >
                      Switch to API key credentials
                    </ActionButton>
                  </div>
                </>
              ) : (
                <div className={styles.visualField}>
                  <div className={styles.visualFieldMeta}>
                    <VisualOptionLabel
                      id="config-option-openai-api-key"
                      label="OpenAI API key"
                      description="API key used by Codex for OpenAI authentication."
                      required={false}
                    />
                  </div>
                  <div className={`${styles.visualFieldControl} ${styles.visualTextControl}`}>
                    <TextInput
                      id="config-option-openai-api-key"
                      type={revealed ? "text" : "password"}
                      value={authKey}
                      onChange={(event) => setAuthKey(event.target.value)}
                      aria-label="OpenAI API key"
                    />
                    <IconButton
                      label={revealed ? "Hide OpenAI API key" : "Show OpenAI API key"}
                      onClick={() => setRevealed((value) => !value)}
                    >
                      {revealed ? <EyeOff size={14} /> : <Eye size={14} />}
                    </IconButton>
                  </div>
                </div>
              )}
              {snapshot.auth.warnings.map((warning) => (
                <div className={styles.inlineWarning} key={warning}>
                  <AlertTriangle size={15} /> <span>{warning}</span>
                </div>
              ))}
              {authMode === "api-key" && snapshot.auth.extra_fields && (
                <div className={styles.inlineWarning}>
                  <AlertTriangle size={15} />
                  <span>Saving will replace extra native credential fields.</span>
                </div>
              )}
            </div>
          </section>
        </div>
      ) : textEditable ? (
        useCodeMirror ? (
          <div ref={rawEditorParent} className={styles.codeEditor} aria-label={`${file} content`} />
        ) : (
          <TextArea
            className={`${styles.codeEditor} ${styles.codeEditorFallback}`}
            aria-label={`${file} content`}
            value={editor}
            onChange={(event) => {
              setEditor(event.target.value);
              diagnose(event.target.value);
            }}
            spellCheck={false}
          />
        )
      ) : (
        <div className={styles.binaryConfigNotice} role="status">
          <AlertTriangle size={18} />
          <span>This file is not valid UTF-8 and cannot be edited in the Console.</span>
          <button
            type="button"
            onClick={() => {
              const raw = decodeBase64(snapshot.content_base64);
              const url = URL.createObjectURL(new Blob([new Uint8Array(raw).buffer]));
              const link = document.createElement("a");
              link.href = url;
              link.download = file;
              link.click();
              URL.revokeObjectURL(url);
            }}
          >
            <Download size={14} /> Download raw file
          </button>
        </div>
      )}
      {mode === "raw" && rawDiagnostics.length > 0 && (
        <div className={styles.editorDiagnostics} role="alert">
          {rawDiagnostics.map((diagnostic, index) => (
            <span key={`${diagnostic.line}-${diagnostic.column}-${index}`}>
              Line {diagnostic.line}, column {diagnostic.column}: {diagnostic.message}
            </span>
          ))}
        </div>
      )}
    </section>
  );
}
