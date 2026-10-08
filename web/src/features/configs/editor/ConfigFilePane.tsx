import styles from "@/features/configs/editor/ConfigFilePane.module.css";
import sharedStyles from "@/features/configs/shared.module.css";
import editorStyles from "@/features/configs/editor/editor.module.css";
import type {
  ConfigEditorSession,
  ConfigFileView,
} from "@/features/configs/editor/configEditorSession";
import {
  codeMirrorAvailable,
  useCodeMirrorEditor,
} from "@/features/configs/editor/useCodeMirrorEditor";
import {
  openDifference,
  FileDifferences,
  FieldDifferences,
  FileDifferenceCount,
} from "@/features/configs/editor/ConfigDifferences";
import { useConfigComparison } from "@/features/configs/editor/ConfigComparisonContext";
import { differenceRange } from "@/features/configs/editor/configDifferenceRanges";
import type { ConfigDifference, ConfigVisualOption, ConfigCustomProvider } from "@/api/configs";
import { AlertTriangle, Check, Download, Eye, EyeOff, LoaderCircle, Save } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { decodeBase64 } from "@/shared/lib/encoding";
import type { TenantSelection } from "@/domain/tenant";

import {
  VisualConfigOptions,
  VisualOptionLabel,
} from "@/features/configs/editor/VisualConfigOptions";

import { ActionButton } from "@/shared/ui/ActionButton";
import { IconButton } from "@/shared/ui/IconButton";
import { Loading } from "@/shared/ui/ManagementFeedback";
import { TextArea, TextInput } from "@/shared/ui/FormControls";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";

import { iconSize } from "@/shared/icons/iconSizes";

interface ConfigFilePaneProps {
  listen?: string;
  tenant: TenantSelection;
  file: string;
  mode: "visual" | "raw";
  controlsDisabled: boolean;
  session: ConfigEditorSession;
  data: ConfigFileView;
  onRequestRaw: () => void;
}

export function ConfigFilePane({
  onRequestRaw,
  controlsDisabled,
  ...options
}: ConfigFilePaneProps) {
  const { result: comparisonResult, current: comparingCurrent } = useConfigComparison();
  const [locate, setLocate] = useState<ConfigDifference | null>(null);
  const [revealed, setRevealed] = useState(false);
  const { listen, file, mode, tenant, session } = options;
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
    saveFailure,
    snapshot,
    textEditable,
    visualOptions,
  } = options.data;
  const updateEditor = useCallback(
    (value: string) => session.updateEditor(file, value),
    [session, file],
  );
  const setAuthKey = (value: string) => session.setAuthKey(file, value);
  const setAuthMode = (value: "api-key" | "chatgpt") => session.setAuthMode(file, value);
  const save = () => session.save(file);
  const updateVisualOption = (path: string, update: Partial<ConfigVisualOption>) =>
    session.updateVisualOption(file, path, update);
  const updateCustomProvider = (update: Partial<ConfigCustomProvider>) =>
    session.updateCustomProvider(file, update);
  const comparedFile = comparisonResult?.files.find((entry) => entry.file === file);
  const ranges = useMemo(() => {
    const side = comparingCurrent ? comparedFile?.current : comparedFile?.named;
    if (side?.content !== editor) return [];
    return (comparedFile?.differences ?? []).flatMap((difference) => {
      const range = differenceRange(file, editor, difference, comparingCurrent);
      return range ? [{ range, path: difference.path }] : [];
    });
  }, [comparedFile, comparingCurrent, editor, file]);
  const onDifference = useCallback((path: string[]) => openDifference(file, path), [file]);
  const { parentRef: rawEditorParent, revealRange } = useCodeMirrorEditor({
    enabled: codeMirrorAvailable && mode === "raw" && Boolean(snapshot) && textEditable,
    file,
    document: editor,
    diagnostics: rawDiagnostics,
    differences: ranges,
    onDifference,
    onChange: updateEditor,
  });
  const useCodeMirror = codeMirrorAvailable;

  useEffect(() => {
    if (!locate || mode !== "raw") return;
    const fileResult = comparisonResult?.files.find((entry) => entry.file === file);
    const side = comparingCurrent ? fileResult?.current : fileResult?.named;
    if (side?.content !== editor) return;
    const latest = fileResult?.differences.find(
      (difference) => JSON.stringify(difference.path) === JSON.stringify(locate.path),
    );
    if (!latest) return;
    const range = differenceRange(file, editor, latest, comparingCurrent);
    if (range) revealRange(range);
  }, [locate, mode, file, editor, comparisonResult, comparingCurrent, revealRange]);

  if (loading)
    return (
      <div className={styles.configFilePane}>
        <Loading />
      </div>
    );
  if (!snapshot) return <div className={styles.configFilePane} />;

  return (
    <section className={styles.configFilePane} aria-label={`${file} editor`}>
      <div className={sharedStyles.editorTools}>
        <div className={styles.fileTitle}>
          <div className={styles.fileHeading}>
            <strong>{file}</strong>
            <FileDifferenceCount file={file} />
          </div>
          {/* The second line speaks only when there is something to say. */}
          {dirty ? (
            <span className={styles.fileStateDirty}>Unsaved changes</span>
          ) : !snapshot.exists ? (
            <span>New file</span>
          ) : null}
        </div>
        {isAuth && mode === "visual" && <span className={styles.authModeBadge}>{authMode}</span>}
        <ActionButton
          tone="primarySoft"
          disabled={controlsDisabled || !dirty || !canSave}
          onClick={() => void save()}
        >
          {feedback === "saving" ? (
            <LoaderCircle className="spin" size={iconSize.xs} />
          ) : (
            <Save size={iconSize.xs} />
          )}
          <span aria-live="polite">
            {feedback === "saving" ? "Saving…" : feedback === "saved" ? "Saved" : "Save"}
          </span>
        </ActionButton>
      </div>
      <FileDifferences
        file={file}
        onLocate={(difference) => {
          setLocate(difference);
          onRequestRaw();
        }}
      />
      {saveFailure && (
        <AlertBanner
          variant="strip"
          tone="danger"
          icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
        >
          {saveFailure.message}
        </AlertBanner>
      )}
      {snapshot.warnings && snapshot.warnings.length > 0 && (
        <AlertBanner
          variant="strip"
          tone="warning"
          icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
        >
          {snapshot.warnings.join(" ")}
        </AlertBanner>
      )}
      {mode === "visual" && !isAuth && visualOptions ? (
        <VisualConfigOptions
          file={file}
          fields={visualOptions}
          provider={customProvider ?? undefined}
          invalidPaths={saveFailure?.paths}
          onChange={updateVisualOption}
          onProviderChange={updateCustomProvider}
          tenant={tenant}
          listen={listen}
        />
      ) : mode === "visual" && isAuth && snapshot.auth ? (
        <div className={editorStyles.visualEditor}>
          <div className={sharedStyles.visualFieldList}>
            <div className={sharedStyles.authVisualBody}>
              <FieldDifferences file={file} path="" sensitive />
              {authMode === "chatgpt" ? (
                <>
                  <div className={styles.authStatus} role="status">
                    <Check size={iconSize.sm} /> ChatGPT credentials are active.
                  </div>
                  <p>Use Raw to inspect the native token object, or switch to an API key.</p>
                  <div className={sharedStyles.dialogActions}>
                    <ActionButton type="button" tone="ghost" onClick={onRequestRaw}>
                      Open Raw
                    </ActionButton>
                    <ActionButton
                      tone="secondary"
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
                <div className={sharedStyles.visualField}>
                  <div className={editorStyles.visualFieldMeta}>
                    <VisualOptionLabel label="OpenAI API key" path="OPENAI_API_KEY" />
                  </div>
                  <div className={editorStyles.visualFieldControl}>
                    <div className={editorStyles.visualTextControl}>
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
                        {revealed ? <EyeOff size={iconSize.xs} /> : <Eye size={iconSize.xs} />}
                      </IconButton>
                    </div>
                  </div>
                </div>
              )}
              {snapshot.auth.warnings.map((warning) => (
                <AlertBanner
                  className={sharedStyles.inlineWarning}
                  key={warning}
                  tone="warning"
                  icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
                >
                  {warning}
                </AlertBanner>
              ))}
              {authMode === "api-key" && snapshot.auth.extra_fields && (
                <AlertBanner
                  className={sharedStyles.inlineWarning}
                  tone="warning"
                  icon={<AlertTriangle size={iconSize.xs} aria-hidden="true" />}
                >
                  Saving will replace extra native credential fields.
                </AlertBanner>
              )}
            </div>
          </div>
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
          <AlertTriangle size={iconSize.md} />
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
            <Download size={iconSize.xs} /> Download raw file
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
