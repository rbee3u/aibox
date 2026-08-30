import { describe, expect, it } from "vitest";
import type { ConversationMessage, SessionDetailStats, ToolActivity } from "@/api/sessions";
import {
  emptySessionDetail,
  sessionDetailReducer,
  type SessionDetailAction,
  type SessionDetailState,
} from "@/features/sessions/detail/sessionDetail";

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
