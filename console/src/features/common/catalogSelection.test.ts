import { describe, expect, it } from "vitest";

import {
  allSelected,
  catalogSelectionReducer,
  initialCatalogSelection,
  type CatalogSelectionState,
} from "@/features/common/catalogSelection";

type State = CatalogSelectionState<string>;

const initial = (): State => initialCatalogSelection<string>();

/** Apply a sequence of actions from the initial state. */
function reduce(...actions: Parameters<typeof catalogSelectionReducer<string>>[1][]): State {
  return actions.reduce<State>(
    (state, action) => catalogSelectionReducer(state, action),
    initial(),
  );
}

describe("catalog selection", () => {
  it("enters selection mode without selecting anything", () => {
    const entered = reduce({ type: "selection_enter" });

    expect(entered.selectionMode).toBe(true);
    expect([...entered.selectedKeys]).toEqual([]);
  });

  it("toggles a key on and back off", () => {
    const added = reduce({ type: "selection_enter" }, { type: "selection_toggle", key: "one" });
    const removed = catalogSelectionReducer(added, { type: "selection_toggle", key: "one" });

    expect([...added.selectedKeys]).toEqual(["one"]);
    expect([...removed.selectedKeys]).toEqual([]);
    expect(removed.selectionMode).toBe(true);
  });

  it("selects every listed key, then clears them all", () => {
    const all = reduce({
      type: "selection_toggle_all",
      keys: ["one", "two"],
      clear: false,
    });
    const cleared = catalogSelectionReducer(all, {
      type: "selection_toggle_all",
      keys: ["one", "two"],
      clear: true,
    });

    expect([...all.selectedKeys]).toEqual(["one", "two"]);
    expect([...cleared.selectedKeys]).toEqual([]);
  });

  it("drops keys a refreshed catalog no longer lists", () => {
    const pruned = reduce(
      { type: "selection_toggle_all", keys: ["gone", "kept"], clear: false },
      { type: "selection_prune", available: new Set(["kept"]) },
    );

    expect([...pruned.selectedKeys]).toEqual(["kept"]);
  });

  it("cancelling discards the selection and leaves selection mode", () => {
    const cancelled = reduce(
      { type: "selection_enter" },
      { type: "selection_toggle", key: "one" },
      { type: "selection_cancel" },
    );

    expect(cancelled).toEqual(initial());
  });

  it("resumes a selection that survived a partial mutation", () => {
    const recovered = reduce({
      type: "selection_recovered",
      remaining: new Set(["survivor"]),
      resume: true,
    });

    expect(recovered.selectionMode).toBe(true);
    expect([...recovered.selectedKeys]).toEqual(["survivor"]);
  });

  it("leaves selection mode when nothing survived, so no empty selection bar remains", () => {
    const recovered = reduce({
      type: "selection_recovered",
      remaining: new Set(),
      resume: true,
    });

    expect(recovered).toEqual(initial());
  });

  it("discards the remainder when the caller does not resume", () => {
    const recovered = reduce({
      type: "selection_recovered",
      remaining: new Set(["ignored"]),
      resume: false,
    });

    expect(recovered).toEqual(initial());
  });

  it("reports all-selected only for a nonempty fully selected list", () => {
    const selected = new Set(["one", "two"]);

    expect(allSelected(["one", "two"], selected)).toBe(true);
    expect(allSelected(["one", "three"], selected)).toBe(false);
    expect(allSelected([], selected)).toBe(false);
  });
});
