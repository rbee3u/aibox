import { describe, expect, it } from "vitest";
import type { ConversationMessage, SessionDetailStats, ToolActivity } from "./controlApi";
import {
  emptySessionDetail,
  sessionDetailReducer,
  type SessionDetailAction,
  type SessionDetailState,
} from "./SessionPage";

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
});
