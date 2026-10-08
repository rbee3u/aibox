import { useCallback, useMemo, useRef } from "react";

/** Registration affects focus, not rendering, so elements stay in a ref. */
export function useElementRegistry<Element extends HTMLElement, Key extends string = string>() {
  const elements = useRef(new Map<Key, Element>());

  const register = useCallback((key: Key, element: Element | null) => {
    if (element) elements.current.set(key, element);
    else elements.current.delete(key);
  }, []);

  const focus = useCallback((key: Key) => {
    const target = elements.current.get(key);
    if (!target) return false;
    if (target instanceof HTMLButtonElement && target.disabled) return false;
    target.focus();
    return true;
  }, []);

  const get = useCallback((key: Key) => elements.current.get(key) ?? null, []);

  // Stable identity lets focus effects depend on the registry.
  return useMemo(() => ({ register, focus, get }), [focus, get, register]);
}
