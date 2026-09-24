import { useCallback, useEffect, useRef, useState } from "react";

const MOBILE_LAYOUT_QUERY = "(max-width: 900px)";

/**
 * Runs the narrow-layout navigation drawer: it tracks the mobile breakpoint,
 * makes the sidebar inert while closed, and traps focus inside it while open.
 */
export function useMobileNavigation() {
  const [open, setOpen] = useState(false);
  const [mobileLayout, setMobileLayout] = useState(
    () => window.matchMedia?.(MOBILE_LAYOUT_QUERY).matches ?? false,
  );
  const sidebarRef = useRef<HTMLElement>(null);
  const menuButtonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!window.matchMedia) return;
    const query = window.matchMedia(MOBILE_LAYOUT_QUERY);
    const update = () => {
      setMobileLayout(query.matches);
      if (!query.matches) setOpen(false);
    };
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, []);

  const close = useCallback((restoreFocus = true) => {
    setOpen(false);
    if (restoreFocus) window.requestAnimationFrame(() => menuButtonRef.current?.focus());
  }, []);

  useEffect(() => {
    const sidebar = sidebarRef.current;
    if (!sidebar) return;
    sidebar.inert = mobileLayout && !open;
    if (!mobileLayout || !open) return;
    const focusable = () =>
      [...sidebar.querySelectorAll<HTMLElement>("a[href], button:not([disabled]), select")].filter(
        (element) => !element.hidden && element.tabIndex >= 0,
      );
    window.requestAnimationFrame(() =>
      (sidebar.querySelector<HTMLElement>('[aria-current="page"]') ?? focusable()[0])?.focus(),
    );
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented) return;
      if (event.key === "Escape") {
        event.preventDefault();
        close();
        return;
      }
      if (event.key !== "Tab") return;
      const elements = focusable();
      const first = elements[0];
      const last = elements.at(-1);
      if (!first || !last) return;
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [close, mobileLayout, open]);

  return { open, setOpen, close, mobileLayout, sidebarRef, menuButtonRef };
}
