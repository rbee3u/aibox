import { useEffect, useRef, type RefObject } from "react";

interface SelectionModeFocusOptions {
  selectionMode: boolean;
  /** The control that enters selection mode; focus returns to it on exit. */
  selectButton: RefObject<HTMLButtonElement | null>;
  /** Where exit focus goes when the Select control is missing or disabled. */
  fallbackButton?: RefObject<HTMLButtonElement | null>;
  /** Focuses the first row that can be ticked; returns whether one existed. */
  focusFirstSelectable: () => boolean;
  onEnter: () => void;
  onExit: () => void;
}

/**
 * Preserve focus when selection mode replaces the toolbar. Only explicit enter
 * and cancel actions move focus here; batch deletion owns its recovery target.
 */
export function useSelectionModeFocus({
  selectionMode,
  selectButton,
  fallbackButton,
  focusFirstSelectable,
  onEnter,
  onExit,
}: SelectionModeFocusOptions) {
  const pending = useRef<"enter" | "exit" | null>(null);

  useEffect(() => {
    const intent = pending.current;
    if (!intent) return;
    if (intent === "enter" && selectionMode) {
      pending.current = null;
      if (!focusFirstSelectable()) fallbackButton?.current?.focus();
    } else if (intent === "exit" && !selectionMode) {
      pending.current = null;
      const target =
        selectButton.current && !selectButton.current.disabled
          ? selectButton.current
          : fallbackButton?.current;
      if (target && !target.disabled) target.focus();
    }
  }, [fallbackButton, focusFirstSelectable, selectButton, selectionMode]);

  return {
    enterSelection: () => {
      pending.current = "enter";
      onEnter();
    },
    cancelSelection: () => {
      pending.current = "exit";
      onExit();
    },
  };
}
