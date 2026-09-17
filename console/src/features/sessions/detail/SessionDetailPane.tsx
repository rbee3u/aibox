import { AlertTriangle, ChevronLeft, Wrench } from "lucide-react";

import type { SessionApi } from "@/api/sessions";
import { SessionConversation } from "@/features/sessions/detail/SessionConversation";
import { SessionDetails } from "@/features/sessions/detail/SessionDetails";
import { messageCountLabel, toolCountLabel } from "@/features/sessions/sessionCatalog";
import { sessionListCopy } from "@/features/sessions/sessionListCopy";
import { sessionListTenantLabel } from "@/features/sessions/sessionSource";
import type { SessionViewModel } from "@/features/sessions/useSessionController";
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import { formatTimestamp } from "@/shared/lib/format";
import { EmptyState } from "@/shared/ui/EmptyState";
import { IconButton } from "@/shared/ui/IconButton";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import styles from "@/features/sessions/SessionPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

const SessionIcon = resourceIcons.session;
const HostTenantIcon = resourceIcons.hostTenant;
const ManagedTenantIcon = resourceIcons.managedTenant;

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
    detailStats,
    jumpToLatest,
    jumpToUserMessage,
    loadingDetail,
    onConversationScroll,
    openSession,
    refreshTranscript,
    registerUserMessage,
    resolvedActiveUserMessage,
    sessionTab,
    sessionWarnings,
    showJumpLatest,
    timeline,
    transcriptHasDiagnostics,
    transcriptAttentionNotice,
    transcriptIsPartial,
    updateSessionTab,
    userMessages,
  } = detail;
  const headline = currentSession
    ? sessionListCopy(currentSession.title, currentSession.latest_message).headline
    : "";
  return (
    <section className={styles.detailPane}>
      {currentSession ? (
        <>
          <header className={`${styles.detailHeader} ${styles.sessionDetailHeader}`}>
            <IconButton label="Back to Sessions" onClick={closeSessionInspection}>
              <ChevronLeft size={iconSize.md} />
            </IconButton>
            <div className={styles.sessionDetailHeading}>
              <h2 ref={detailHeadingRef} tabIndex={-1} title={headline}>
                {headline}
              </h2>
              <div className={styles.sessionDetailSource}>
                <span className={styles.contextPill}>
                  {currentSession.source.tenant.kind === "host" ? (
                    <HostTenantIcon size={iconSize.xs} aria-hidden="true" />
                  ) : (
                    <ManagedTenantIcon size={iconSize.xs} aria-hidden="true" />
                  )}
                  <small>Tenant: </small>
                  <strong>
                    {sessionListTenantLabel(currentSession.source.tenantSelectionValue)}
                  </strong>
                </span>
                <span className={styles.contextPill}>
                  <BrandIcon
                    brand={brandForAgent(currentSession.source.agent)}
                    size={iconSize.xs}
                  />
                  <small>Agent: </small>
                  <strong>{currentSession.source.agentLabel}</strong>
                </span>
                <span className={styles.contextPill}>
                  <time dateTime={currentSession.start_ts}>
                    {formatTimestamp(currentSession.start_ts)}
                  </time>
                  {` · ${messageCountLabel(detailStats?.message_count ?? currentSession.message_count ?? 0)} · ${toolCountLabel(detailStats?.tool_count ?? currentSession.tool_count ?? 0)}`}
                </span>
              </div>
            </div>
            <div className={styles.sessionDetailActions}>
              {loadingDetail && (
                <span className="srOnly" role="status">
                  Reading Transcript…
                </span>
              )}
              <RefreshButton
                label="Refresh Session detail"
                busyLabel="Refreshing Session detail"
                busy={loadingDetail}
                compactOnNarrow
                iconSize={15}
                disabled={mutations.deletionBusy}
                onClick={() => void openSession(currentSession, false, true)}
              >
                Refresh
              </RefreshButton>
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
              {transcriptAttentionNotice !== null && (
                <span
                  className={styles.sessionTabIssue}
                  aria-label="Transcript diagnostics"
                  title="Transcript diagnostics"
                >
                  <AlertTriangle size={iconSize.xs} aria-hidden="true" />
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
              attentionNotice={transcriptAttentionNotice}
              snapshot={detailStats?.snapshot}
              showJumpLatest={showJumpLatest}
              scrollRef={conversationScrollRef}
              registerMessage={registerUserMessage}
              onScroll={onConversationScroll}
              onSelectMessage={jumpToUserMessage}
              onJumpLatest={jumpToLatest}
              onViewDiagnostics={() => updateSessionTab("details")}
              onTranscriptStale={refreshTranscript}
            />
          )}
        </>
      ) : (
        <EmptyState
          variant="detail"
          icon={<SessionIcon size={iconSize.xl} data-icon="session-empty" aria-hidden="true" />}
          title="Select a Session"
          description="Choose a Session to inspect its conversation and Transcript."
        >
          <div className={styles.emptyStateGuide}>
            <div className={styles.emptyStateCard}>
              <div className={styles.emptyStateCardHeader}>
                <SessionIcon size={iconSize.sm} />
                <strong>Session Transcripts</strong>
                <span className={styles.emptyStateBadge}>Full Audit</span>
              </div>
              <p>
                Complete record of multi-turn interactions between users and Coding Agents. Review
                prompts, model reasoning, and historical dialogue progression.
              </p>
            </div>
            <div className={styles.emptyStateCard}>
              <div className={styles.emptyStateCardHeader}>
                <Wrench size={iconSize.sm} />
                <strong>Interactive Evidence</strong>
                <span className={styles.emptyStateBadge}>Tools & Calls</span>
              </div>
              <p>
                Drill down into individual tool invocations, shell command executions, file
                read/write operations, and diagnostic logs with raw payload disclosures.
              </p>
            </div>
          </div>
        </EmptyState>
      )}
    </section>
  );
}
