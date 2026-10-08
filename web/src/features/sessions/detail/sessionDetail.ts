import type { SessionTimelineItem, SessionActivityItem } from "@/features/sessions/viewTypes";
import type {
  ConversationMessage,
  SessionDetailMeta,
  SessionDetailStats,
  ToolActivity,
} from "@/api/sessions";

/**
 * Keep keys stable as groups grow or tool results arrive, preserving disclosures.
 */
export function sessionItemKey(item: SessionTimelineItem): string {
  if (item.kind === "message") return `message:${item.value.entry_ids[0]}`;
  const first = item.value[0];
  return `activity:${first.kind === "tool" ? first.value.entry_ids[0] : first.value.entry_id}`;
}

/**
 * Merge terminal Tool Activity into its earlier call. An unanswered call repeats
 * its original entry ids; update its status without inventing a result.
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
        const answered = !entry.value.entry_ids.every((id) =>
          existing.value.entry_ids.includes(id),
        );
        nextActivity[entryIndex] = {
          kind: "tool",
          value: {
            ...existing.value,
            entry_ids: answered
              ? [...existing.value.entry_ids, ...entry.value.entry_ids]
              : existing.value.entry_ids,
            status: entry.value.status,
          },
          result: answered ? entry.value : undefined,
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

export function isConversationNotice(message: ConversationMessage): boolean {
  return message.notice !== undefined;
}

/**
 * Keep CLI notices separate so errors are not attributed to the Agent.
 */
export function appendConversationMessage(
  current: SessionTimelineItem[],
  message: ConversationMessage,
): SessionTimelineItem[] {
  const last = current.at(-1);
  if (
    message.role === "assistant" &&
    !isConversationNotice(message) &&
    last?.kind === "message" &&
    last.value.role === "assistant" &&
    !isConversationNotice(last.value)
  ) {
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

/**
 * No result is neutral: the Transcript cannot distinguish a running call
 * from one abandoned by the CLI.
 */
export function toolNeedsAttention(status: ToolActivity["status"]): boolean {
  return status === "failed" || status === "unknown";
}

function evidenceNeedsAttention(status: string): boolean {
  return status === "malformed";
}

/**
 * Hide CLI housekeeping and internal reasoning from Conversation.
 */
export function isRoutineEvidence(entry: SessionActivityItem): boolean {
  return (
    entry.kind === "evidence" &&
    (entry.value.status === "filtered" || entry.value.status === "hidden_internal")
  );
}

export function activitySummary(entries: SessionActivityItem[]): {
  count: number;
  toolCount: number;
  /** Evidence the reader could not project: unsupported or malformed. */
  diagnosticCount: number;
  routineCount: number;
  labels: string[];
  title: string;
  detail: string;
  hasIssue: boolean;
} {
  const toolCount = entries.filter((entry) => entry.kind === "tool").length;
  const routineCount = entries.filter(isRoutineEvidence).length;
  const diagnostics = entries.filter(
    (entry) => entry.kind === "evidence" && !isRoutineEvidence(entry),
  );
  const hasIssue = entries.some((entry) =>
    entry.kind === "tool"
      ? toolNeedsAttention(entry.value.status)
      : evidenceNeedsAttention(entry.value.status),
  );
  const diagnosticDetail = Object.entries(
    diagnostics.reduce<Record<string, number>>((counts, entry) => {
      if (entry.kind === "evidence")
        counts[entry.value.status] = (counts[entry.value.status] ?? 0) + 1;
      return counts;
    }, {}),
  )
    .map(([status, count]) => `${count} ${status}`)
    .join(" · ");
  if (toolCount > 0) {
    const labels = uniqueLabels(
      entries.flatMap((entry) =>
        entry.kind === "tool" && entry.value.name ? [entry.value.name] : [],
      ),
    );
    return {
      count: entries.length,
      toolCount,
      diagnosticCount: diagnostics.length,
      routineCount,
      labels,
      title: `${toolCount} ${toolCount === 1 ? "tool" : "tools"}`,
      detail: [formatLabelList(labels), diagnosticDetail].filter(Boolean).join(" · "),
      hasIssue,
    };
  }
  return {
    count: entries.length,
    toolCount,
    diagnosticCount: diagnostics.length,
    routineCount,
    labels: [],
    title: "Transcript activity",
    detail: diagnosticDetail,
    hasIssue,
  };
}

/**
 * Routine-only groups remain counted in Details but hidden in Conversation.
 */
export function conversationReadingTimeline(
  timeline: readonly SessionTimelineItem[],
): SessionTimelineItem[] {
  return timeline.filter((item) => {
    if (item.kind === "message") {
      if (item.value.role === "assistant" && !item.value.notice && !item.value.text.trim()) {
        return false;
      }
      return true;
    }
    const summary = activitySummary(item.value);
    return summary.toolCount > 0 || summary.diagnosticCount > 0;
  });
}

function uniqueLabels(values: string[]): string[] {
  return [...new Set(values)];
}

function formatLabelList(labels: string[]): string {
  if (labels.length === 0) return "";
  return `${labels.slice(0, 3).join(", ")}${labels.length > 3 ? ` +${labels.length - 3}` : ""}`;
}

const ROUTINE_UNSUPPORTED_PROJECTION =
  /^encountered \d+ unsupported Transcript Entry projection\(s\)$/;

/** Routine Codex projection notes are counts, not attention chrome. */
export function isRoutineProjectionWarning(warning: string): boolean {
  return ROUTINE_UNSUPPORTED_PROJECTION.test(warning);
}

export function transcriptAttentionWarnings(warnings: readonly string[]): string[] {
  return warnings.filter((warning) => !isRoutineProjectionWarning(warning));
}

/**
 * Report Transcript read failures here; tool errors belong to their activity
 * groups, and routine projection counts belong to Details.
 */
export function transcriptAttentionNotice(input: {
  partial: boolean;
  malformedCount: number;
  listWarnings: readonly string[];
}): string | null {
  if (input.partial) return "Transcript did not finish loading — content may be incomplete.";
  if (input.malformedCount > 0) {
    return `${input.malformedCount} malformed ${input.malformedCount === 1 ? "entry" : "entries"} could not be read.`;
  }
  return transcriptAttentionWarnings(input.listWarnings)[0] ?? null;
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
