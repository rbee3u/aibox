/**
 * The batch-selection state machine every catalog page shares.
 *
 * Tenants, Configs, Sessions, and Requests all let a user enter selection
 * mode, toggle rows, select or clear a whole page, drop rows a refresh removed,
 * and resume a selection after a partial delete. Those six transitions were
 * implemented four separate times with different field and action names, so a
 * fix to one did not reach the others. Requests kept a fifth implementation as
 * a hook for longer than the rest, because it alone needed per-row context;
 * that need is now `Context` below.
 *
 * A feature composes this by spreading `CatalogSelectionState` into its own
 * state and delegating the `selection_*` actions, keeping its mutation or
 * dialog state to itself.
 *
 * `Context` is optional per-row data recorded when a row is selected, for
 * features whose selection outlives the view it was made in: Requests paginates,
 * so it records which page each Request was selected on and returns to the
 * earliest of them after a delete. Features that need none leave it `never`.
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
  /**
   * Add every listed key, or remove them all when `clear` is set.
   *
   * Adds and removes rather than replacing the whole selection, so a paginated
   * catalog keeps rows selected on other pages. For a catalog that lists every
   * selectable key at once the two are equivalent.
   */
  | { type: "selection_toggle_all"; keys: readonly Key[]; clear: boolean; context?: Context }
  /** Drop keys a refreshed catalog no longer lists. */
  | { type: "selection_prune"; available: ReadonlySet<Key> }
  /**
   * Restore what a failed batch mutation left behind. Selection mode persists
   * only when something remains and the caller asks to resume, so a fully
   * successful delete exits instead of leaving an empty selection bar.
   */
  | { type: "selection_recovered"; remaining: ReadonlySet<Key>; resume: boolean };

export function initialCatalogSelection<
  Key extends string,
  Context = never,
>(): CatalogSelectionState<Key, Context> {
  return { selectedKeys: new Set(), selectionMode: false, selectionContexts: new Map() };
}

/** Keep only the contexts whose key is still selected. */
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
      return { selectedKeys: new Set(), selectionMode: false, selectionContexts: new Map() };
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

/** True when every selectable key is already selected. */
export function allSelected<Key extends string>(
  selectable: readonly Key[],
  selected: ReadonlySet<Key>,
): boolean {
  return selectable.length > 0 && selectable.every((key) => selected.has(key));
}
