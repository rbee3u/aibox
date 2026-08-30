/**
 * The batch-selection state machine every catalog page shares.
 *
 * Tenants, Configs, Sessions, and Requests all let a user enter selection
 * mode, toggle rows, select or clear a whole page, drop rows a refresh removed,
 * and resume a selection after a partial delete. Those six transitions were
 * implemented four separate times with different field and action names, so a
 * fix to one did not reach the others.
 *
 * A feature composes this by spreading `CatalogSelectionState` into its own
 * state and delegating the `selection_*` actions, keeping its mutation or
 * dialog state to itself.
 */

export interface CatalogSelectionState<Key extends string> {
  selectedKeys: Set<Key>;
  selectionMode: boolean;
}

export type CatalogSelectionAction<Key extends string> =
  /** Reveal the per-row selection controls without selecting anything. */
  | { type: "selection_enter" }
  /** Leave selection mode and discard the selection. */
  | { type: "selection_cancel" }
  | { type: "selection_toggle"; key: Key }
  /** Select every listed key, or clear them all when `clear` is set. */
  | { type: "selection_toggle_all"; keys: readonly Key[]; clear: boolean }
  /** Drop keys a refreshed catalog no longer lists. */
  | { type: "selection_prune"; available: ReadonlySet<Key> }
  /**
   * Restore what a failed batch mutation left behind. Selection mode persists
   * only when something remains and the caller asks to resume, so a fully
   * successful delete exits instead of leaving an empty selection bar.
   */
  | { type: "selection_recovered"; remaining: ReadonlySet<Key>; resume: boolean };

export function initialCatalogSelection<Key extends string>(): CatalogSelectionState<Key> {
  return { selectedKeys: new Set(), selectionMode: false };
}

export function catalogSelectionReducer<Key extends string>(
  state: CatalogSelectionState<Key>,
  action: CatalogSelectionAction<Key>,
): CatalogSelectionState<Key> {
  switch (action.type) {
    case "selection_enter":
      return { ...state, selectionMode: true };
    case "selection_cancel":
      return { selectedKeys: new Set(), selectionMode: false };
    case "selection_toggle": {
      const selectedKeys = new Set(state.selectedKeys);
      if (!selectedKeys.delete(action.key)) selectedKeys.add(action.key);
      return { ...state, selectedKeys };
    }
    case "selection_toggle_all":
      return {
        ...state,
        selectedKeys: action.clear ? new Set() : new Set(action.keys),
      };
    case "selection_prune":
      return {
        ...state,
        selectedKeys: new Set([...state.selectedKeys].filter((key) => action.available.has(key))),
      };
    case "selection_recovered":
      return {
        selectedKeys: action.resume ? new Set(action.remaining) : new Set(),
        selectionMode: action.resume && action.remaining.size > 0,
      };
  }
}

/** True when every selectable key is already selected. */
export function allSelected<Key extends string>(
  selectable: readonly Key[],
  selected: ReadonlySet<Key>,
): boolean {
  return selectable.length > 0 && selectable.every((key) => selected.has(key));
}
