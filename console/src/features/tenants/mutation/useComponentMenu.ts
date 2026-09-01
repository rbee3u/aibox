import { useCallback, useEffect, useRef, useState } from "react";

import type { ComponentKind } from "@/api/tenants";
import { componentMenuCoordinates } from "@/features/tenants/componentCatalog";

export function useComponentMenu() {
  const [openMenu, setOpenMenu] = useState<ComponentKind | null>(null);
  const [menuPosition, setMenuPosition] = useState<{ top: number; left: number } | null>(null);
  const menuButtons = useRef(new Map<ComponentKind, HTMLButtonElement>());
  const menuItems = useRef(new Map<ComponentKind, HTMLButtonElement>());
  const menuRef = useRef<HTMLDivElement>(null);

  const positionFromAnchor = useCallback((anchor: HTMLElement, width: number) => {
    setMenuPosition(componentMenuCoordinates(anchor.getBoundingClientRect(), width));
  }, []);

  const open = useCallback(
    (kind: ComponentKind, anchor: HTMLElement, width: number) => {
      positionFromAnchor(anchor, width);
      setOpenMenu(kind);
    },
    [positionFromAnchor],
  );

  const toggle = useCallback(
    (kind: ComponentKind, anchor: HTMLElement, width: number) => {
      if (openMenu === kind) setOpenMenu(null);
      else open(kind, anchor, width);
    },
    [open, openMenu],
  );

  const close = useCallback(() => {
    setOpenMenu(null);
  }, []);

  const registerButton = useCallback((kind: ComponentKind, element: HTMLButtonElement | null) => {
    if (element) menuButtons.current.set(kind, element);
    else menuButtons.current.delete(kind);
  }, []);

  const registerItem = useCallback((kind: ComponentKind, element: HTMLButtonElement | null) => {
    if (element) menuItems.current.set(kind, element);
    else menuItems.current.delete(kind);
  }, []);

  useEffect(() => {
    if (!openMenu) return;
    function positionMenu() {
      if (!openMenu) return;
      const button = menuButtons.current.get(openMenu);
      const menu = menuRef.current;
      if (!button || !menu) return;
      const menuBounds = menu.getBoundingClientRect();
      setMenuPosition(
        componentMenuCoordinates(
          button.getBoundingClientRect(),
          menuBounds.width,
          menuBounds.height,
        ),
      );
    }
    const positionFrame = window.requestAnimationFrame(positionMenu);
    const focusFrame = window.requestAnimationFrame(() => {
      positionMenu();
      menuItems.current.get(openMenu)?.focus();
    });
    const closeOnPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      const button = menuButtons.current.get(openMenu);
      if (button?.contains(target) || menuRef.current?.contains(target)) return;
      close();
    };
    const closeOnKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      const key = openMenu;
      close();
      window.requestAnimationFrame(() => menuButtons.current.get(key)?.focus());
    };
    document.addEventListener("pointerdown", closeOnPointerDown);
    document.addEventListener("keydown", closeOnKeyDown);
    window.addEventListener("resize", positionMenu);
    window.addEventListener("scroll", positionMenu, true);
    return () => {
      window.cancelAnimationFrame(positionFrame);
      window.cancelAnimationFrame(focusFrame);
      document.removeEventListener("pointerdown", closeOnPointerDown);
      document.removeEventListener("keydown", closeOnKeyDown);
      window.removeEventListener("resize", positionMenu);
      window.removeEventListener("scroll", positionMenu, true);
    };
  }, [close, openMenu]);

  return {
    close,
    menuPosition,
    menuRef,
    open,
    openMenu,
    registerButton,
    registerItem,
    toggle,
  };
}
