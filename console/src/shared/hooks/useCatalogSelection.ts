import { useCallback, useRef, useState } from "react";

/**
 * Holds the batch-selection state a catalog needs: whether selection mode is
 * active, which rows are selected, and an optional per-row context the page
 * records at selection time (for example the page a Request was selected on).
 *
 * The context map is a ref because it never affects rendering; only the selected
 * identifiers do.
 */
export function useCatalogSelection<Context = never>() {
  const [active, setActive] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const contexts = useRef<Map<string, Context>>(new Map());

  const clear = useCallback(() => {
    setSelected(new Set());
    contexts.current.clear();
  }, []);

  const exit = useCallback(() => {
    setActive(false);
    clear();
  }, [clear]);

  const toggle = useCallback((id: string, context?: Context) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.delete(id)) {
        contexts.current.delete(id);
      } else {
        next.add(id);
        if (context !== undefined) contexts.current.set(id, context);
      }
      return next;
    });
  }, []);

  /** Selects every identifier, or clears them all when they are already selected. */
  const toggleAll = useCallback((ids: readonly string[], context?: Context) => {
    setSelected((current) => {
      const next = new Set(current);
      const allSelected = ids.length > 0 && ids.every((id) => current.has(id));
      for (const id of ids) {
        if (allSelected) {
          next.delete(id);
          contexts.current.delete(id);
        } else if (!next.has(id)) {
          next.add(id);
          if (context !== undefined) contexts.current.set(id, context);
        }
      }
      return next;
    });
  }, []);

  return {
    active,
    enter: useCallback(() => setActive(true), []),
    exit,
    clear,
    selected,
    ids: [...selected],
    toggle,
    toggleAll,
    contextOf: useCallback((id: string) => contexts.current.get(id), []),
    setActive,
  };
}
