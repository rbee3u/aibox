import type {
  ConversationMessage,
  SessionDetailMeta,
  SessionDetailStats,
  ToolActivity,
  TranscriptEvidenceSummary,
} from "@/api/sessions";

/**
 * A Transcript projects into an ordered reading stream. Conversation Messages
 * stand alone, while consecutive Tool Activity and Transcript Evidence records
 * collapse into one activity group that keeps its native order.
 */
export type SessionTimelineItem =
  | { kind: "message"; value: ConversationMessage }
  | { kind: "activity"; value: SessionActivityItem[] };

export type SessionActivityItem =
  { kind: "tool"; value: ToolActivity } | { kind: "evidence"; value: TranscriptEvidenceSummary };

export function sessionItemKey(item: SessionTimelineItem): string {
  if (item.kind === "message") return `message:${item.value.entry_ids.join(",")}`;
  return `activity:${item.value
    .map((entry) =>
      entry.kind === "tool"
        ? `tool:${entry.value.entry_ids.join(",")}:${entry.value.status}`
        : `evidence:${entry.value.entry_id}`,
    )
    .join(",")}`;
}

/**
 * Appends one activity record. A terminal Tool Activity that names an earlier
 * call updates that entry in place so a tool appears once with its final status;
 * anything else extends the trailing activity group or opens a new one.
 */
export function appendActivityItem(
  current: SessionTimelineItem[],
  entry: SessionActivityItem,
): SessionTimelineItem[] {
  const last = current.at(-1);
  if (entry.kind === "tool" && entry.value.status !== "started" && entry.value.call_id) {
    for (let cursor = current.length - 1; cursor >= 0; cursor -= 1) {
      const item = current[cursor];
      if (item.kind !== "activity") continue;
      const entryIndex = item.value.findIndex(
        (candidate) => candidate.kind === "tool" && candidate.value.call_id === entry.value.call_id,
      );
      if (entryIndex < 0) continue;
      const nextActivity = [...item.value];
      const existing = nextActivity[entryIndex];
      if (existing.kind === "tool") {
        nextActivity[entryIndex] = {
          kind: "tool",
          value: {
            ...existing.value,
            entry_ids: [...existing.value.entry_ids, ...entry.value.entry_ids],
            status: entry.value.status,
            summary: entry.value.summary || existing.value.summary,
          },
        };
      }
      const next = [...current];
      next[cursor] = { kind: "activity", value: nextActivity };
      return next;
    }
  }
  if (last?.kind === "activity") {
    return [...current.slice(0, -1), { kind: "activity", value: [...last.value, entry] }];
  }
  return [...current, { kind: "activity", value: [entry] }];
}

/** Adjacent Agent messages merge only when no other record separates them. */
export function appendConversationMessage(
  current: SessionTimelineItem[],
  message: ConversationMessage,
): SessionTimelineItem[] {
  const last = current.at(-1);
  if (message.role === "assistant" && last?.kind === "message" && last.value.role === "assistant") {
    return [
      ...current.slice(0, -1),
      {
        kind: "message",
        value: {
          ...last.value,
          entry_ids: [...last.value.entry_ids, ...message.entry_ids],
          timestamp: message.timestamp || last.value.timestamp,
          text: `${last.value.text}\n\n${message.text}`,
        },
      },
    ];
  }
  return [...current, { kind: "message", value: message }];
}

/** Summarizes one activity group for its collapsed disclosure. */
export function activitySummary(entries: SessionActivityItem[]): {
  count: number;
  labels: string[];
  hasIssue: boolean;
} {
  const labels = [
    ...new Set(
      entries
        .map((entry) => (entry.kind === "tool" ? entry.value.name : entry.value.native_type))
        .filter(Boolean),
    ),
  ];
  const hasIssue = entries.some((entry) => {
    if (entry.kind === "tool") {
      return !["started", "completed"].includes(entry.value.status);
    }
    return ["malformed", "unsupported", "hidden_internal"].includes(entry.value.status);
  });
  return { count: entries.length, labels, hasIssue };
}

export interface SessionDetailState {
  timeline: SessionTimelineItem[];
  meta: SessionDetailMeta | null;
  stats: SessionDetailStats | null;
  warnings: string[];
  loading: boolean;
}

export type SessionDetailAction =
  | { type: "reset" }
  | { type: "start"; preserveContent: boolean }
  | { type: "stop" }
  | { type: "meta"; value: SessionDetailMeta }
  | { type: "message"; value: ConversationMessage }
  | { type: "activity"; value: SessionActivityItem }
  | { type: "complete"; stats: SessionDetailStats; warnings: string[] }
  | {
      type: "replace";
      timeline: SessionTimelineItem[];
      meta: SessionDetailMeta | null;
      stats: SessionDetailStats | null;
      warnings: string[];
    };

export const emptySessionDetail: SessionDetailState = {
  timeline: [],
  meta: null,
  stats: null,
  warnings: [],
  loading: false,
};

/**
 * Accumulates the NDJSON detail stream. A manual refresh starts with
 * `preserveContent` so the previous Transcript stays visible until the new
 * stream succeeds.
 */
export function sessionDetailReducer(
  state: SessionDetailState,
  action: SessionDetailAction,
): SessionDetailState {
  switch (action.type) {
    case "reset":
      return emptySessionDetail;
    case "start":
      return action.preserveContent
        ? { ...state, loading: true }
        : { ...emptySessionDetail, loading: true };
    case "stop":
      return state.loading ? { ...state, loading: false } : state;
    case "meta":
      return { ...state, meta: action.value };
    case "message":
      return { ...state, timeline: appendConversationMessage(state.timeline, action.value) };
    case "activity":
      return { ...state, timeline: appendActivityItem(state.timeline, action.value) };
    case "complete":
      return { ...state, stats: action.stats, warnings: action.warnings };
    case "replace":
      return {
        timeline: action.timeline,
        meta: action.meta,
        stats: action.stats,
        warnings: action.warnings,
        loading: state.loading,
      };
  }
}
