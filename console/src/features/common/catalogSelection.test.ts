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

  it("keeps keys selected on other pages when toggling one page", () => {
    const first = reduce({ type: "selection_toggle_all", keys: ["a1", "a2"], clear: false });
    const second = catalogSelectionReducer(first, {
      type: "selection_toggle_all",
      keys: ["b1"],
      clear: false,
    });
    const clearedFirstPage = catalogSelectionReducer(second, {
      type: "selection_toggle_all",
      keys: ["a1", "a2"],
      clear: true,
    });

    expect([...second.selectedKeys]).toEqual(["a1", "a2", "b1"]);
    expect([...clearedFirstPage.selectedKeys]).toEqual(["b1"]);
  });
});

/**
 * Per-row context, used by a catalog whose selection outlives the page it was
 * made on. Requests records the page each row was selected on so a delete can
 * return to the earliest page it touched.
 */
describe("catalog selection context", () => {
  type ContextState = CatalogSelectionState<string, number>;

  const withContext = (
    ...actions: Parameters<typeof catalogSelectionReducer<string, number>>[1][]
  ): ContextState =>
    actions.reduce<ContextState>(
      (state, action) => catalogSelectionReducer(state, action),
      initialCatalogSelection<string, number>(),
    );

  it("records the context a row was selected with and drops it on deselect", () => {
    const selected = withContext({ type: "selection_toggle", key: "one", context: 3 });
    const deselected = catalogSelectionReducer(selected, {
      type: "selection_toggle",
      key: "one",
      context: 3,
    });

    expect(selected.selectionContexts.get("one")).toBe(3);
    expect(deselected.selectionContexts.size).toBe(0);
  });

  it("keeps each page's own context when a later page is added", () => {
    const both = withContext(
      { type: "selection_toggle_all", keys: ["a"], clear: false, context: 1 },
      { type: "selection_toggle_all", keys: ["b"], clear: false, context: 2 },
    );

    expect(both.selectionContexts.get("a")).toBe(1);
    expect(both.selectionContexts.get("b")).toBe(2);
  });

  it("forgets contexts for keys a prune or cancel removed", () => {
    const selected = withContext(
      { type: "selection_toggle", key: "gone", context: 1 },
      { type: "selection_toggle", key: "kept", context: 2 },
    );
    const pruned = catalogSelectionReducer(selected, {
      type: "selection_prune",
      available: new Set(["kept"]),
    });
    const cancelled = catalogSelectionReducer(selected, { type: "selection_cancel" });

    expect([...pruned.selectionContexts.keys()]).toEqual(["kept"]);
    expect(cancelled.selectionContexts.size).toBe(0);
  });

  it("keeps only the surviving contexts when a partial mutation recovers", () => {
    const selected = withContext(
      { type: "selection_toggle", key: "gone", context: 1 },
      { type: "selection_toggle", key: "kept", context: 4 },
    );
    const recovered = catalogSelectionReducer(selected, {
      type: "selection_recovered",
      remaining: new Set(["kept"]),
      resume: true,
    });

    expect([...recovered.selectedKeys]).toEqual(["kept"]);
    expect(recovered.selectionContexts.get("kept")).toBe(4);
  });
});
