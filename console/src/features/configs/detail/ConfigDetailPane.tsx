import { AlertTriangle, ChevronLeft, Save } from "lucide-react";

import type { ConfigApi } from "@/api/configs";
import { ConfigFilePane } from "@/features/configs/detail/ConfigFilePane";
import {
  configTenantSelectionValue,
  isNamedCatalog,
  namedConfigName,
} from "@/features/configs/route";
import type { ConfigViewModel } from "@/features/configs/useConfigController";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { ActionButton } from "@/shared/ui/ActionButton";
import { EmptyState } from "@/shared/ui/EmptyState";
import { IconButton } from "@/shared/ui/IconButton";
import { Loading } from "@/shared/ui/ManagementFeedback";
import { SegmentedControl } from "@/shared/ui/SegmentedControl";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/configs/ConfigPage.module.css";

const ManagedTenantIcon = resourceIcons.managedTenant;
const NamedConfigIcon = resourceIcons.namedConfig;
export function ConfigDetailPane({
  api,
  catalog,
  detail,
  editor,
  feedback,
  mutations,
}: Pick<ConfigViewModel, "catalog" | "detail" | "editor" | "feedback" | "mutations"> & {
  api: ConfigApi;
}) {
  const {
    agent,
    catalog: data,
    configFiles,
    configSelectionLabel,
    configTenantLabel,
    loadingCatalog,
    loadingTenants,
    managedTenantMissing,
    tenant,
  } = catalog;
  const { closeConfigDetail, detailBackButtonRef, detailHeadingRef, file, selection } = detail;
  const {
    dirtyFiles,
    editorMode,
    handleLinkedFileSaved,
    handlePaneSaved,
    handleVisualAvailable,
    prepareMainConfigSave,
    registerFileController,
    registerPane,
    registerRevealRetry,
    showRawEditor,
    switchEditorMode,
    visualAvailable,
  } = editor;
  const { setError } = feedback;
  const { mutationBusy, saveAll } = mutations;
  return (
    <section className={layout.detailPane}>
      {loadingTenants || loadingCatalog ? (
        <Loading />
      ) : managedTenantMissing ? (
        <EmptyState
          variant="detail"
          icon={<ManagedTenantIcon size={26} aria-hidden="true" />}
          title="Managed Tenant not found"
          description="The selected Managed Tenant does not exist."
        />
      ) : isNamedCatalog(selection) && data ? (
        <EmptyState
          variant="detail"
          icon={<NamedConfigIcon size={26} aria-hidden="true" />}
          title="Named Configs"
          description="Select Current Config or a Named Config to inspect its files."
        />
      ) : data ? (
        <>
          <div className={styles.configEditorHeader}>
            <IconButton
              buttonRef={detailBackButtonRef}
              label="Back to Configs"
              onClick={closeConfigDetail}
            >
              <ChevronLeft size={17} />
            </IconButton>
            <div className={styles.configContextStack}>
              <div className={styles.contextFacts} aria-label="Config editing context">
                <span>
                  <small>Tenant</small>
                  <strong>
                    {configTenantLabel}
                    {tenant.kind === "host" && <em>Host risk</em>}
                  </strong>
                </span>
                <span>
                  <small>Coding Agent</small>
                  <strong>{agent === "codex" ? "Codex" : "Claude"}</strong>
                </span>
                <span>
                  <small>Config</small>
                  <strong>{configSelectionLabel}</strong>
                </span>
                <span>
                  <small>File</small>
                  <strong
                    className={styles.contextFile}
                    title={agent === "codex" ? "config.toml + auth.json" : "settings.json"}
                  >
                    {agent === "codex" ? "config.toml + auth.json" : "settings.json"}
                  </strong>
                </span>
              </div>
              {(selection.current || agent === "codex" || editorMode === "raw") && (
                <span className={styles.sensitiveContext}>
                  Native content may contain credentials and is displayed without redaction.
                </span>
              )}
              <h2 ref={detailHeadingRef} tabIndex={-1}>
                {agent === "codex" && !selection.current
                  ? "Codex configuration"
                  : (file ?? "Configuration")}
              </h2>
            </div>
          </div>
          <div className={styles.configFilePanel}>
            <div className={styles.editorModeBar} aria-label="Editor mode">
              <span>
                {dirtyFiles.length > 0
                  ? `${dirtyFiles.length} unsaved file${dirtyFiles.length === 1 ? "" : "s"}`
                  : "All files saved"}
              </span>
              <SegmentedControl variant="filled" role="group" aria-label="Editor mode">
                {visualAvailable && !selection.current && (
                  <button
                    type="button"
                    aria-pressed={editorMode === "visual"}
                    onClick={() => switchEditorMode("visual")}
                  >
                    Visual
                  </button>
                )}
                <button
                  type="button"
                  aria-pressed={editorMode === "raw"}
                  onClick={() => switchEditorMode("raw")}
                >
                  Raw
                </button>
              </SegmentedControl>
              {dirtyFiles.length > 0 && (
                <ActionButton
                  tone="primarySoft"
                  disabled={mutationBusy}
                  onClick={() => void saveAll()}
                >
                  <Save size={14} /> Save all
                </ActionButton>
              )}
            </div>
            <div className={styles.configFileStack}>
              {configFiles.map((name) => (
                <div
                  key={name}
                  ref={(element) => registerPane(name, element)}
                  className={`${styles.configFileSection} ${file === name ? styles.configFileSectionFocused : ""}`}
                >
                  <ConfigFilePane
                    key={`${configTenantSelectionValue(tenant)}:${agent}:${selection.current ? "current" : `named:${namedConfigName(selection)}`}:${name}`}
                    api={api}
                    tenant={tenant}
                    agent={agent}
                    selection={selection}
                    file={name}
                    mode={selection.current ? "raw" : editorMode}
                    controlsDisabled={mutationBusy}
                    onControllerChange={registerFileController}
                    onError={setError}
                    onRevealRetryChange={registerRevealRetry}
                    onSaved={handlePaneSaved}
                    onBeforeSave={name === "config.toml" ? prepareMainConfigSave : undefined}
                    onLinkedFileSaved={handleLinkedFileSaved}
                    onVisualAvailable={
                      name === (agent === "claude" ? "settings.json" : "config.toml")
                        ? handleVisualAvailable
                        : undefined
                    }
                    onRequestRaw={showRawEditor}
                  />
                </div>
              ))}
            </div>
          </div>
        </>
      ) : (
        <div className={styles.emptyPane} role="status">
          <AlertTriangle size={22} aria-hidden="true" />
          <span>Configuration is unavailable. Use Retry to load it again.</span>
        </div>
      )}
    </section>
  );
}
