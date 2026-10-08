import { useCallback, useEffect, useRef, useState } from "react";

import type { ComponentKind } from "@/api/tenants";
import { useElementRegistry } from "@/features/common/useElementRegistry";
import { componentMenuCoordinates } from "@/features/tenants/components/componentCatalog";

export function useComponentMenu() {
  const [openMenu, setOpenMenu] = useState<ComponentKind | null>(null);
  const [menuPosition, setMenuPosition] = useState<{ top: number; left: number } | null>(null);
  const menuButtons = useElementRegistry<HTMLButtonElement, ComponentKind>();
  const menuItems = useElementRegistry<HTMLButtonElement, ComponentKind>();
  const menuRef = useRef<HTMLDivElement>(null);

  const open = useCallback((kind: ComponentKind, anchor: HTMLElement, width: number) => {
    setMenuPosition(componentMenuCoordinates(anchor.getBoundingClientRect(), width));
    setOpenMenu(kind);
  }, []);

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

  useEffect(() => {
    if (!openMenu) return;
    function positionMenu() {
      if (!openMenu) return;
      const button = menuButtons.get(openMenu);
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
      menuItems.get(openMenu)?.focus();
    });
    const closeOnPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      const button = menuButtons.get(openMenu);
      if (button?.contains(target) || menuRef.current?.contains(target)) return;
      close();
    };
    const closeOnKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      const key = openMenu;
      close();
      window.requestAnimationFrame(() => menuButtons.get(key)?.focus());
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
  }, [close, menuButtons, menuItems, openMenu]);

  return {
    close,
    menuPosition,
    menuRef,
    open,
    openMenu,
    registerButton: menuButtons.register,
    registerItem: menuItems.register,
    toggle,
  };
}
