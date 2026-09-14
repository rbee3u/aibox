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
  | {
      kind: "tool";
      value: ToolActivity;
      /** The terminal record for this call, once it has arrived: what came back. */
      result?: ToolActivity;
    }
  | { kind: "evidence"; value: TranscriptEvidenceSummary };

/**
 * An item's identity is where it starts. A group that grows or a call that
 * gets its result keeps the same key, so a disclosure the reader opened stays
 * open across a refresh and fills in rather than remounting closed.
 */
export function sessionItemKey(item: SessionTimelineItem): string {
  if (item.kind === "message") return `message:${item.value.entry_ids[0]}`;
  const first = item.value[0];
  return `activity:${first.kind === "tool" ? first.value.entry_ids[0] : first.value.entry_id}`;
}

/**
 * Appends one activity record. A terminal Tool Activity that names an earlier
 * call updates that entry in place so a tool appears once with its final status;
 * anything else extends the trailing activity group or opens a new one.
 *
 * A call the Transcript never answered ends the stream as a terminal record
 * cloned from the call itself — same entries, same input. That is a status
 * without a result, so nothing is kept as one.
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

/** A CLI-written line in a speaker's slot: an event, not something anyone said. */
export function isConversationNotice(message: ConversationMessage): boolean {
  return message.notice !== undefined;
}

/**
 * Adjacent Agent messages merge only when no other record separates them. A
 * notice never merges: it is not the Agent's voice, and an API error folded
 * into the reply before it would read as the model's last sentence.
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
 * A tool the reader should look at: one that returned an error, or one whose
 * outcome the projection could not tell. A call with no result is neither —
 * it is still running, or the CLI stopped before it answered — and the
 * Transcript cannot say which, so it is stated, not flagged.
 */
export function toolNeedsAttention(status: ToolActivity["status"]): boolean {
  return status === "failed" || status === "unknown";
}

export function evidenceNeedsAttention(status: string): boolean {
  return status === "malformed";
}

/**
 * Housekeeping the CLI writes around every call — mode, permission, latch,
 * last prompt — and reasoning the reader hides on purpose. Neither is
 * something a reader of the Conversation is looking for.
 */
export function isRoutineEvidence(entry: SessionActivityItem): boolean {
  return (
    entry.kind === "evidence" &&
    (entry.value.status === "filtered" || entry.value.status === "hidden_internal")
  );
}

/** Summarizes one activity group for its collapsed disclosure. */
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
 * Groups holding nothing but routine evidence stay off the reading stream:
 * there is nothing in them a reader would open. Details still counts them.
 */
export function conversationReadingTimeline(
  timeline: readonly SessionTimelineItem[],
): SessionTimelineItem[] {
  return timeline.filter((item) => {
    if (item.kind === "message") return true;
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
 * One sentence naming why Conversation reading is impaired, or `null` when it
 * is not. A tool that returned an error is content, marked on its own activity
 * group, and never a Transcript problem; routine Codex projection notes stay
 * counts on Details.
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
