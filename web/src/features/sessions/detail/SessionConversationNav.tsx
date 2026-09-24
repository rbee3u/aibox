import { useEffect, useRef } from "react";
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

/**
 * One numbered stop per user Conversation Message, following the reading
 * position. The rail renders even with nothing to number so the reading keeps
 * its column while the Transcript is still arriving.
 */
export function SessionConversationNav({
  messages,
  activeEntryId,
  mobile = false,
  onSelect,
}: SessionConversationNavProps) {
  const activeRef = useRef<HTMLButtonElement>(null);

  // A long Session has more stops than the rail shows at once; keep the
  // current one where the reader can see it.
  useEffect(() => {
    const active = activeRef.current;
    if (active && typeof active.scrollIntoView === "function") {
      active.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  }, [activeEntryId]);

  return (
    <nav
      className={mobile ? styles.sessionConversationMobileNav : styles.sessionConversationRail}
      aria-label="Conversation messages"
    >
      <div className={styles.sessionConversationNavItems}>
        {messages.map((message, index) => {
          const entryId = message.entry_ids[0] ?? `message-${index}`;
          const label = messageNavigationLabel(message.text);
          const active = activeEntryId === entryId;
          return (
            <button
              key={entryId}
              ref={active ? activeRef : undefined}
              type="button"
              className={active ? styles.sessionConversationNavActive : undefined}
              aria-current={active ? "location" : undefined}
              aria-label={`Jump to message ${index + 1}: ${label}`}
              title={label}
              onClick={() => onSelect(entryId)}
            >
              <span className={styles.sessionConversationNavMarker} aria-hidden="true">
                {index + 1}
              </span>
              <span className={styles.sessionConversationNavLabel}>{label}</span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}
