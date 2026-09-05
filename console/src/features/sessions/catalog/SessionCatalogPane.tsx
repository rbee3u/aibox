import { AlertTriangle, Box, ListChecks, Trash2 } from "lucide-react";

import { SessionRow } from "@/features/sessions/catalog/SessionRow";
import type { SessionViewModel } from "@/features/sessions/useSessionController";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { ActionButton } from "@/shared/ui/ActionButton";
import { EmptyState } from "@/shared/ui/EmptyState";
import { Loading } from "@/shared/ui/ManagementFeedback";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { SelectionMenu } from "@/shared/ui/SelectionMenu";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/sessions/SessionPage.module.css";

const SessionIcon = resourceIcons.session;
const ManagedTenantIcon = resourceIcons.managedTenant;

export function SessionCatalogPane({
  catalog,
  detail,
  dialogs,
  mutations,
  selection,
}: Pick<SessionViewModel, "catalog" | "detail" | "dialogs" | "mutations" | "selection">) {
  const {
    agentOptions,
    commitAgents,
    commitTenants,
    data,
    load,
    loadingList,
    loadingTenants,
    refreshButton,
    refreshing,
    selectedAgents,
    selectedTenants,
    sessions,
    sessionTenantMissing,
    tenantOptions,
  } = catalog;
  const { currentSession, openSession, unsafeView } = detail;
  const { openBatchDelete, openSingleDelete, registerDeleteButton, selectButton } = dialogs;
  const { deletion, deletionBusy, mutationBusy } = mutations;
  const {
    allSelected,
    cancelSelection,
    selectedKeys,
    selectionMode,
    registerSessionRow,
    enterSelection,
    toggleAllSessions,
    toggleSession,
  } = selection;
  return (
    <aside className={`${layout.catalogPanel} ${styles.sessionCatalog}`} aria-label="Sessions">
      <div className={`${layout.toolbar} ${selectionMode ? layout.selectionBar : ""}`}>
        {selectionMode ? (
          <>
            <ActionButton
              tone="ghost"
              className={layout.selectionCancel}
              disabled={deletionBusy}
              onClick={cancelSelection}
            >
              Cancel
            </ActionButton>
            <div className={layout.selectionCenter}>
              <span className={layout.selectionCount} title={`${selectedKeys.size} selected`}>
                {selectedKeys.size} selected
              </span>
              <ActionButton
                tone="ghost"
                className={layout.selectionAll}
                onClick={toggleAllSessions}
                disabled={sessions.length === 0 || deletionBusy}
              >
                {allSelected ? "Clear all" : "Select all"}
              </ActionButton>
            </div>
            <ActionButton
              tone="danger"
              className={layout.selectionDelete}
              aria-label="Delete selected Sessions"
              disabled={selectedKeys.size === 0 || mutationBusy}
              onClick={() => openBatchDelete([...selectedKeys])}
            >
              <Trash2 size={14} aria-hidden="true" />
              Delete
            </ActionButton>
          </>
        ) : (
          <>
            <div className={layout.toolbarFilters}>
              <SelectionMenu
                className={layout.filterControl}
                disabled={loadingTenants || deletionBusy}
                label="Tenant"
                onCommit={commitTenants}
                options={tenantOptions}
                pluralLabel="tenants"
                selected={selectedTenants}
                triggerIcon={<ManagedTenantIcon size={14} aria-hidden="true" />}
                unavailableSummary={
                  loadingTenants ? "Loading" : sessionTenantMissing ? "Not found" : "Unavailable"
                }
              />
              <SelectionMenu
                className={layout.filterControl}
                disabled={deletionBusy}
                label="Coding Agent"
                onCommit={commitAgents}
                options={agentOptions}
                pluralLabel="Coding Agents"
                selected={selectedAgents}
                triggerIcon={
                  selectedAgents.size === 1 ? (
                    <BrandIcon brand={brandForAgent([...selectedAgents][0] ?? "codex")} size={14} />
                  ) : (
                    <Box size={14} aria-hidden="true" />
                  )
                }
              />
            </div>
            <div className={layout.toolbarActions}>
              <RefreshButton
                ref={refreshButton}
                data-dialog-focus-fallback="true"
                className={layout.refreshAction}
                label="Refresh Sessions"
                busyLabel="Refreshing Sessions"
                busy={refreshing}
                disabled={loadingList || refreshing || deletionBusy}
                onClick={() => void load("refresh")}
              >
                Refresh
              </RefreshButton>
              <ActionButton
                ref={selectButton}
                tone="ghost"
                className={layout.selectionEnter}
                aria-label="Select Sessions"
                disabled={
                  sessions.length === 0 || unsafeView || loadingList || refreshing || deletionBusy
                }
                onClick={enterSelection}
              >
                <ListChecks size={14} aria-hidden="true" />
                Select
              </ActionButton>
            </div>
          </>
        )}
      </div>
      <div className={styles.sessionWarnings}>
        {data?.warnings.map((warning) => (
          <AlertBanner
            className={styles.inlineWarning}
            key={warning}
            tone="warning"
            icon={<AlertTriangle size={15} aria-hidden="true" />}
          >
            {warning}
          </AlertBanner>
        ))}
      </div>
      <div className={`${styles.catalogList} ${styles.sessionList}`} aria-busy={loadingList}>
        {!data && loadingList && <Loading />}
        {sessions.map((row) => (
          <SessionRow
            key={row.key}
            row={row}
            current={currentSession?.key === row.key}
            selectionMode={selectionMode}
            selected={selectedKeys.has(row.key)}
            deleting={deletion?.kind === "record" && deletion.key === row.key}
            mutationBusy={mutationBusy}
            deletionBusy={deletionBusy}
            loadingList={loadingList}
            unsafeView={unsafeView}
            showSource={selectedTenants.size > 1 || selectedAgents.size > 1}
            onOpen={() => void openSession(row)}
            onToggle={() => toggleSession(row.key)}
            onDelete={() => openSingleDelete(row)}
            registerRow={(element) => registerSessionRow(row.key, element)}
            registerDelete={(element) => registerDeleteButton(row.key, element)}
          />
        ))}
        {data?.sessions.length === 0 && !loadingList && (
          <EmptyState
            variant="list"
            icon={<SessionIcon size={22} data-icon="session-list-empty" aria-hidden="true" />}
            title="No Sessions found"
            description="No Sessions were found for the selected Tenants and Coding Agents."
          />
        )}
      </div>
    </aside>
  );
}
