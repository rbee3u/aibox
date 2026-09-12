import { ConfigComparisonProvider } from "@/features/configs/detail/ConfigComparisonContext";
import { AlertTriangle, ChevronLeft, Save } from "lucide-react";
import { useId } from "react";

import type { ConfigApi } from "@/api/configs";
import { ConfigDriftBadge } from "@/features/configs/ConfigDriftBadge";
import { appliedConfigPresentation } from "@/features/configs/configCatalog";
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
import { iconSize } from "@/shared/icons/iconSizes";

const ManagedTenantIcon = resourceIcons.managedTenant;
const NamedConfigIcon = resourceIcons.namedConfig;
export function ConfigDetailPane({
  api,
  catalog,
  detail,
  dialogs,
  editor,
  feedback,
  mutations,
}: Pick<ConfigViewModel, "catalog" | "detail" | "dialogs" | "editor" | "feedback" | "mutations"> & {
  api: ConfigApi;
}) {
  const filesId = useId();
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
  const { requestApply } = dialogs;
  const { setError } = feedback;
  const { mutationBusy, saveAll } = mutations;
  const inspectedName = namedConfigName(selection);
  const inspectedEntry = data?.configs.find((entry) => entry.name === inspectedName) ?? null;
  const inspectedApplied =
    inspectedName !== null && data?.application.last_application?.applied === inspectedName;
  /*
   * Standing conditions of this editor, stated once and kept stable across
   * Visual and Raw so the header never resizes under the mode toggle. Visual
   * mode masks credentials but still holds them, one reveal away.
   */
  const editorNotice = [
    tenant.kind === "host" ? "Edits write to the real Host Home" : null,
    "Native content may contain credentials and is shown without redaction.",
  ]
    .filter((part): part is string => part !== null)
    .join(" · ");
  return (
    <section className={layout.detailPane}>
      {loadingTenants || loadingCatalog ? (
        <Loading />
      ) : managedTenantMissing ? (
        <EmptyState
          variant="detail"
          icon={<ManagedTenantIcon size={iconSize.xl} aria-hidden="true" />}
          title="Managed Tenant not found"
          description="The selected Managed Tenant does not exist."
        />
      ) : isNamedCatalog(selection) && data ? (
        <EmptyState
          variant="detail"
          icon={<NamedConfigIcon size={iconSize.xl} aria-hidden="true" />}
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
              <ChevronLeft size={iconSize.md} />
            </IconButton>
            <div className={styles.configContextStack}>
              <div className={styles.configTitleRow}>
                <h2 ref={detailHeadingRef} tabIndex={-1}>
                  {configSelectionLabel}
                </h2>
                {inspectedEntry?.state === "ready" && (
                  <div className={styles.configHeaderAction}>
                    {inspectedApplied && <ConfigDriftBadge status={data.application} />}
                    {(!inspectedApplied ||
                      appliedConfigPresentation(data.application).applicable) && (
                      <ActionButton
                        tone="primarySoft"
                        disabled={mutationBusy}
                        onClick={() => requestApply(inspectedEntry.name)}
                      >
                        Apply to Current Config
                      </ActionButton>
                    )}
                  </div>
                )}
              </div>
              <p className={styles.editorNotice}>{editorNotice}</p>
              <div className={styles.contextFacts} aria-label="Config editing context">
                <span>
                  <small>Tenant</small>
                  <strong>{configTenantLabel}</strong>
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
            </div>
          </div>
          <div id={filesId} className={styles.configFilePanel}>
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
                  <Save size={iconSize.xs} /> Save all
                </ActionButton>
              )}
            </div>
            <ConfigComparisonProvider
              key={`${configTenantSelectionValue(tenant)}:${agent}:${selection.current ? "current" : namedConfigName(selection)}`}
              api={api}
              target={{
                tenant,
                agent,
                current: selection.current,
                config: selection.current ? null : namedConfigName(selection),
              }}
              enabled={Boolean(
                data.application.last_application &&
                (selection.current ||
                  namedConfigName(selection) === data.application.last_application.applied),
              )}
              files={configFiles}
              refresh={data}
            >
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
            </ConfigComparisonProvider>
          </div>
        </>
      ) : (
        <div className={styles.emptyPane} role="status">
          <AlertTriangle size={iconSize.lg} aria-hidden="true" />
          <span>Configuration is unavailable. Use Retry to load it again.</span>
        </div>
      )}
    </section>
  );
}
