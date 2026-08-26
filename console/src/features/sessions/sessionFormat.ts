import type { ConversationMessage } from "@/api/sessions";
import { formatTimestamp } from "@/shared/lib/format";

/** Drops the date while a message falls on the Session's own start date. */
export function compactMessageTimestamp(value: string, sessionStart: string): string {
  const formatted = formatTimestamp(value);
  if (formatted === "—") return formatted;
  const start = formatTimestamp(sessionStart);
  const [date, time] = formatted.split(" ");
  const [startDate] = start.split(" ");
  return date === startDate ? time : formatted;
}

export function messageCountLabel(count: number): string {
  return `${count} message${count === 1 ? "" : "s"}`;
}

export function toolCountLabel(count: number): string {
  return `${count} tool${count === 1 ? "" : "s"}`;
}

/** Uses a message's first readable line as its navigator label. */
export function messageNavigationLabel(text: string): string {
  const firstLine = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find(Boolean);
  const value = firstLine || text.trim() || "Untitled message";
  return value.length > 96 ? `${value.slice(0, 93)}…` : value;
}

export function messageAnchorId(message: ConversationMessage): string {
  return `session-message-${message.entry_ids[0]?.replace(/[^a-zA-Z0-9_-]/g, "-") || "unknown"}`;
}

/** Reports whether the reader has scrolled well away from the newest message. */
export function conversationIsAwayFromLatest(element: HTMLDivElement): boolean {
  return element.scrollHeight - element.scrollTop - element.clientHeight > 160;
}
