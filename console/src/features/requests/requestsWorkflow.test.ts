import { describe, expect, it } from "vitest";

import {
  earliestSelectedPage,
  initialRequestsWorkflow,
  requestsWorkflowReducer,
  type RequestsWorkflowAction,
  type RequestsWorkflowState,
} from "@/features/requests/requestsWorkflow";

function reduce(...actions: RequestsWorkflowAction[]): RequestsWorkflowState {
  return actions.reduce<RequestsWorkflowState>(
    (state, action) => requestsWorkflowReducer(state, action),
    initialRequestsWorkflow,
  );
}

describe("requests workflow", () => {
  it("opens and dismisses a batch dialog without touching the selection", () => {
    const opened = reduce(
      { type: "selection_toggle", key: "one", context: 2 },
      { type: "dialog_opened", dialog: { kind: "batch", ids: ["one"] } },
    );
    const dismissed = requestsWorkflowReducer(opened, { type: "dialog_dismissed" });

    expect(opened.dialog).toEqual({ kind: "batch", ids: ["one"] });
    expect(dismissed.dialog).toBeNull();
    expect([...dismissed.selectedKeys]).toEqual(["one"]);
  });

  it("tracks the delete in flight independently of the dialog", () => {
    const deleting = reduce(
      { type: "dialog_opened", dialog: { kind: "request", id: "one" } },
      { type: "delete_started", deletion: { kind: "request", id: "one" } },
    );
    const finished = requestsWorkflowReducer(deleting, { type: "delete_finished" });

    expect(deleting.deletion).toEqual({ kind: "request", id: "one" });
    expect(finished.deletion).toBeNull();
    expect(finished.dialog).toEqual({ kind: "request", id: "one" });
  });

  it("returns to the earliest page a batch was selected across", () => {
    const spanning = reduce(
      { type: "selection_toggle", key: "late", context: 7 },
      { type: "selection_toggle", key: "early", context: 3 },
    );

    expect(earliestSelectedPage(spanning, ["late", "early"], 9)).toBe(3);
  });

  it("falls back to the current page for a row with no recorded context", () => {
    expect(earliestSelectedPage(initialRequestsWorkflow, ["unknown"], 5)).toBe(5);
  });

  it("cancelling a selection clears its rows, contexts, and selection mode", () => {
    const cancelled = reduce(
      { type: "selection_enter" },
      { type: "selection_toggle", key: "one", context: 1 },
      { type: "selection_cancel" },
    );

    expect(cancelled.selectionMode).toBe(false);
    expect([...cancelled.selectedKeys]).toEqual([]);
    expect(cancelled.selectionContexts.size).toBe(0);
  });
});
