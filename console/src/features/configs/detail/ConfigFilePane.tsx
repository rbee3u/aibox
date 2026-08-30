import { AlertTriangle, Check, Download, Eye, EyeOff, LoaderCircle, Save } from "lucide-react";
import { useState } from "react";

import type { ConfigApi } from "@/api/configs";
import { decodeBase64 } from "@/shared/lib/encoding";
import type { CodingAgentKind } from "@/domain/codingAgent";
import type { TenantSelection } from "@/domain/tenant";
import type { ConfigFileController } from "@/features/configs/detail/configFileController";
import { useConfigFileSession } from "@/features/configs/detail/useConfigFileSession";
import {
  VisualConfigOptions,
  VisualOptionLabel,
} from "@/features/configs/detail/VisualConfigOptions";
import type { ConfigSelection } from "@/features/configs/route";
import { ActionButton } from "@/shared/ui/ActionButton";
import { IconButton } from "@/shared/ui/IconButton";
import { Loading } from "@/shared/ui/ManagementFeedback";
import { TextArea, TextInput } from "@/shared/ui/FormControls";
import styles from "@/features/configs/ConfigPage.module.css";

interface ConfigFilePaneProps {
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
}

export function ConfigFilePane({ onRequestRaw, ...options }: ConfigFilePaneProps) {
  const [revealed, setRevealed] = useState(false);
  const {
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
    useCodeMirror,
    visualOptions,
  } = useConfigFileSession(options);
  const { api, file, mode, operationBusy, tenant } = options;

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
            onChange={(event) => updateEditor(event.target.value)}
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
