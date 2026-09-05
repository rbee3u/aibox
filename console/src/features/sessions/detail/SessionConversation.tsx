import { AlertTriangle, ArrowDown } from "lucide-react";
import type { RefObject, UIEvent } from "react";
import type { ConversationMessage, SessionApi } from "@/api/sessions";
import { SessionActivityGroup } from "@/features/sessions/detail/SessionActivityGroup";
import { SessionConversationNav } from "@/features/sessions/detail/SessionConversationNav";
import { SessionMessageContent } from "@/features/sessions/detail/SessionMessageContent";
import {
  conversationReadingTimeline,
  sessionItemKey,
  type SessionTimelineItem,
} from "@/features/sessions/detail/sessionDetail";
import { compactMessageTimestamp, messageAnchorId } from "@/features/sessions/detail/sessionFormat";
import type { SourcedSession } from "@/features/sessions/sessionSource";
import { formatTimestamp } from "@/shared/lib/format";
import { EmptyState } from "@/shared/ui/EmptyState";
import { IconButton } from "@/shared/ui/IconButton";
import { Loading } from "@/shared/ui/ManagementFeedback";
import { resourceIcons } from "@/shared/icons/consoleIcons";
import styles from "@/features/sessions/SessionPage.module.css";

const SessionIcon = resourceIcons.session;

interface SessionConversationProps {
  api: SessionApi;
  session: SourcedSession;
  timeline: SessionTimelineItem[];
  userMessages: ConversationMessage[];
  /** Anchor the navigator marks as current. */
  activeUserMessage: string | null;
  loading: boolean;
  needsAttention: boolean;
  snapshot?: string;
  /** Changes whenever the Session reloads, collapsing activity disclosures. */
  revision: number;
  showJumpLatest: boolean;
  scrollRef: RefObject<HTMLDivElement | null>;
  registerMessage: (entryId: string, element: HTMLElement | null) => void;
  onScroll: (event: UIEvent<HTMLDivElement>) => void;
  onSelectMessage: (entryId: string) => void;
  onJumpLatest: () => void;
  onViewDiagnostics: () => void;
}

/** The Conversation tab: a centered reading stream with its message navigator. */
export function SessionConversation({
  api,
  session,
  timeline,
  userMessages,
  activeUserMessage,
  loading,
  needsAttention,
  snapshot,
  revision,
  showJumpLatest,
  scrollRef,
  registerMessage,
  onScroll,
  onSelectMessage,
  onJumpLatest,
  onViewDiagnostics,
}: SessionConversationProps) {
  const readingTimeline = conversationReadingTimeline(timeline);
  return (
    <div className={styles.sessionConversationLayout}>
      <SessionConversationNav
        messages={userMessages}
        activeEntryId={activeUserMessage}
        onSelect={onSelectMessage}
      />
      <div className={styles.sessionConversationMain}>
        <SessionConversationNav
          messages={userMessages}
          activeEntryId={activeUserMessage}
          mobile
          onSelect={onSelectMessage}
        />
        <div ref={scrollRef} className={styles.sessionConversationScroll} onScroll={onScroll}>
          <div key={revision} className={styles.sessionConversationContent}>
            {needsAttention && (
              <button
                type="button"
                className={styles.sessionConversationWarning}
                onClick={onViewDiagnostics}
              >
                <AlertTriangle size={14} aria-hidden="true" />
                <span>Some transcript events could not be interpreted.</span>
                <span>View Details</span>
              </button>
            )}
            {readingTimeline.map((item) => {
              if (item.kind === "message") {
                const label = item.value.role === "user" ? "You" : session.source.agentLabel;
                const timestamp = compactMessageTimestamp(item.value.timestamp, session.start_ts);
                return (
                  <article
                    key={sessionItemKey(item)}
                    id={item.value.role === "user" ? messageAnchorId(item.value) : undefined}
                    ref={(element) => {
                      if (item.value.role !== "user") return;
                      const entryId = item.value.entry_ids[0];
                      if (entryId) registerMessage(entryId, element);
                    }}
                    className={`${styles.sessionMessage} ${item.value.role === "user" ? styles.sessionMessageUser : styles.sessionMessageAssistant}`}
                  >
                    <header>
                      <span>{label}</span>
                      <time
                        dateTime={item.value.timestamp}
                        title={formatTimestamp(item.value.timestamp)}
                      >
                        {timestamp}
                      </time>
                    </header>
                    <SessionMessageContent role={item.value.role} text={item.value.text} />
                  </article>
                );
              }
              return (
                <SessionActivityGroup
                  key={sessionItemKey(item)}
                  api={api}
                  entries={item.value}
                  reloadRevision={revision}
                  session={session}
                  snapshot={snapshot}
                />
              );
            })}
            {loading && <Loading />}
            {!loading && readingTimeline.length === 0 && (
              <EmptyState
                className={styles.promptEmptyState}
                variant="detail"
                icon={<SessionIcon size={26} aria-hidden="true" />}
                title="No readable conversation"
                description="This Transcript contains no supported user or Coding Agent messages. Transcript events stay on Details."
              />
            )}
          </div>
          {showJumpLatest && (
            <IconButton className={styles.jumpLatest} label="Jump to latest" onClick={onJumpLatest}>
              <ArrowDown size={16} aria-hidden="true" />
            </IconButton>
          )}
        </div>
      </div>
    </div>
  );
}
