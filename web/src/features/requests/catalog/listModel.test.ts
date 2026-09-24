import { describe, expect, it } from "vitest";
import type { RequestList, RequestState, RequestSummary } from "@/api/requests";
import {
  focusTargetAfterDelete,
  removeDeletedFromList,
} from "@/features/requests/catalog/listModel";

function row(id: string, state: RequestState = "completed"): RequestSummary {
  return {
    id,
    started_at: "2026-08-06T04:00:00Z",
    ended_at: state === "active" ? null : "2026-08-06T04:00:01Z",
    method: "POST",
    incoming_uri: "/v1/responses",
    upstream_url: "https://api.example.test/v1/responses",
    status: 200,
    http_version: "HTTP/1.1",
    outcome: "completed",
    state,
    total_ms: 10,
    protocol: null,
    assessment: { level: "ok", primary: null, issue_count: 0 },
  };
}

function list(rows: RequestSummary[], overrides: Partial<RequestList> = {}): RequestList {
  return {
    requests: rows,
    total: rows.length,
    deletable_count: rows.filter((request) => request.state !== "active").length,
    has_next: false,
    ...overrides,
  };
}

describe("focus target after a Request deletion", () => {
  it("prefers the row after the deleted one", () => {
    const before = [row("a"), row("b"), row("c")];
    const after = [row("a"), row("c")];
    expect(focusTargetAfterDelete(before, "b", after, false)).toBe("c");
  });

  it("falls back to the preceding row when the deleted row was last", () => {
    const before = [row("a"), row("b")];
    expect(focusTargetAfterDelete(before, "b", [row("a")], false)).toBe("a");
  });

  it("skips active rows that cannot be deleted", () => {
    const before = [row("a"), row("b"), row("c", "active"), row("d")];
    const after = [row("a"), row("c", "active"), row("d")];
    expect(focusTargetAfterDelete(before, "b", after, false)).toBe("d");
  });

  it("focuses the last remaining row after falling back a page", () => {
    const after = [row("x"), row("y")];
    expect(focusTargetAfterDelete([row("z")], "z", after, true)).toBe("y");
  });

  it("reports no target when nothing deletable remains", () => {
    expect(focusTargetAfterDelete([row("a")], "a", [], false)).toBeNull();
    expect(focusTargetAfterDelete([row("a")], "a", [row("b", "active")], false)).toBeNull();
  });
});

describe("applying a completed deletion to the visible page", () => {
  it("removes the rows and lowers both counters", () => {
    const current = list([row("a"), row("b"), row("c")], { total: 3, deletable_count: 3 });
    expect(removeDeletedFromList(current, ["a", "c"], 2, 1)).toMatchObject({
      requests: [row("b")],
      total: 1,
      deletable_count: 1,
      has_next: false,
    });
  });

  it("never lowers a counter below zero", () => {
    const current = list([row("a")], { total: 1, deletable_count: 1 });
    expect(removeDeletedFromList(current, ["a"], 5, 1)).toMatchObject({
      total: 0,
      deletable_count: 0,
    });
  });

  it("keeps a later page reachable while the remaining total spans it", () => {
    const current = list([row("a")], { total: 120, deletable_count: 120, has_next: true });
    expect(removeDeletedFromList(current, ["a"], 1, 1).has_next).toBe(true);
    expect(removeDeletedFromList(current, ["a"], 70, 1).has_next).toBe(false);
  });
});
