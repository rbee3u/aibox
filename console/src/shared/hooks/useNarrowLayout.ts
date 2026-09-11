import { useEffect, useState } from "react";

export const NARROW_LAYOUT_QUERY = "(max-width: 760px)";

/**
 * Catalog inspection chrome follows the visible detail. Desktop split views
 * still mark the fallback row; one-pane catalogs wait until a row is opened.
 */
export function catalogMarksInspection(narrowLayout: boolean, detailOpen: boolean): boolean {
  return !narrowLayout || detailOpen;
}

/** Tracks the catalog/detail one-pane breakpoint used by `catalog.module.css`. */
export function useNarrowLayout(): boolean {
  const [narrowLayout, setNarrowLayout] = useState(
    () => window.matchMedia?.(NARROW_LAYOUT_QUERY).matches ?? false,
  );
  useEffect(() => {
    const media = window.matchMedia?.(NARROW_LAYOUT_QUERY);
    if (!media) return;
    const update = () => setNarrowLayout(media.matches);
    update();
    media.addEventListener?.("change", update);
    return () => media.removeEventListener?.("change", update);
  }, []);
  return narrowLayout;
}
