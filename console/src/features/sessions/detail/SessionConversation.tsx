import { AlertTriangle, ArrowDown, Ban, User, type LucideIcon } from "lucide-react";
import type { RefObject, UIEvent } from "react";
import type { ConversationMessage, ConversationNotice, SessionApi } from "@/api/sessions";
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
import { BrandIcon, brandForAgent } from "@/shared/icons/brandIcons";
import { resourceIcons, toneIcons } from "@/shared/icons/consoleIcons";
import styles from "@/features/sessions/SessionPage.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

const SessionIcon = resourceIcons.session;

/**
 * A notice is a line the CLI wrote in a speaker's slot. It keeps its verbatim
 * text but drops the author, so a failed request never reads as the Agent's
 * last sentence and an interruption never reads as something the user typed.
 */
const conversationNotices: Record<
  ConversationNotice,
  { icon: LucideIcon; label: string; className: "sessionNoticeError" | "sessionNoticeMuted" }
> = {
  api_error: { icon: toneIcons.error, label: "Request failed", className: "sessionNoticeError" },
  interrupted: { icon: Ban, label: "Turn interrupted", className: "sessionNoticeMuted" },
};

interface SessionConversationProps {
  api: SessionApi;
  session: SourcedSession;
  timeline: SessionTimelineItem[];
  userMessages: ConversationMessage[];
  /** Anchor the navigator marks as current. */
  activeUserMessage: string | null;
  loading: boolean;
  /** Why the reading below may be incomplete; `null` when it is not. */
  attentionNotice: string | null;
  snapshot?: string;
  showJumpLatest: boolean;
  scrollRef: RefObject<HTMLDivElement | null>;
  registerMessage: (entryId: string, element: HTMLElement | null) => void;
  onScroll: (event: UIEvent<HTMLDivElement>) => void;
  onSelectMessage: (entryId: string) => void;
  onJumpLatest: () => void;
  onViewDiagnostics: () => void;
  /** The Transcript grew under an evidence read; re-read it in place. */
  onTranscriptStale: () => Promise<string | null>;
}

/** The Conversation tab: a centered reading stream with its message navigator. */
export function SessionConversation({
  api,
  session,
  timeline,
  userMessages,
  activeUserMessage,
  loading,
  attentionNotice,
  snapshot,
  showJumpLatest,
  scrollRef,
  registerMessage,
  onScroll,
  onSelectMessage,
  onJumpLatest,
  onViewDiagnostics,
  onTranscriptStale,
}: SessionConversationProps) {
  const readingTimeline = conversationReadingTimeline(timeline);
  const singleTurn = userMessages.length <= 1;
  return (
    <div
      className={`${styles.sessionConversationLayout} ${singleTurn ? styles.singleTurnLayout : ""}`}
    >
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
          <div className={styles.sessionConversationContent}>
            {attentionNotice !== null && (
              <button
                type="button"
                className={styles.sessionConversationWarning}
                onClick={onViewDiagnostics}
              >
                <AlertTriangle size={iconSize.xs} aria-hidden="true" />
                <span>{attentionNotice}</span>
                <span>View Details</span>
              </button>
            )}
            {readingTimeline.map((item) => {
              if (item.kind === "message" && item.value.notice) {
                const notice = conversationNotices[item.value.notice];
                const NoticeIcon = notice.icon;
                return (
                  <div
                    key={sessionItemKey(item)}
                    className={`${styles.sessionNotice} ${styles[notice.className]}`}
                    role="note"
                    aria-label={notice.label}
                  >
                    <NoticeIcon size={iconSize.xs} aria-hidden="true" />
                    <span>{item.value.text}</span>
                    <time
                      dateTime={item.value.timestamp}
                      title={formatTimestamp(item.value.timestamp)}
                    >
                      {compactMessageTimestamp(item.value.timestamp, session.start_ts)}
                    </time>
                  </div>
                );
              }
              if (item.kind === "message") {
                const isUser = item.value.role === "user";
                const label = isUser ? "You" : session.source.agentLabel;
                const timestamp = compactMessageTimestamp(item.value.timestamp, session.start_ts);
                return (
                  <article
                    key={sessionItemKey(item)}
                    id={isUser ? messageAnchorId(item.value) : undefined}
                    ref={(element) => {
                      if (!isUser) return;
                      const entryId = item.value.entry_ids[0];
                      if (entryId) registerMessage(entryId, element);
                    }}
                    className={`${styles.sessionMessage} ${isUser ? styles.sessionMessageUser : styles.sessionMessageAssistant}`}
                  >
                    <header>
                      <div className={styles.sessionMessageSender}>
                        <span className={styles.sessionMessageAvatar} aria-hidden="true">
                          {isUser ? (
                            <User size={iconSize.xs} />
                          ) : (
                            <BrandIcon
                              brand={brandForAgent(session.source.agent)}
                              size={iconSize.xs}
                            />
                          )}
                        </span>
                        <span>{label}</span>
                      </div>
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
                  session={session}
                  snapshot={snapshot}
                  onTranscriptStale={onTranscriptStale}
                />
              );
            })}
            {loading && <Loading />}
            {!loading && readingTimeline.length === 0 && (
              <EmptyState
                className={styles.promptEmptyState}
                variant="detail"
                icon={<SessionIcon size={iconSize.xl} aria-hidden="true" />}
                title="No readable conversation"
                description="This Transcript contains no supported user or Agent messages. Transcript events stay on Details."
              />
            )}
          </div>
          {showJumpLatest && (
            <IconButton className={styles.jumpLatest} label="Jump to latest" onClick={onJumpLatest}>
              <ArrowDown size={iconSize.sm} aria-hidden="true" />
            </IconButton>
          )}
        </div>
      </div>
    </div>
  );
}
