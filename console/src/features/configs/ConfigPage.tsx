import { flushSync } from "react-dom";
import {
  AlertTriangle,
  Check,
  ChevronLeft,
  ListChecks,
  LoaderCircle,
  Plus,
  Save,
  Trash2,
} from "lucide-react";

import type { ConfigApi } from "@/api/configs";
import type { Operation } from "@/api/operations";
import { ConfigDriftBadge } from "@/features/configs/components/ConfigDriftBadge";
import {
  configIssueDescriptionId,
  configIssuePresentation,
  configWarningPresentation,
  propagationDetail,
  propagationGroup,
} from "@/features/configs/configCatalog";
import { ConfigFilePane } from "@/features/configs/editor/ConfigFilePane";
import { configLocation, configTenantKey } from "@/features/configs/route";
import { useConfigController } from "@/features/configs/useConfigController";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { ActionButton } from "@/shared/ui/ActionButton";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { Dialog } from "@/shared/ui/Dialog";
import { EmptyState } from "@/shared/ui/EmptyState";
import { TextInput } from "@/shared/ui/FormControls";
import { IconButton } from "@/shared/ui/IconButton";
import { IssueIndicator } from "@/shared/ui/IssueIndicator";
import { Loading, MutationUnavailable, PageError } from "@/shared/ui/ManagementFeedback";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { SelectionMenu } from "@/shared/ui/SelectionMenu";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/configs/ConfigPage.module.css";

const CurrentConfigIcon = resourceIcons.currentConfig;
const ManagedTenantIcon = resourceIcons.managedTenant;
const NamedConfigIcon = resourceIcons.namedConfig;

interface PageProps {
  api: ConfigApi;
  operation?: Operation | null;
  search: string;
  onDirtyChange?: (dirty: boolean) => void;
  onLocationChange: ModuleLocationChange;
  onOperation?: (operation: Operation) => void;
}

export function ConfigPage(props: PageProps) {
  const {
    agent,
    agentOptions,
    allSelectable,
    appliedName,
    applyConfig,
    applyFeedback,
    applyTarget,
    busy,
    cancelPending,
    cancelSelection,
    catalog,
    catalogError,
    configFiles,
    configRowButtons,
    configSelectionLabel,
    configTenantLabel,
    createConfig,
    createError,
    createHelpId,
    createNameValid,
    createOpen,
    createTitleId,
    deleteConfigs,
    deleteTarget,
    detailBackButtonRef,
    detailHeadingRef,
    detailOpen,
    dirtyFiles,
    discardAndRunPendingAction,
    editorMode,
    error,
    executePropagation,
    file,
    fileStatuses,
    handleLinkedFileSaved,
    handlePaneSaved,
    handleVisualAvailable,
    loadCatalog,
    loadingCatalog,
    loadingTenants,
    managedTenantMissing,
    mutationBusy,
    newName,
    openConfig,
    openCurrent,
    paneRefs,
    pendingAction,
    prepareMainConfigSave,
    preview,
    previewPropagation,
    propagationHasFailures,
    propagationNeedsAttention,
    propagationTitleId,
    refreshing,
    registerFileController,
    registerRevealRetry,
    report,
    requestDelete,
    requestEditorAction,
    retryReveals,
    retryTenants,
    saveInOrder,
    saveOrder,
    savePending,
    selectAgent,
    selectableNames,
    selectedCount,
    selectedNames,
    selection,
    selectionMode,
    selectTenant,
    setApplyTarget,
    setBusy,
    setCreateError,
    setCreateOpen,
    setDeleteTarget,
    setDetailOpen,
    setEditorMode,
    setError,
    setNewName,
    setPreview,
    setReport,
    setSelectionMode,
    switchEditorMode,
    tenant,
    tenantError,
    tenantOptions,
    toggleAllConfigs,
    toggleConfig,
    unsavedTitleId,
    visualAvailable,
  } = useConfigController(props);
  const { api, onLocationChange, operation } = props;
  return (
    <div className={`${layout.page} ${layout.catalogPage}`}>
      <PageError
        error={tenantError ?? catalogError ?? error}
        onRetry={
          tenantError
            ? retryTenants
            : catalogError || error
              ? () => {
                  setError(null);
                  retryReveals();
                  void loadCatalog("refresh");
                }
              : undefined
        }
      />
      <MutationUnavailable operation={operation} />
      <div className={`${layout.splitLayout} ${detailOpen ? layout.showsDetail : ""}`}>
        <aside className={styles.configCatalog} aria-label="Configs">
          <div className={`${layout.toolbar} ${selectionMode ? layout.selectionBar : ""}`}>
            {selectionMode ? (
              <>
                <button
                  type="button"
                  className={layout.selectionCancel}
                  disabled={busy}
                  onClick={cancelSelection}
                >
                  Cancel
                </button>
                <div className={layout.selectionCenter}>
                  <span className={layout.selectionCount}>{selectedCount} selected</span>
                  <button
                    type="button"
                    className={layout.selectionAll}
                    disabled={selectableNames.length === 0 || busy}
                    onClick={toggleAllConfigs}
                  >
                    {allSelectable ? "Clear all" : "Select all"}
                  </button>
                </div>
                <button
                  type="button"
                  className={layout.selectionDelete}
                  aria-label="Delete selected Named Configs"
                  disabled={selectedCount === 0 || mutationBusy}
                  onClick={() => requestDelete([...selectedNames])}
                >
                  <Trash2 size={14} aria-hidden="true" /> Delete
                </button>
              </>
            ) : (
              <>
                <div className={layout.toolbarFilters}>
                  <SelectionMenu
                    className={layout.filterControl}
                    disabled={busy || loadingCatalog || refreshing}
                    label="Tenant"
                    onCommit={selectTenant}
                    options={tenantOptions}
                    pluralLabel="tenants"
                    selected={new Set([configTenantKey(tenant)])}
                    triggerIcon={<ManagedTenantIcon size={14} aria-hidden="true" />}
                    unavailableSummary={
                      loadingTenants
                        ? "Loading"
                        : managedTenantMissing
                          ? "Not found"
                          : "Unavailable"
                    }
                    allowMultiple={false}
                  />
                  <SelectionMenu
                    className={layout.filterControl}
                    disabled={busy || loadingCatalog || refreshing}
                    label="Coding Agent"
                    onCommit={selectAgent}
                    options={agentOptions}
                    pluralLabel="Coding Agents"
                    selected={new Set([agent])}
                    triggerIcon={<BrandIcon brand={brandForAgent(agent)} size={14} />}
                    allowMultiple={false}
                  />
                </div>
                <div className={layout.toolbarActions}>
                  <RefreshButton
                    className={layout.refreshAction}
                    label="Refresh Configs"
                    busyLabel="Refreshing Configs"
                    busy={refreshing}
                    disabled={loadingCatalog || refreshing || busy}
                    onClick={() =>
                      requestEditorAction(async () => {
                        await loadCatalog("refresh");
                      })
                    }
                  >
                    Refresh
                  </RefreshButton>
                  <button
                    type="button"
                    className={layout.selectionEnter}
                    aria-label="Select Configs"
                    disabled={selectableNames.length === 0 || loadingCatalog || refreshing || busy}
                    onClick={() => setSelectionMode(true)}
                  >
                    <ListChecks size={14} /> Select
                  </button>
                </div>
              </>
            )}
          </div>
          <div className={styles.configWarnings} aria-live="polite">
            {appliedName && !applyFeedback && (
              <div className={styles.applicationNotice}>
                <Check size={15} aria-hidden="true" />
                <span>
                  Last applied: <strong>Named Config {appliedName}</strong>. Application is a
                  one-time projection to Current Config, not an Active Config.
                </span>
              </div>
            )}
            {applyFeedback && (
              <div className={styles.applicationNotice} role="status">
                <Check size={15} aria-hidden="true" />
                <span>{applyFeedback}</span>
              </div>
            )}
            {catalog?.application.drift === "source-missing" && (
              <div className={styles.inlineWarning}>
                <AlertTriangle size={15} />
                <span title={catalog.application.detail}>
                  Last applied Named Config is missing.
                </span>
              </div>
            )}
            {catalog?.application.drift === "comparison-error" && catalog.application.detail && (
              <div className={styles.inlineWarning}>
                <AlertTriangle size={15} />
                <span>{catalog.application.detail}</span>
              </div>
            )}
          </div>
          <div className={layout.list} aria-busy={loadingCatalog}>
            {(loadingTenants || loadingCatalog) && !catalog && <Loading />}
            <div className={layout.rowGroup}>
              {!managedTenantMissing && (
                <div
                  className={`${layout.row} ${selection.current ? layout.rowInspected : ""} ${selectionMode ? `${layout.rowSelectable} ${layout.rowProtected}` : ""}`}
                >
                  <button
                    ref={(element) => {
                      if (element) configRowButtons.current.set("current", element);
                      else configRowButtons.current.delete("current");
                    }}
                    type="button"
                    className={styles.configRowMain}
                    aria-label={
                      selectionMode ? "Current Config cannot be selected" : "Current Config"
                    }
                    aria-pressed={!selectionMode && selection.current ? true : undefined}
                    disabled={busy || loadingCatalog || (selectionMode ? true : false)}
                    onClick={() => void openCurrent()}
                  >
                    <CurrentConfigIcon size={16} data-icon="current-config" />
                    <span className={styles.configRowText}>
                      <strong>Current Config</strong>
                    </span>
                    {selectionMode && <span className={layout.protectedBadge}>Protected</span>}
                  </button>
                  {!selectionMode &&
                    tenant.kind === "host" &&
                    agent === "codex" &&
                    catalog?.credential_propagation_available && (
                      <button
                        type="button"
                        className={`${styles.configRowPrimaryAction} ${styles.configPropagateAction}`}
                        title="Propagate credentials"
                        aria-label="Propagate credentials"
                        disabled={mutationBusy}
                        onClick={() => void previewPropagation()}
                      >
                        Propagate credentials
                      </button>
                    )}
                </div>
              )}
              <div className={layout.divider}>
                <span>Named Configs</span>
                <IconButton
                  className={layout.addAction}
                  label="Create Named Config"
                  disabled={mutationBusy || loadingCatalog || selectionMode}
                  onClick={() =>
                    requestEditorAction(() => {
                      setCreateError(null);
                      setCreateOpen(true);
                    })
                  }
                >
                  <Plus size={15} />
                </IconButton>
              </div>
              {catalog?.configs.map((entry) => {
                const applied = entry.name === appliedName;
                const selectedForDeletion = selectedNames.has(entry.name);
                const selectedForInspection = !selection.current && selection.config === entry.name;
                const issue = configIssuePresentation(entry) ?? configWarningPresentation(entry);
                const issueDescriptionId = issue
                  ? configIssueDescriptionId(tenant, agent, entry.name)
                  : undefined;
                return (
                  <div
                    key={entry.name}
                    className={`${layout.row} ${selectedForInspection ? layout.rowInspected : ""} ${selectedForDeletion ? layout.rowSelected : ""} ${selectionMode ? layout.rowSelectable : ""}`}
                  >
                    <button
                      ref={(element) => {
                        if (element) configRowButtons.current.set(entry.name, element);
                        else configRowButtons.current.delete(entry.name);
                      }}
                      type="button"
                      className={styles.configRowMain}
                      aria-label={
                        selectionMode
                          ? `${selectedForDeletion ? "Deselect" : "Select"} ${entry.name}`
                          : entry.name
                      }
                      aria-describedby={issueDescriptionId}
                      aria-pressed={selectionMode ? selectedForDeletion : selectedForInspection}
                      disabled={busy || loadingCatalog}
                      onClick={() =>
                        selectionMode ? toggleConfig(entry.name) : void openConfig(entry.name)
                      }
                    >
                      <NamedConfigIcon size={16} />
                      <span className={styles.configRowText}>
                        <span className={styles.configRowTitle}>
                          <strong>{entry.name}</strong>
                          {issue && (
                            <IssueIndicator
                              tone={issue.tone}
                              label={issue.label}
                              message={issue.message}
                              ariaLabel={issue.accessibleLabel}
                            />
                          )}
                          {applied && <ConfigDriftBadge status={catalog.application} />}
                        </span>
                      </span>
                      {selectionMode && (
                        <span className={layout.selectionIndicator} aria-hidden="true">
                          {selectedForDeletion && <Check size={15} strokeWidth={3} />}
                        </span>
                      )}
                      {issue && (
                        <span id={issueDescriptionId} className="srOnly">
                          {issue.accessibleLabel}
                        </span>
                      )}
                    </button>
                    {!selectionMode && (
                      <div className={layout.rowActions}>
                        {entry.state === "ready" && (
                          <button
                            type="button"
                            className={styles.configRowPrimaryAction}
                            title={
                              applied && catalog.application.drift === "clean"
                                ? "Already clean"
                                : `Apply Named Config ${entry.name} to Current Config`
                            }
                            aria-label={`Apply Named Config ${entry.name} to Current Config`}
                            disabled={
                              mutationBusy || (applied && catalog.application.drift === "clean")
                            }
                            onClick={() =>
                              requestEditorAction(() => setApplyTarget({ name: entry.name }))
                            }
                          >
                            Apply to Current Config
                          </button>
                        )}
                        {entry.state === "incomplete" && (
                          <button
                            type="button"
                            className={styles.configRowPrimaryAction}
                            title={`Repair Named Config ${entry.name}`}
                            aria-label={`Repair Named Config ${entry.name}`}
                            disabled={mutationBusy}
                            onClick={() => requestEditorAction(() => createConfig(entry.name))}
                          >
                            Repair
                          </button>
                        )}
                        <IconButton
                          className={`${layout.rowAction} ${layout.rowDeleteAction}`}
                          label={`Delete Named Config ${entry.name}`}
                          disabled={mutationBusy}
                          onClick={() => requestDelete([entry.name])}
                        >
                          <Trash2 size={15} />
                        </IconButton>
                      </div>
                    )}
                  </div>
                );
              })}
              {catalog && catalog.configs.length === 0 && !loadingCatalog && (
                <EmptyState
                  variant="list"
                  icon={<NamedConfigIcon size={22} aria-hidden="true" />}
                  title="No Named Configs found."
                />
              )}
            </div>
          </div>
        </aside>
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
          ) : catalog ? (
            <>
              <div className={styles.configEditorHeader}>
                <IconButton
                  buttonRef={detailBackButtonRef}
                  label="Back to Configs"
                  onClick={() =>
                    requestEditorAction(() => {
                      const focusKey = selection.current ? "current" : selection.config;
                      flushSync(() => setDetailOpen(false));
                      if (focusKey) {
                        const target = configRowButtons.current.get(focusKey);
                        target?.focus();
                      }
                      onLocationChange(configLocation(tenant, agent, null));
                    })
                  }
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
                  <div className={styles.segmented}>
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
                  </div>
                  {dirtyFiles.length > 0 && (
                    <ActionButton
                      tone="primary"
                      disabled={mutationBusy}
                      onClick={() => {
                        void (async () => {
                          setBusy(true);
                          await saveInOrder(saveOrder);
                          await loadCatalog("background");
                          setBusy(false);
                        })();
                      }}
                    >
                      <Save size={14} /> Save all
                    </ActionButton>
                  )}
                </div>
                <div className={styles.configFileStack}>
                  {configFiles.map((name) => (
                    <div
                      key={name}
                      ref={(element) => {
                        if (element) paneRefs.current.set(name, element);
                        else paneRefs.current.delete(name);
                      }}
                      className={`${styles.configFileSection} ${file === name ? styles.configFileSectionFocused : ""}`}
                    >
                      <ConfigFilePane
                        key={`${configTenantKey(tenant)}:${agent}:${selection.current ? "current" : `named:${selection.config}`}:${name}`}
                        api={api}
                        tenant={tenant}
                        agent={agent}
                        selection={selection}
                        file={name}
                        mode={selection.current ? "raw" : editorMode}
                        operationBusy={mutationBusy}
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
                        onRequestRaw={() => setEditorMode("raw")}
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
      </div>
      {pendingAction && (
        <Dialog
          className={layout.dialog}
          ariaLabelledBy={unsavedTitleId}
          busy={mutationBusy}
          onCancel={cancelPending}
        >
          <section>
            <h2 id={unsavedTitleId}>Unsaved changes</h2>
            <p>
              Save changes to{" "}
              {dirtyFiles.length > 1
                ? `${dirtyFiles.length} files`
                : (dirtyFiles[0] ?? "this file")}{" "}
              before continuing?
            </p>
            <div className={styles.dialogActions}>
              <button type="button" onClick={cancelPending} disabled={busy}>
                Cancel
              </button>
              <button
                type="button"
                onClick={() => void discardAndRunPendingAction()}
                disabled={busy}
              >
                Discard and continue
              </button>
              <ActionButton
                tone="primary"
                onClick={() => void savePending(saveOrder)}
                disabled={mutationBusy || dirtyFiles.some((name) => !fileStatuses[name]?.canSave)}
              >
                Save and continue
              </ActionButton>
            </div>
          </section>
        </Dialog>
      )}
      {createOpen && (
        <Dialog
          className={layout.dialog}
          ariaLabelledBy={createTitleId}
          busy={mutationBusy}
          onCancel={() => setCreateOpen(false)}
        >
          <form
            onSubmit={(event) => {
              event.preventDefault();
              if (createNameValid && !mutationBusy) void createConfig(newName);
            }}
          >
            <h2 id={createTitleId}>Create Named Config</h2>
            <label>
              Name
              <TextInput
                autoFocus
                aria-label="Named Config name"
                value={newName}
                onChange={(event) => {
                  setNewName(event.target.value);
                  setCreateError(null);
                }}
                aria-invalid={newName.length > 0 && !createNameValid}
                aria-describedby={createHelpId}
              />
            </label>
            <p id={createHelpId} className={layout.dialogDescription}>
              Use 1–63 lowercase letters, numbers, or hyphens; start and end with a letter or
              number.
            </p>
            {newName.length > 0 && !createNameValid && (
              <div className={styles.inlineWarning} role="alert">
                Enter a valid lowercase DNS label.
              </div>
            )}
            {createError && <div className={styles.inlineWarning}>{createError}</div>}
            <div className={styles.dialogActions}>
              <button type="button" onClick={() => setCreateOpen(false)} disabled={busy}>
                Cancel
              </button>
              <ActionButton
                type="submit"
                tone="primary"
                disabled={!createNameValid || mutationBusy}
              >
                {busy ? (
                  <LoaderCircle className="spin" size={14} aria-hidden="true" />
                ) : (
                  <Plus size={14} />
                )}
                {busy ? "Creating…" : "Create"}
              </ActionButton>
            </div>
          </form>
        </Dialog>
      )}
      {applyTarget && (
        <ConfirmDialog
          title={`Apply Named Config ${applyTarget.name} to Current Config?`}
          description={
            <div className={layout.dialogDescription}>
              <p>
                Tenant: <strong>{configTenantLabel}</strong>
                <br />
                Coding Agent: <strong>{agent === "codex" ? "Codex" : "Claude"}</strong>
                <br />
                Source: <strong>Named Config {applyTarget.name}</strong>
                <br />
                Target: <strong>Current Config</strong>
              </p>
              <p>
                Included fixed Config Fields may be added or replaced; omitted fixed fields are
                removed. Unrelated native configuration is preserved. This is a one-time projection
                to Current Config and does not create an Active Config. Files commit one at a time;
                a later file failure does not roll back earlier updates.
              </p>
            </div>
          }
          confirmation={tenant.kind === "host" ? "Host Tenant" : undefined}
          confirmLabel="Apply to Current Config"
          variant="primary"
          busy={mutationBusy}
          onCancel={() => setApplyTarget(null)}
          onConfirm={() => void applyConfig(applyTarget.name)}
        />
      )}
      {deleteTarget?.names.length === 1 && (
        <ConfirmDialog
          title={`Delete Named Config ${deleteTarget.names[0]}?`}
          description={
            <p className={layout.dialogDescription}>
              This deletes only the Named Config. Current Config stays unchanged; if this was the
              last applied source, Config Drift will report it as missing.
            </p>
          }
          confirmLabel="Delete Config"
          busy={mutationBusy}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void deleteConfigs()}
        />
      )}
      {deleteTarget && deleteTarget.names.length > 1 && (
        <ConfirmDialog
          title="Delete selected Named Configs?"
          description={
            <>
              <p className={layout.dialogDescription}>
                This deletes only the selected Named Configs. Current Config files are not changed.
                If a last applied source is deleted, Config Drift becomes Source missing.
              </p>
              <div className={styles.planList}>
                {deleteTarget.names.map((name) => (
                  <code key={name}>{name}</code>
                ))}
              </div>
            </>
          }
          confirmLabel="Delete selected"
          busy={mutationBusy}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={() => void deleteConfigs()}
        />
      )}
      {(preview || report) && (
        <Dialog
          className={`${layout.dialog} ${styles.wideDialog}`}
          ariaLabelledBy={propagationTitleId}
          busy={mutationBusy}
          onCancel={() => {
            setPreview(null);
            setReport(null);
          }}
        >
          <section>
            <h2 id={propagationTitleId}>
              {preview ? "Credential Propagation preview" : "Credential Propagation result"}
            </h2>
            {report && (
              <div
                className={`${styles.propagationSummary} ${
                  propagationHasFailures || propagationNeedsAttention
                    ? styles.propagationSummaryPartial
                    : styles.propagationSummaryComplete
                }`}
                role={propagationHasFailures ? "alert" : "status"}
              >
                {propagationHasFailures
                  ? "Partially completed. Successful credential updates were kept; failed targets need attention."
                  : propagationNeedsAttention
                    ? "Credential propagation completed with targets that need attention."
                    : "Credential propagation completed."}
              </div>
            )}
            <div className={styles.propagationGroups}>
              {(["updated", "skipped", "attention"] as const).map((group) => {
                const entries = (preview?.preview.entries ?? report?.entries ?? []).filter(
                  (entry) => propagationGroup(entry.outcome.status) === group,
                );
                if (entries.length === 0) return null;
                const heading =
                  group === "updated"
                    ? "Updated"
                    : group === "skipped"
                      ? "Skipped"
                      : "Needs attention";
                return (
                  <section key={group}>
                    <h3>
                      {heading} <span>{entries.length}</span>
                    </h3>
                    <div className={styles.planList}>
                      {entries.map((entry) => (
                        <div key={entry.label}>
                          <code>{entry.label}</code>
                          <span>
                            {preview && entry.outcome.status === "updated"
                              ? "Will update"
                              : entry.outcome.status}
                            {propagationDetail(entry.outcome) && (
                              <small>{propagationDetail(entry.outcome)}</small>
                            )}
                          </span>
                        </div>
                      ))}
                    </div>
                  </section>
                );
              })}
              {(preview?.preview.entries.length ?? report?.entries.length ?? 0) === 0 && (
                <p>No matching credentials.</p>
              )}
            </div>
            <div className={styles.dialogActions}>
              <button
                type="button"
                onClick={() => {
                  setPreview(null);
                  setReport(null);
                }}
              >
                Close
              </button>
              {preview && (
                <ActionButton
                  tone="primary"
                  disabled={mutationBusy || preview.preview.updates === 0}
                  onClick={() => void executePropagation()}
                >
                  {busy && <LoaderCircle className="spin" size={14} aria-hidden="true" />}
                  {busy
                    ? "Propagating…"
                    : `Propagate ${preview.preview.updates} credential update${preview.preview.updates === 1 ? "" : "s"}`}
                </ActionButton>
              )}
            </div>
          </section>
        </Dialog>
      )}
    </div>
  );
}
