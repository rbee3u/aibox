import { AlertTriangle, Box, ChevronLeft, ListChecks, Trash2 } from "lucide-react";

import type { Operation } from "@/api/operations";
import type { SessionApi } from "@/api/sessions";
import { SessionConversation } from "@/features/sessions/components/SessionConversation";
import { SessionDetails } from "@/features/sessions/components/SessionDetails";
import { SessionRow } from "@/features/sessions/components/SessionRow";
import { messageCountLabel, toolCountLabel } from "@/features/sessions/sessionFormat";
import { visibleSessionListSource, visibleSessionSource } from "@/features/sessions/sessionSource";
import { useSessionController } from "@/features/sessions/useSessionController";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { compactDuration, formatTimestamp } from "@/shared/lib/format";
import type { ModuleLocationChange } from "@/shared/lib/navigation";
import { ConfirmDialog } from "@/shared/ui/ConfirmDialog";
import { EmptyState } from "@/shared/ui/EmptyState";
import { IconButton } from "@/shared/ui/IconButton";
import { Loading, MutationUnavailable, PageError } from "@/shared/ui/ManagementFeedback";
import { NotificationCenter } from "@/shared/ui/NotificationCenter";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { SelectionMenu } from "@/shared/ui/SelectionMenu";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/sessions/SessionPage.module.css";

const SessionIcon = resourceIcons.session;
const ManagedTenantIcon = resourceIcons.managedTenant;

interface PageProps {
  api: SessionApi;
  operation?: Operation | null;
  search: string;
  onLocationChange: ModuleLocationChange;
}

export function SessionPage(props: PageProps) {
  const {
    agentOptions,
    allSelected,
    batchBusy,
    cancelSelection,
    closeSessionInspection,
    commitAgents,
    commitTenants,
    conversationScrollRef,
    currentSession,
    data,
    deleteButtons,
    deleteSelectedSessions,
    deleteSession,
    deletion,
    deletionBusy,
    detailHeadingRef,
    detailMeta,
    detailRevision,
    detailStats,
    dialogKeys,
    dialogSources,
    dismissNotification,
    error,
    jumpToLatest,
    jumpToUserMessage,
    load,
    loadingDetail,
    loadingList,
    loadingTenants,
    mutationBusy,
    notifications,
    onConversationScroll,
    openSession,
    refreshButton,
    refreshing,
    resolvedActiveUserMessage,
    retryPageError,
    retryTenants,
    selectedAgents,
    selectedKeys,
    selectedTenants,
    selectionMode,
    selectButton,
    sessionRowButtons,
    sessions,
    sessionTab,
    sessionTenantMissing,
    sessionWarnings,
    setDialogKeys,
    setSelectionMode,
    setSingleDeleteTarget,
    showJumpLatest,
    singleDeleteTarget,
    tenantError,
    tenantOptions,
    timeline,
    toggleAllSessions,
    toggleSession,
    transcriptHasDiagnostics,
    transcriptIsPartial,
    unsafeView,
    updateSessionTab,
    userMessageRefs,
    userMessages,
  } = useSessionController(props);
  const { api, operation } = props;
  return (
    <div className={`${layout.page} ${layout.catalogPage} ${styles.sessionPage}`}>
      <PageError
        error={tenantError ?? error}
        onRetry={tenantError ? retryTenants : error ? retryPageError : undefined}
      />
      <MutationUnavailable operation={operation} />
      <div className={`${styles.splitLayout} ${currentSession ? layout.showsDetail : ""}`}>
        <aside className={`${layout.catalogPanel} ${styles.sessionCatalog}`} aria-label="Sessions">
          <div className={`${layout.toolbar} ${selectionMode ? layout.selectionBar : ""}`}>
            {selectionMode ? (
              <>
                <button
                  type="button"
                  className={layout.selectionCancel}
                  disabled={deletionBusy}
                  onClick={cancelSelection}
                >
                  Cancel
                </button>
                <div className={layout.selectionCenter}>
                  <span className={layout.selectionCount} title={`${selectedKeys.size} selected`}>
                    {selectedKeys.size} selected
                  </span>
                  <button
                    type="button"
                    className={layout.selectionAll}
                    onClick={toggleAllSessions}
                    disabled={sessions.length === 0 || deletionBusy}
                  >
                    {allSelected ? "Clear all" : "Select all"}
                  </button>
                </div>
                <button
                  type="button"
                  className={layout.selectionDelete}
                  aria-label="Delete selected Sessions"
                  disabled={selectedKeys.size === 0 || mutationBusy}
                  onClick={() => setDialogKeys([...selectedKeys])}
                >
                  <Trash2 size={14} aria-hidden="true" />
                  Delete
                </button>
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
                      loadingTenants
                        ? "Loading"
                        : sessionTenantMissing
                          ? "Not found"
                          : "Unavailable"
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
                        <BrandIcon
                          brand={brandForAgent([...selectedAgents][0] ?? "codex")}
                          size={14}
                        />
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
                  <button
                    ref={selectButton}
                    type="button"
                    className={layout.selectionEnter}
                    aria-label="Select Sessions"
                    disabled={
                      sessions.length === 0 ||
                      unsafeView ||
                      loadingList ||
                      refreshing ||
                      deletionBusy
                    }
                    onClick={() => setSelectionMode(true)}
                  >
                    <ListChecks size={14} aria-hidden="true" />
                    Select
                  </button>
                </div>
              </>
            )}
          </div>
          <div className={styles.sessionWarnings}>
            {data?.warnings.map((warning) => (
              <div className={styles.inlineWarning} key={warning}>
                <AlertTriangle size={15} aria-hidden="true" />
                <span>{warning}</span>
              </div>
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
                onOpen={() => void openSession(row)}
                onToggle={() => toggleSession(row.key)}
                onDelete={() => setSingleDeleteTarget(row)}
                registerRow={(element) => {
                  if (element) sessionRowButtons.current.set(row.key, element);
                  else sessionRowButtons.current.delete(row.key);
                }}
                registerDelete={(element) => {
                  if (element) deleteButtons.current.set(row.key, element);
                  else deleteButtons.current.delete(row.key);
                }}
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
        <section className={styles.detailPane}>
          {currentSession ? (
            <>
              <header className={`${styles.detailHeader} ${styles.sessionDetailHeader}`}>
                <IconButton label="Back to Sessions" onClick={closeSessionInspection}>
                  <ChevronLeft size={17} />
                </IconButton>
                <div className={styles.sessionDetailHeading}>
                  <h2 ref={detailHeadingRef} tabIndex={-1}>
                    {currentSession.title || "Untitled Session"}
                  </h2>
                  <span className={styles.sessionDetailSource}>
                    {visibleSessionListSource(currentSession.source)} ·{" "}
                    <time dateTime={currentSession.start_ts}>
                      {formatTimestamp(currentSession.start_ts)}
                    </time>{" "}
                    · {compactDuration(detailStats?.observed_duration_ms)} ·{" "}
                    {messageCountLabel(
                      detailStats?.message_count ?? currentSession.message_count ?? 0,
                    )}{" "}
                    · {toolCountLabel(detailStats?.tool_count ?? currentSession.tool_count ?? 0)}
                  </span>
                </div>
                <div className={styles.sessionDetailActions}>
                  {loadingDetail && (
                    <span className={styles.sessionDetailStatus} role="status">
                      Reading Transcript…
                    </span>
                  )}
                  {!loadingDetail && !detailStats && (
                    <span
                      className={`${styles.sessionDetailStatus} ${styles.sessionStatusWarning}`}
                    >
                      Partial transcript
                    </span>
                  )}
                  {!loadingDetail && detailStats && sessionWarnings.length > 0 && (
                    <span
                      className={`${styles.sessionDetailStatus} ${styles.sessionStatusWarning}`}
                    >
                      <AlertTriangle size={13} aria-hidden="true" /> Transcript warning
                    </span>
                  )}
                  <RefreshButton
                    label="Refresh Session detail"
                    busyLabel="Refreshing Session detail"
                    busy={loadingDetail}
                    iconOnly
                    iconSize={15}
                    disabled={deletionBusy}
                    onClick={() => void openSession(currentSession, false, true)}
                  />
                </div>
              </header>
              <nav className={styles.sessionTabs} aria-label="Session views">
                <button
                  type="button"
                  className={sessionTab === "conversation" ? styles.sessionTabActive : undefined}
                  aria-current={sessionTab === "conversation" ? "page" : undefined}
                  onClick={() => updateSessionTab("conversation")}
                >
                  Conversation
                </button>
                <button
                  type="button"
                  className={sessionTab === "details" ? styles.sessionTabActive : undefined}
                  aria-current={sessionTab === "details" ? "page" : undefined}
                  onClick={() => updateSessionTab("details")}
                >
                  Details
                  {transcriptHasDiagnostics && (
                    <span
                      className={styles.sessionTabIssue}
                      aria-label="Transcript diagnostics"
                      title="Transcript diagnostics"
                    >
                      <AlertTriangle size={11} aria-hidden="true" />
                    </span>
                  )}
                </button>
              </nav>
              {sessionTab === "details" ? (
                <SessionDetails
                  session={currentSession}
                  meta={detailMeta}
                  stats={detailStats}
                  warnings={sessionWarnings}
                  loading={loadingDetail}
                  hasDiagnostics={transcriptHasDiagnostics}
                  partial={transcriptIsPartial}
                />
              ) : (
                <SessionConversation
                  api={api}
                  session={currentSession}
                  timeline={timeline}
                  userMessages={userMessages}
                  activeUserMessage={resolvedActiveUserMessage}
                  loading={loadingDetail}
                  warnings={sessionWarnings}
                  snapshot={detailStats?.snapshot}
                  revision={detailRevision}
                  showJumpLatest={showJumpLatest}
                  scrollRef={conversationScrollRef}
                  messageRefs={userMessageRefs}
                  onScroll={onConversationScroll}
                  onSelectMessage={jumpToUserMessage}
                  onJumpLatest={jumpToLatest}
                  onViewDiagnostics={() => updateSessionTab("details")}
                />
              )}
            </>
          ) : (
            <EmptyState
              variant="detail"
              icon={<SessionIcon size={26} data-icon="session-empty" aria-hidden="true" />}
              title="Select a Session"
              description="Choose a Session to inspect its conversation and Transcript."
            />
          )}
        </section>
      </div>
      <NotificationCenter
        notifications={notifications.map((notification) => ({
          ...notification,
          actionLabel: undefined,
        }))}
        paused={dialogKeys !== null || singleDeleteTarget !== null}
        onAction={() => undefined}
        onDismiss={dismissNotification}
      />
      {singleDeleteTarget && (
        <ConfirmDialog
          title={`Delete Session ${singleDeleteTarget.display_id}?`}
          message={`This permanently deletes its Transcript from ${visibleSessionSource(singleDeleteTarget.source)}.`}
          confirmLabel="Delete permanently"
          busy={deletion?.kind === "record" || operation?.state === "running"}
          onCancel={() => {
            if (deletion?.kind !== "record") setSingleDeleteTarget(null);
          }}
          onConfirm={() => void deleteSession(singleDeleteTarget)}
        />
      )}
      {dialogKeys && (
        <ConfirmDialog
          title={`Delete ${dialogKeys.length} selected Session${dialogKeys.length === 1 ? "" : "s"}?`}
          message={`This permanently deletes the Transcripts for the selected Sessions. Sources: ${dialogSources
            .map(({ count, source }) => `${visibleSessionSource(source)} (${count})`)
            .join("; ")}.`}
          confirmLabel="Delete permanently"
          busy={batchBusy || operation?.state === "running"}
          onCancel={() => {
            if (!batchBusy) setDialogKeys(null);
          }}
          onConfirm={() => void deleteSelectedSessions()}
        />
      )}
    </div>
  );
}
