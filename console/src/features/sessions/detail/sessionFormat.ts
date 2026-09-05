import type { ConversationMessage } from "@/api/sessions";
import { userMessageReadingText } from "@/features/sessions/sessionListCopy";
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

const TOOL_INPUT_KEYS = [
  "command",
  "cmd",
  "cmd_string",
  "input",
  "query",
  "path",
  "file",
  "pattern",
];

/** First readable line of a Tool Activity summary, for the collapsed row. */
export function toolActivityHeadline(summary: string, max = 96): string | null {
  const first = summary
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find(Boolean);
  if (!first) return null;
  const readable = unwrapToolSummary(first) ?? first;
  const compact = readable.replace(/\s+/g, " ").trim();
  if (!compact) return null;
  return compact.length > max ? `${compact.slice(0, max - 1)}…` : compact;
}

function unwrapToolSummary(value: string): string | null {
  try {
    const parsed: unknown = JSON.parse(value);
    if (typeof parsed === "string") return parsed.trim() || null;
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      const record = parsed as Record<string, unknown>;
      for (const key of TOOL_INPUT_KEYS) {
        const field = record[key];
        if (typeof field === "string" && field.trim()) return field.trim();
      }
    }
    return null;
  } catch {
    return null;
  }
}

/** Uses a message's first readable line as its navigator label. */
export function messageNavigationLabel(text: string): string {
  const reading = userMessageReadingText(text);
  const firstLine = reading
    .split(/\r?\n/)
    .map((line) => line.trim())
    .find(Boolean);
  const value = firstLine || reading.trim() || "Untitled message";
  return value.length > 96 ? `${value.slice(0, 93)}…` : value;
}

export function messageAnchorId(message: ConversationMessage): string {
  return `session-message-${message.entry_ids[0]?.replace(/[^a-zA-Z0-9_-]/g, "-") || "unknown"}`;
}

/** Reports whether the reader has scrolled well away from the newest message. */
export function conversationIsAwayFromLatest(element: HTMLDivElement): boolean {
  return element.scrollHeight - element.scrollTop - element.clientHeight > 160;
}
