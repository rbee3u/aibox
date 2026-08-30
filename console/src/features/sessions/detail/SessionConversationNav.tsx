import type { ConversationMessage } from "@/api/sessions";
import { messageNavigationLabel } from "@/features/sessions/detail/sessionFormat";
import styles from "@/features/sessions/SessionPage.module.css";

interface SessionConversationNavProps {
  messages: ConversationMessage[];
  activeEntryId: string | null;
  /** Narrow layouts show the same anchors as a horizontal strip. */
  mobile?: boolean;
  onSelect: (entryId: string) => void;
}

/** One anchor per user Conversation Message, following the reading position. */
export function SessionConversationNav({
  messages,
  activeEntryId,
  mobile = false,
  onSelect,
}: SessionConversationNavProps) {
  if (messages.length === 0) return null;
  return (
    <nav
      className={mobile ? styles.sessionConversationMobileNav : styles.sessionConversationRail}
      aria-label="Conversation messages"
    >
      <div className={styles.sessionConversationNavItems}>
        {messages.map((message, index) => {
          const entryId = message.entry_ids[0] ?? `message-${index}`;
          const label = messageNavigationLabel(message.text);
          return (
            <button
              key={entryId}
              type="button"
              className={
                activeEntryId === entryId ? styles.sessionConversationNavActive : undefined
              }
              aria-current={activeEntryId === entryId ? "location" : undefined}
              aria-label={`Jump to message ${index + 1}: ${label}`}
              title={label}
              onClick={() => onSelect(entryId)}
            >
              <span className={styles.sessionConversationNavDot} aria-hidden="true">
                <span />
              </span>
              <span className={styles.sessionConversationNavIndex}>{index + 1}</span>
              <span className={styles.sessionConversationNavLabel}>{label}</span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}
