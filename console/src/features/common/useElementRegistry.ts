import { useCallback, useMemo, useRef } from "react";

/**
 * Keeps a keyed map of live DOM elements so a page can move focus to a row it
 * does not otherwise hold a ref to.
 *
 * Six call sites built this by hand with a bare `useRef(new Map(...))` plus a
 * register callback. The map is a ref because it never affects rendering; only
 * the keys a caller focuses do.
 */
export function useElementRegistry<Element extends HTMLElement, Key extends string = string>() {
  const elements = useRef(new Map<Key, Element>());

  const register = useCallback((key: Key, element: Element | null) => {
    if (element) elements.current.set(key, element);
    else elements.current.delete(key);
  }, []);

  /** Focus a registered element, skipping one that is absent or disabled. */
  const focus = useCallback((key: Key) => {
    const target = elements.current.get(key);
    if (!target) return false;
    if (target instanceof HTMLButtonElement && target.disabled) return false;
    target.focus();
    return true;
  }, []);

  const get = useCallback((key: Key) => elements.current.get(key) ?? null, []);

  // A stable object so an effect that focuses a row can list the registry as a
  // dependency without re-running on every render.
  return useMemo(() => ({ register, focus, get }), [focus, get, register]);
}
