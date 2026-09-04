import { AlertTriangle, Check, ListChecks, Plus, Trash2 } from "lucide-react";

import { ConfigDriftBadge } from "@/features/configs/catalog/ConfigDriftBadge";
import {
  configIssueDescriptionId,
  configIssuePresentation,
  configWarningPresentation,
} from "@/features/configs/configCatalog";
import { configTenantSelectionValue, namedConfigName } from "@/features/configs/route";
import type { ConfigViewModel } from "@/features/configs/useConfigController";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { EmptyState } from "@/shared/ui/EmptyState";
import { ActionButton } from "@/shared/ui/ActionButton";
import { IconButton } from "@/shared/ui/IconButton";
import { IssueIndicator } from "@/shared/ui/IssueIndicator";
import { Loading } from "@/shared/ui/ManagementFeedback";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { SelectionMenu } from "@/shared/ui/SelectionMenu";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/configs/ConfigPage.module.css";

const CurrentConfigIcon = resourceIcons.currentConfig;
const ManagedTenantIcon = resourceIcons.managedTenant;
const NamedConfigIcon = resourceIcons.namedConfig;
export function ConfigCatalogPane({
  catalog,
  detail,
  dialogs,
  editor,
  feedback,
  mutations,
  selection: selectionState,
}: Pick<
  ConfigViewModel,
  "catalog" | "detail" | "dialogs" | "editor" | "feedback" | "mutations" | "selection"
>) {
  const {
    agent,
    agentOptions,
    catalog: data,
    loadCatalog,
    loadingCatalog,
    loadingTenants,
    managedTenantMissing,
    refreshing,
    selectAgent,
    selectTenant,
    tenant,
    tenantOptions,
  } = catalog;
  const { openConfig, openCurrent, selection } = detail;
  const { openCreateDialog, requestApply } = dialogs;
  const { requestEditorAction } = editor;
  const { appliedName, applyFeedback } = feedback;
  const { busy, createConfig, mutationBusy, previewPropagation, requestDelete } = mutations;
  const {
    allSelectable,
    cancelSelection,
    registerConfigRow,
    selectableNames,
    selectedCount,
    selectedKeys,
    selectionMode,
    enterSelection,
    toggleAllConfigs,
    toggleConfig,
  } = selectionState;
  return (
    <aside className={styles.configCatalog} aria-label="Configs">
      <div className={`${layout.toolbar} ${selectionMode ? layout.selectionBar : ""}`}>
        {selectionMode ? (
          <>
            <ActionButton
              tone="ghost"
              className={layout.selectionCancel}
              disabled={busy}
              onClick={cancelSelection}
            >
              Cancel
            </ActionButton>
            <div className={layout.selectionCenter}>
              <span className={layout.selectionCount}>{selectedCount} selected</span>
              <ActionButton
                tone="ghost"
                className={layout.selectionAll}
                disabled={selectableNames.length === 0 || busy}
                onClick={toggleAllConfigs}
              >
                {allSelectable ? "Clear all" : "Select all"}
              </ActionButton>
            </div>
            <ActionButton
              tone="danger"
              className={layout.selectionDelete}
              aria-label="Delete selected Named Configs"
              disabled={selectedCount === 0 || mutationBusy}
              onClick={() => requestDelete([...selectedKeys])}
            >
              <Trash2 size={14} aria-hidden="true" /> Delete
            </ActionButton>
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
                selected={new Set([configTenantSelectionValue(tenant)])}
                triggerIcon={<ManagedTenantIcon size={14} aria-hidden="true" />}
                unavailableSummary={
                  loadingTenants ? "Loading" : managedTenantMissing ? "Not found" : "Unavailable"
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
              <ActionButton
                tone="ghost"
                className={layout.selectionEnter}
                aria-label="Select Configs"
                disabled={selectableNames.length === 0 || loadingCatalog || refreshing || busy}
                onClick={enterSelection}
              >
                <ListChecks size={14} aria-hidden="true" /> Select
              </ActionButton>
            </div>
          </>
        )}
      </div>
      <div className={styles.configWarnings} aria-live="polite">
        {appliedName && !applyFeedback && (
          <AlertBanner
            className={styles.inlineNotice}
            tone="success"
            icon={<Check size={15} aria-hidden="true" />}
          >
            Last applied: <strong>Named Config {appliedName}</strong>. Application is a one-time
            projection to Current Config, not an Active Config.
          </AlertBanner>
        )}
        {applyFeedback && (
          <AlertBanner
            className={styles.inlineNotice}
            tone="success"
            icon={<Check size={15} aria-hidden="true" />}
          >
            {applyFeedback}
          </AlertBanner>
        )}
        {data?.application.drift === "source-missing" && (
          <AlertBanner
            className={styles.inlineWarning}
            tone="warning"
            icon={<AlertTriangle size={15} aria-hidden="true" />}
          >
            <span title={data.application.detail}>Last applied Named Config is missing.</span>
          </AlertBanner>
        )}
        {data?.application.drift === "comparison-error" && data.application.detail && (
          <AlertBanner
            className={styles.inlineWarning}
            tone="warning"
            icon={<AlertTriangle size={15} aria-hidden="true" />}
          >
            {data.application.detail}
          </AlertBanner>
        )}
      </div>
      <div className={layout.list} aria-busy={loadingCatalog}>
        {(loadingTenants || loadingCatalog) && !data && <Loading />}
        <div className={layout.rowGroup}>
          {!managedTenantMissing && (
            <div
              className={`${layout.row} ${selection.current ? layout.rowInspected : ""} ${selectionMode ? `${layout.rowSelectable} ${layout.rowProtected}` : ""}`}
            >
              <button
                ref={(element) => registerConfigRow("current", element)}
                type="button"
                className={styles.configRowMain}
                aria-label={selectionMode ? "Current Config cannot be selected" : "Current Config"}
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
                data?.credential_propagation_available && (
                  <ActionButton
                    tone="primarySoft"
                    className={`${styles.configRowPrimaryAction} ${styles.configPropagateAction}`}
                    aria-label="Propagate credentials"
                    disabled={mutationBusy}
                    onClick={() => void previewPropagation()}
                  >
                    Propagate credentials
                  </ActionButton>
                )}
            </div>
          )}
          <div className={layout.divider}>
            <span>Named Configs</span>
            <ActionButton
              tone="primarySoft"
              className={layout.addAction}
              aria-label="Create Named Config"
              disabled={mutationBusy || loadingCatalog || selectionMode}
              onClick={openCreateDialog}
            >
              <Plus size={15} aria-hidden="true" />
            </ActionButton>
          </div>
          {data?.configs.map((entry) => {
            const applied = entry.name === appliedName;
            const selectedForDeletion = selectedKeys.has(entry.name);
            const selectedForInspection = namedConfigName(selection) === entry.name;
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
                  ref={(element) => registerConfigRow(entry.name, element)}
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
                      {applied && <ConfigDriftBadge status={data.application} />}
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
                      <ActionButton
                        tone="primarySoft"
                        className={styles.configRowPrimaryAction}
                        aria-label={`Apply Named Config ${entry.name} to Current Config`}
                        disabled={mutationBusy || (applied && data.application.drift === "clean")}
                        onClick={() => requestApply(entry.name)}
                      >
                        Apply
                      </ActionButton>
                    )}
                    {entry.state === "incomplete" && (
                      <ActionButton
                        tone="primarySoft"
                        className={styles.configRowPrimaryAction}
                        aria-label={`Repair Named Config ${entry.name}`}
                        disabled={mutationBusy}
                        onClick={() => requestEditorAction(() => createConfig(entry.name))}
                      >
                        Repair
                      </ActionButton>
                    )}
                    <IconButton
                      className={`${layout.rowAction} ${layout.rowDeleteAction}`}
                      tone="dangerQuiet"
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
          {data && data.configs.length === 0 && !loadingCatalog && (
            <EmptyState
              variant="list"
              icon={<NamedConfigIcon size={22} aria-hidden="true" />}
              title="No Named Configs found."
            />
          )}
        </div>
      </div>
    </aside>
  );
}
