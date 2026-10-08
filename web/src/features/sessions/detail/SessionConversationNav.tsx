import styles from "@/features/sessions/detail/SessionConversationNav.module.css";
import conversationStyles from "@/features/sessions/detail/conversation.module.css";
import { useEffect, useRef } from "react";
import type { ConversationMessage } from "@/api/sessions";
import { messageNavigationLabel } from "@/features/sessions/detail/sessionFormat";

interface SessionConversationNavProps {
  messages: ConversationMessage[];
  activeEntryId: string | null;
  /** Narrow layouts show the same anchors as a horizontal strip. */
  mobile?: boolean;
  onSelect: (entryId: string) => void;
}

/** Keep the empty rail mounted to preserve the reading column during streaming. */
export function SessionConversationNav({
  messages,
  activeEntryId,
  mobile = false,
  onSelect,
}: SessionConversationNavProps) {
  const activeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    const active = activeRef.current;
    if (active && typeof active.scrollIntoView === "function") {
      active.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  }, [activeEntryId]);

  return (
    <nav
      className={
        mobile
          ? conversationStyles.sessionConversationMobileNav
          : conversationStyles.sessionConversationRail
      }
      aria-label="Conversation messages"
    >
      <div className={conversationStyles.sessionConversationNavItems}>
        {messages.map((message, index) => {
          const entryId = message.entry_ids[0] ?? `message-${index}`;
          const label = messageNavigationLabel(message.text);
          const active = activeEntryId === entryId;
          return (
            <button
              key={entryId}
              ref={active ? activeRef : undefined}
              type="button"
              className={active ? conversationStyles.sessionConversationNavActive : undefined}
              aria-current={active ? "location" : undefined}
              aria-label={`Jump to message ${index + 1}: ${label}`}
              title={label}
              onClick={() => onSelect(entryId)}
            >
              <span className={conversationStyles.sessionConversationNavMarker} aria-hidden="true">
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
