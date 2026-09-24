import { describe, expect, it } from "vitest";
import { configWorkflowReducer, initialConfigWorkflow } from "@/features/configs/configWorkflow";

// Selection transitions are covered once in features/common/catalogSelection.test.ts.
describe("Config workflow reducer", () => {
  it("returns mutation state to idle explicitly", () => {
    const busy = configWorkflowReducer(initialConfigWorkflow, {
      type: "mutation_changed",
      busy: true,
    });

    expect(busy.mutationBusy).toBe(true);
    expect(
      configWorkflowReducer(busy, { type: "mutation_changed", busy: false }).mutationBusy,
    ).toBe(false);
  });

  it("keeps a mutation in flight while the selection changes", () => {
    const busy = configWorkflowReducer(initialConfigWorkflow, {
      type: "mutation_changed",
      busy: true,
    });
    const toggled = configWorkflowReducer(busy, { type: "selection_toggle", key: "one" });

    expect(toggled.mutationBusy).toBe(true);
    expect([...toggled.selectedKeys]).toEqual(["one"]);
  });
});
