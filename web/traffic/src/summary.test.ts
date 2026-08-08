import { describe, expect, it } from "vitest";
import { completedDetail } from "./test/fixtures";
import { elapsedNsMs, resolveRequestedEffective, timingStages, tokenCount } from "./summary";

describe("Summary presentation helpers", () => {
  it("prefers effective values and reports requested fallbacks", () => {
    expect(resolveRequestedEffective({ requested: "low", effective: "high" })).toEqual({
      value: "high",
      source: "effective",
    });
    expect(resolveRequestedEffective({ requested: "medium", effective: null })).toEqual({
      value: "medium",
      source: "requested",
    });
    expect(resolveRequestedEffective(undefined)).toEqual({ value: null, source: null });
  });

  it("builds the six streaming Timing Stages on one axis", () => {
    expect(timingStages(completedDetail)).toEqual([
      expect.objectContaining({ label: "Proxy setup", durationMs: 100, status: "complete" }),
      expect.objectContaining({ label: "Request upload", durationMs: 100, status: "complete" }),
      expect.objectContaining({ label: "Response wait", durationMs: 300, status: "complete" }),
      expect.objectContaining({ label: "First-token wait", durationMs: 400, status: "complete" }),
      expect.objectContaining({ label: "Model output", durationMs: 330, status: "complete" }),
      expect.objectContaining({ label: "Finalization", durationMs: 20, status: "complete" }),
    ]);
  });

  it("keeps First-token wait live until an output-bearing event arrives", () => {
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

  it("uses a single Response body stage when First Token is not observable", () => {
    const detail = {
      ...completedDetail,
      summary: {
        ...completedDetail.summary,
        protocol: {
          ...completedDetail.summary.protocol!,
          response_mode: { requested: "normal" as const, observed: "normal" as const },
          first_token_at_ns: null,
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
  });

  it("formats nanosecond offsets and token counters without losing zero", () => {
    expect(elapsedNsMs("77581959")).toBeCloseTo(77.581959);
    expect(elapsedNsMs("invalid")).toBeNull();
    expect(tokenCount(0)).toBe("0");
    expect(tokenCount(156770)).toBe("156,770");
  });
});
