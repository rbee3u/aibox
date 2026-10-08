/**
 * Shared selection state; features own dialogs and mutations.
 * `Context` captures optional per-row data at selection time, such as the
 * Request's page for post-deletion navigation. Use `never` when unnecessary.
 */

export interface CatalogSelectionState<Key extends string, Context = never> {
  selectedKeys: Set<Key>;
  selectionMode: boolean;
  /** Per-row context captured at selection time; empty when unused. */
  selectionContexts: ReadonlyMap<Key, Context>;
}

export type CatalogSelectionAction<Key extends string, Context = never> =
  /** Reveal the per-row selection controls without selecting anything. */
  | { type: "selection_enter" }
  /** Leave selection mode and discard the selection. */
  | { type: "selection_cancel" }
  | { type: "selection_toggle"; key: Key; context?: Context }
  /** Change only the listed keys, preserving selections on other pages. */
  | { type: "selection_toggle_all"; keys: readonly Key[]; clear: boolean; context?: Context }
  /** Drop keys a refreshed catalog no longer lists. */
  | { type: "selection_prune"; available: ReadonlySet<Key> }
  /** Resume selection only when requested and unprocessed keys remain. */
  | { type: "selection_recovered"; remaining: ReadonlySet<Key>; resume: boolean };

export function initialCatalogSelection<
  Key extends string,
  Context = never,
>(): CatalogSelectionState<Key, Context> {
  return { selectedKeys: new Set(), selectionMode: false, selectionContexts: new Map() };
}

function retainContexts<Key extends string, Context>(
  contexts: ReadonlyMap<Key, Context>,
  selectedKeys: ReadonlySet<Key>,
): ReadonlyMap<Key, Context> {
  if (contexts.size === 0) return contexts;
  return new Map([...contexts].filter(([key]) => selectedKeys.has(key)));
}

export function catalogSelectionReducer<Key extends string, Context = never>(
  state: CatalogSelectionState<Key, Context>,
  action: CatalogSelectionAction<Key, Context>,
): CatalogSelectionState<Key, Context> {
  switch (action.type) {
    case "selection_enter":
      return { ...state, selectionMode: true };
    case "selection_cancel":
      return initialCatalogSelection<Key, Context>();
    case "selection_toggle": {
      const selectedKeys = new Set(state.selectedKeys);
      const selectionContexts = new Map(state.selectionContexts);
      if (selectedKeys.delete(action.key)) {
        selectionContexts.delete(action.key);
      } else {
        selectedKeys.add(action.key);
        if (action.context !== undefined) selectionContexts.set(action.key, action.context);
      }
      return { ...state, selectedKeys, selectionContexts };
    }
    case "selection_toggle_all": {
      const selectedKeys = new Set(state.selectedKeys);
      const selectionContexts = new Map(state.selectionContexts);
      for (const key of action.keys) {
        if (action.clear) {
          selectedKeys.delete(key);
          selectionContexts.delete(key);
        } else if (!selectedKeys.has(key)) {
          selectedKeys.add(key);
          if (action.context !== undefined) selectionContexts.set(key, action.context);
        }
      }
      return { ...state, selectedKeys, selectionContexts };
    }
    case "selection_prune": {
      const selectedKeys = new Set(
        [...state.selectedKeys].filter((key) => action.available.has(key)),
      );
      return {
        ...state,
        selectedKeys,
        selectionContexts: retainContexts(state.selectionContexts, selectedKeys),
      };
    }
    case "selection_recovered": {
      const resumed = action.resume && action.remaining.size > 0;
      const selectedKeys: Set<Key> = action.resume ? new Set(action.remaining) : new Set();
      return {
        selectedKeys,
        selectionMode: resumed,
        selectionContexts: retainContexts(state.selectionContexts, selectedKeys),
      };
    }
  }
}

export function allSelected<Key extends string>(
  selectable: readonly Key[],
  selected: ReadonlySet<Key>,
): boolean {
  return selectable.length > 0 && selectable.every((key) => selected.has(key));
}
