import { describe, expect, it } from "vitest";
import type { Operation } from "@/api/operations";
import { mergeOperation } from "@/app/useOperationFeed";

function operation(
  id: string,
  logs: Array<{ sequence: number; message: string }>,
  firstSequence = logs[0]?.sequence ?? 0,
): Operation {
  return {
    id,
    kind: "install codex",
    state: "running",
    started_at: "2026-08-27T00:00:00Z",
    ended_at: null,
    result: null,
    first_sequence: firstSequence,
    next_sequence: Math.max(firstSequence, ...logs.map((entry) => entry.sequence + 1)),
    logs,
  };
}

describe("mergeOperation", () => {
  it("deduplicates reconnect frames and keeps sequence order", () => {
    const current = operation("one", [
      { sequence: 2, message: "two" },
      { sequence: 3, message: "old three" },
    ]);
    const incoming = operation(
      "one",
      [
        { sequence: 3, message: "new three" },
        { sequence: 4, message: "four" },
      ],
      2,
    );

    expect(mergeOperation(current, incoming, false)?.logs).toEqual([
      { sequence: 2, message: "two" },
      { sequence: 3, message: "new three" },
      { sequence: 4, message: "four" },
    ]);
  });

  it("drops logs before the server retention window", () => {
    const current = operation("one", [
      { sequence: 1, message: "one" },
      { sequence: 2, message: "two" },
    ]);
    const incoming = operation("one", [{ sequence: 4, message: "four" }], 4);

    expect(mergeOperation(current, incoming, false)?.logs).toEqual([
      { sequence: 4, message: "four" },
    ]);
  });

  it("replaces state after a reported gap or operation change", () => {
    const current = operation("one", [{ sequence: 1, message: "one" }]);
    const gap = operation("one", [{ sequence: 5, message: "five" }], 5);
    const changed = operation("two", [{ sequence: 0, message: "new" }]);

    expect(mergeOperation(current, gap, true)).toBe(gap);
    expect(mergeOperation(current, changed, false)).toBe(changed);
  });

  it("clears the current operation when the Service reports none", () => {
    expect(mergeOperation(operation("one", []), null, false)).toBeNull();
  });
});
