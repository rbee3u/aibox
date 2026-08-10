import { describe, expect, it } from "vitest";
import { completedDetail } from "./test/fixtures";
import { elapsedNsMs, resolveRequestedEffective, timingStages, tokenCount } from "./summary";
import type { ProtocolSummary } from "./types";

describe("Summary presentation helpers", () => {
  it("prefers effective values and falls back to requested values", () => {
    expect(resolveRequestedEffective({ requested: "low", effective: "high" })).toBe("high");
    expect(resolveRequestedEffective({ requested: "medium", effective: null })).toBe("medium");
    expect(resolveRequestedEffective(undefined)).toBeNull();
  });

  it("builds the six streaming Timing Stages on one axis", () => {
    expect(timingStages(completedDetail)).toEqual([
      expect.objectContaining({ label: "Proxy setup", durationMs: 100, status: "complete" }),
      expect.objectContaining({ label: "Request upload", durationMs: 100, status: "complete" }),
      expect.objectContaining({ label: "Response wait", durationMs: 300, status: "complete" }),
      expect.objectContaining({ label: "First-token wait", durationMs: 400, status: "complete" }),
      expect.objectContaining({ label: "Response stream", durationMs: 330, status: "complete" }),
      expect.objectContaining({ label: "Finalization", durationMs: 20, status: "complete" }),
    ]);
  });

  it("keeps First-token wait live until an eligible SSE data line arrives", () => {
    const detail = {
      ...completedDetail,
      state: "active" as const,
      result: null,
      timeline_end_at_ns: "800000000",
      summary: {
        ...completedDetail.summary,
        terminal: false,
        outcome: null,
        timing: {
          ...completedDetail.summary.timing,
          upstream_response_headers_at_ns: "500000000",
          upstream_response_body_completed_at_ns: null,
          finished_at_ns: null,
        },
        protocol: {
          ...completedDetail.summary.protocol!,
          response_terminal: false,
          first_token_at_ns: null,
          token_usage: null,
        },
      },
    };
    expect(timingStages(detail).at(-1)).toEqual(
      expect.objectContaining({ label: "First-token wait", durationMs: 300, status: "ongoing" }),
    );
  });

  it.each([
    {
      mode: "a terminal stream without First Token",
      protocol: { first_token_at_ns: null },
    },
    {
      mode: "a non-streaming response",
      protocol: {
        response_mode: { requested: "normal", observed: "normal" },
        first_token_at_ns: null,
      },
    },
  ] satisfies Array<{ mode: string; protocol: Partial<ProtocolSummary> }>)(
    "uses a single Response body stage for $mode",
    ({ protocol }) => {
      const detail = {
        ...completedDetail,
        summary: {
          ...completedDetail.summary,
          protocol: {
            ...completedDetail.summary.protocol!,
            ...protocol,
          },
        },
      };
      const stages = timingStages(detail);
      expect(stages.map((stage) => stage.label)).toEqual([
        "Proxy setup",
        "Request upload",
        "Response wait",
        "Response body",
        "Finalization",
      ]);
      expect(stages[3].durationMs).toBe(730);
    },
  );

  it("moves to active finalization when a stream completes without a First Token", () => {
    const detail = {
      ...completedDetail,
      state: "active" as const,
      result: null,
      timeline_end_at_ns: "1240000000",
      summary: {
        ...completedDetail.summary,
        terminal: false,
        outcome: null,
        timing: {
          ...completedDetail.summary.timing,
          finished_at_ns: null,
        },
        protocol: {
          ...completedDetail.summary.protocol!,
          first_token_at_ns: null,
          token_usage: null,
        },
      },
    };

    const stages = timingStages(detail);
    expect(stages.map((stage) => stage.label)).toEqual([
      "Proxy setup",
      "Request upload",
      "Response wait",
      "Response body",
      "Finalization",
    ]);
    expect(stages.at(-1)).toEqual(
      expect.objectContaining({ label: "Finalization", status: "ongoing", durationMs: 10 }),
    );
  });

  it("formats nanosecond offsets and token counters without losing zero", () => {
    expect(elapsedNsMs("77581959")).toBeCloseTo(77.581959);
    expect(elapsedNsMs("invalid")).toBeNull();
    expect(tokenCount(0)).toBe("0");
    expect(tokenCount(156770)).toBe("156,770");
  });
});
