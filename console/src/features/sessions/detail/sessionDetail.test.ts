import { describe, expect, it } from "vitest";
import type { ConversationMessage, SessionDetailStats, ToolActivity } from "@/api/sessions";
import {
  activitySummary,
  appendActivityItem,
  conversationReadingTimeline,
  emptySessionDetail,
  isRoutineProjectionWarning,
  sessionDetailReducer,
  transcriptAttentionWarnings,
  transcriptNeedsAttention,
  type SessionActivityItem,
  type SessionDetailAction,
  type SessionDetailState,
} from "@/features/sessions/detail/sessionDetail";
import { toolActivityHeadline } from "@/features/sessions/detail/sessionFormat";

const message: ConversationMessage = {
  entry_ids: ["message-1"],
  role: "user",
  timestamp: "2026-08-23T01:00:00Z",
  text: "Inspect the Console",
};
const tool: ToolActivity = {
  entry_ids: ["tool-1"],
  call_id: "call-1",
  timestamp: "2026-08-23T01:00:01Z",
  name: "Read",
  status: "completed",
  summary: "Read Console source",
};
const stats: SessionDetailStats = {
  start_ts: message.timestamp,
  last_event_ts: tool.timestamp,
  message_count: 1,
  tool_count: 1,
  entry_count: 2,
  malformed_count: 0,
  unsupported_count: 0,
  hidden_internal_count: 0,
  observed_duration_ms: 1000,
  file_size: 128,
  snapshot: "128:1",
};

function evidence(status: string, nativeType = "response_item"): SessionActivityItem {
  return {
    kind: "evidence",
    value: {
      entry_id: `evidence-${status}`,
      line: 1,
      timestamp: message.timestamp,
      native_type: nativeType,
      role: null,
      content_types: [],
      status,
      preview: status,
    },
  };
}

describe("Conversation attention", () => {
  it("treats only malformed evidence and failed tools as issues", () => {
    expect(
      activitySummary([evidence("filtered"), evidence("unsupported"), evidence("hidden_internal")])
        .hasIssue,
    ).toBe(false);
    expect(activitySummary([evidence("malformed")]).hasIssue).toBe(true);
    expect(activitySummary([{ kind: "tool", value: { ...tool, status: "failed" } }]).hasIssue).toBe(
      true,
    );
    expect(activitySummary([{ kind: "tool", value: tool }]).hasIssue).toBe(false);
  });

  it("labels tool-bearing groups as tools and keeps evidence-only groups as quiet Transcript activity", () => {
    expect(
      activitySummary([evidence("filtered"), evidence("unsupported", "world_state")]),
    ).toMatchObject({
      title: "Transcript activity",
      detail: "2 items",
      labels: [],
    });
    expect(
      activitySummary([
        { kind: "tool", value: { ...tool, name: "exec", summary: '{"cmd":"git status"}' } },
        evidence("filtered"),
        evidence("hidden_internal"),
      ]),
    ).toMatchObject({
      title: "1 tool",
      detail: "exec · 2 events",
    });
    expect(activitySummary([{ kind: "tool", value: { ...tool, name: "exec" } }])).toMatchObject({
      title: "1 tool",
      detail: "exec",
    });
  });

  it("keeps the started Tool Activity summary when the result arrives", () => {
    const started: SessionActivityItem = {
      kind: "tool",
      value: { ...tool, status: "started", summary: '{"cmd":"git status"}' },
    };
    const completed: SessionActivityItem = {
      kind: "tool",
      value: {
        ...tool,
        entry_ids: ["tool-1-done"],
        status: "completed",
        summary: "Script completed",
      },
    };
    const timeline = appendActivityItem(appendActivityItem([], started), completed);
    expect(timeline[0]).toMatchObject({
      kind: "activity",
      value: [{ kind: "tool", value: { status: "completed", summary: '{"cmd":"git status"}' } }],
    });
  });

  it("promotes the first readable tool input onto the collapsed row", () => {
    expect(toolActivityHeadline('{"cmd":"git status --porcelain"}')).toBe("git status --porcelain");
    expect(toolActivityHeadline('"ls -la src"')).toBe("ls -la src");
    expect(toolActivityHeadline("Read Console source")).toBe("Read Console source");
    expect(toolActivityHeadline("")).toBeNull();
  });

  it("keeps unsupported projection notes out of attention chrome", () => {
    expect(
      isRoutineProjectionWarning("encountered 1 unsupported Transcript Entry projection(s)"),
    ).toBe(true);
    expect(
      transcriptAttentionWarnings([
        "encountered 2 unsupported Transcript Entry projection(s)",
        "line 2: malformed JSONL (invalid)",
        "skipped 1 malformed JSONL record(s)",
      ]),
    ).toEqual(["line 2: malformed JSONL (invalid)", "skipped 1 malformed JSONL record(s)"]);
  });

  it("keeps leading evidence-only groups off the Conversation reading stream", () => {
    const leading = {
      kind: "activity" as const,
      value: [evidence("filtered"), evidence("unsupported")],
    };
    const tools = {
      kind: "activity" as const,
      value: [{ kind: "tool" as const, value: tool }],
    };
    const trailing = { kind: "activity" as const, value: [evidence("hidden_internal")] };
    expect(
      conversationReadingTimeline([leading, { kind: "message", value: message }, trailing]),
    ).toEqual([{ kind: "message", value: message }, trailing]);
    expect(conversationReadingTimeline([leading])).toEqual([]);
    expect(conversationReadingTimeline([tools, { kind: "message", value: message }])).toEqual([
      tools,
      { kind: "message", value: message },
    ]);
  });

  it("does not alarm a complete Transcript with routine unsupported projections", () => {
    expect(
      transcriptNeedsAttention({
        partial: false,
        malformedCount: 0,
        listWarningCount: 0,
        timeline: [{ kind: "activity", value: [evidence("unsupported"), evidence("filtered")] }],
      }),
    ).toBe(false);
  });

  it("alarms when reading is impaired", () => {
    expect(
      transcriptNeedsAttention({
        partial: true,
        malformedCount: 0,
        listWarningCount: 0,
        timeline: [],
      }),
    ).toBe(true);
    expect(
      transcriptNeedsAttention({
        partial: false,
        malformedCount: 1,
        listWarningCount: 0,
        timeline: [],
      }),
    ).toBe(true);
    expect(
      transcriptNeedsAttention({
        partial: false,
        malformedCount: 0,
        listWarningCount: 1,
        timeline: [],
      }),
    ).toBe(true);
    expect(
      transcriptNeedsAttention({
        partial: false,
        malformedCount: 0,
        listWarningCount: 0,
        timeline: [
          { kind: "activity", value: [{ kind: "tool", value: { ...tool, status: "incomplete" } }] },
        ],
      }),
    ).toBe(true);
  });
});

describe("Session detail reducer", () => {
  it.each([
    [false, 0],
    [true, 1],
  ])("starts loading with preserveContent=%s", (preserveContent, expectedItems) => {
    const populated = sessionDetailReducer(emptySessionDetail, { type: "message", value: message });
    const state = sessionDetailReducer(populated, { type: "start", preserveContent });

    expect(state.loading).toBe(true);
    expect(state.timeline).toHaveLength(expectedItems);
  });

  it("aggregates streamed messages, activity, completion facts, and warnings", () => {
    const actions: SessionDetailAction[] = [
      { type: "start", preserveContent: false },
      { type: "message", value: message },
      { type: "activity", value: { kind: "tool", value: tool } },
      { type: "complete", stats, warnings: ["partial Transcript"] },
      { type: "stop" },
    ];
    const state = actions.reduce<SessionDetailState>(sessionDetailReducer, emptySessionDetail);

    expect(state.timeline.map((item) => item.kind)).toEqual(["message", "activity"]);
    expect(state.stats).toBe(stats);
    expect(state.warnings).toEqual(["partial Transcript"]);
    expect(state.loading).toBe(false);
  });

  it("atomically replaces content after a background refresh", () => {
    const loading = sessionDetailReducer(emptySessionDetail, {
      type: "start",
      preserveContent: true,
    });
    const state = sessionDetailReducer(loading, {
      type: "replace",
      timeline: [{ kind: "message", value: message }],
      meta: null,
      stats,
      warnings: [],
    });

    expect(state.timeline).toEqual([{ kind: "message", value: message }]);
    expect(state.stats).toBe(stats);
    expect(state.loading).toBe(true);
  });

  it("preserves order, merging, and final state across 5,000 mixed Transcript frames", () => {
    const actions: SessionDetailAction[] = [];
    for (let index = 0; index < 1_000; index += 1) {
      const callId = `call-${index}`;
      actions.push(
        {
          type: "message",
          value: { ...message, entry_ids: [`user-${index}`], text: `User ${index}` },
        },
        {
          type: "message",
          value: {
            ...message,
            entry_ids: [`assistant-${index}-a`],
            role: "assistant",
            text: `Assistant ${index} A`,
          },
        },
        {
          type: "message",
          value: {
            ...message,
            entry_ids: [`assistant-${index}-b`],
            role: "assistant",
            text: `Assistant ${index} B`,
          },
        },
        {
          type: "activity",
          value: {
            kind: "tool",
            value: {
              ...tool,
              entry_ids: [`tool-${index}-start`],
              call_id: callId,
              status: "started",
              summary: "",
            },
          },
        },
        {
          type: "activity",
          value: {
            kind: "tool",
            value: {
              ...tool,
              entry_ids: [`tool-${index}-complete`],
              call_id: callId,
              summary: `Completed ${index}`,
            },
          },
        },
      );
    }

    const streamed = actions.reduce<SessionDetailState>(sessionDetailReducer, emptySessionDetail);
    const state = sessionDetailReducer(streamed, {
      type: "complete",
      stats: { ...stats, entry_count: actions.length },
      warnings: ["final warning"],
    });

    expect(actions).toHaveLength(5_000);
    expect(state.timeline).toHaveLength(3_000);
    const [first, second, third] = state.timeline;
    expect(first?.kind).toBe("message");
    if (first?.kind === "message") expect(first.value.text).toBe("User 0");
    expect(second?.kind).toBe("message");
    if (second?.kind === "message") {
      expect(second.value.entry_ids).toEqual(["assistant-0-a", "assistant-0-b"]);
      expect(second.value.text).toBe("Assistant 0 A\n\nAssistant 0 B");
    }
    expect(third?.kind).toBe("activity");
    if (third?.kind === "activity") {
      const firstActivity = third.value[0];
      expect(firstActivity?.kind).toBe("tool");
      if (firstActivity?.kind === "tool") {
        expect(firstActivity.value.entry_ids).toEqual(["tool-0-start", "tool-0-complete"]);
        expect(firstActivity.value.status).toBe("completed");
        expect(firstActivity.value.summary).toBe("Completed 0");
      }
    }
    const last = state.timeline.at(-1);
    expect(last?.kind).toBe("activity");
    if (last?.kind === "activity") {
      const lastActivity = last.value[0];
      expect(lastActivity?.kind).toBe("tool");
      if (lastActivity?.kind === "tool") {
        expect(lastActivity.value.call_id).toBe("call-999");
        expect(lastActivity.value.status).toBe("completed");
      }
    }
    expect(state.stats?.entry_count).toBe(5_000);
    expect(state.warnings).toEqual(["final warning"]);
  });
});
