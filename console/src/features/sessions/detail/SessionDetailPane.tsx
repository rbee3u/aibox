import { AlertTriangle, ChevronLeft } from "lucide-react";

import type { SessionApi } from "@/api/sessions";
import { SessionConversation } from "@/features/sessions/detail/SessionConversation";
import { SessionDetails } from "@/features/sessions/detail/SessionDetails";
import { messageCountLabel, toolCountLabel } from "@/features/sessions/detail/sessionFormat";
import { visibleSessionListSource } from "@/features/sessions/sessionSource";
import type { SessionViewModel } from "@/features/sessions/useSessionController";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { compactDuration, formatTimestamp } from "@/shared/lib/format";
import { EmptyState } from "@/shared/ui/EmptyState";
import { IconButton } from "@/shared/ui/IconButton";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import styles from "@/features/sessions/SessionPage.module.css";

const SessionIcon = resourceIcons.session;

export function SessionDetailPane({
  api,
  detail,
  mutations,
}: {
  api: SessionApi;
  detail: SessionViewModel["detail"];
  mutations: SessionViewModel["mutations"];
}) {
  const {
    closeSessionInspection,
    conversationScrollRef,
    currentSession,
    detailHeadingRef,
    detailMeta,
    detailRevision,
    detailStats,
    jumpToLatest,
    jumpToUserMessage,
    loadingDetail,
    onConversationScroll,
    openSession,
    registerUserMessage,
    resolvedActiveUserMessage,
    sessionTab,
    sessionWarnings,
    showJumpLatest,
    timeline,
    transcriptHasDiagnostics,
    transcriptIsPartial,
    updateSessionTab,
    userMessages,
  } = detail;
  return (
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
                {messageCountLabel(detailStats?.message_count ?? currentSession.message_count ?? 0)}{" "}
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
                <span className={`${styles.sessionDetailStatus} ${styles.sessionStatusWarning}`}>
                  Partial transcript
                </span>
              )}
              {!loadingDetail && detailStats && sessionWarnings.length > 0 && (
                <span className={`${styles.sessionDetailStatus} ${styles.sessionStatusWarning}`}>
                  <AlertTriangle size={13} aria-hidden="true" /> Transcript warning
                </span>
              )}
              <RefreshButton
                label="Refresh Session detail"
                busyLabel="Refreshing Session detail"
                busy={loadingDetail}
                iconOnly
                iconSize={15}
                disabled={mutations.deletionBusy}
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
              registerMessage={registerUserMessage}
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
  );
}
