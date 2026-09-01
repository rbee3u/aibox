import { useEffect, type RefObject } from "react";

const NARROW_LAYOUT_QUERY = "(max-width: 760px)";

/**
 * Moves focus into a detail region when a narrow layout replaces the catalog.
 * Desktop layouts keep both regions visible and are left alone.
 */
export function useNarrowDetailFocus(
  target: RefObject<HTMLElement | null>,
  active: boolean,
  ...changeKeys: readonly unknown[]
) {
  useEffect(() => {
    if (!active || !window.matchMedia?.(NARROW_LAYOUT_QUERY).matches) return;
    const frame = window.requestAnimationFrame(() => target.current?.focus());
    return () => window.cancelAnimationFrame(frame);
    // The caller lists the selection values that should re-run this focus move.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, target, ...changeKeys]);
}
