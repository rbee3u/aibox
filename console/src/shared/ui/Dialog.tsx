import { useEffect, useRef } from "react";
import type { KeyboardEvent, ReactNode, RefObject } from "react";

const FOCUSABLE = [
  "button:not([disabled])",
  "input:not([disabled])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "a[href]",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

export function Dialog({
  ariaLabelledBy,
  busy = false,
  children,
  className,
  initialFocusRef,
  onCancel,
}: {
  ariaLabelledBy?: string;
  busy?: boolean;
  children: ReactNode;
  className?: string;
  initialFocusRef?: RefObject<HTMLElement | null>;
  onCancel: () => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const restoreFocusRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const dialog = dialogRef.current;
    restoreFocusRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (dialog) {
      if (typeof dialog.showModal === "function") dialog.showModal();
      else dialog.setAttribute("open", "");
    }
    const initial =
      initialFocusRef?.current ?? dialog?.querySelector<HTMLElement>("[autofocus], " + FOCUSABLE);
    initial?.focus();
    return () => {
      if (dialog?.open) {
        if (typeof dialog.close === "function") dialog.close();
        else dialog.removeAttribute("open");
      }
      const target = restoreFocusRef.current;
      window.setTimeout(() => {
        const active = document.activeElement;
        if (active && active !== document.body && active !== document.documentElement) return;
        if (target?.isConnected) target.focus();
        else document.querySelector<HTMLElement>('[data-dialog-focus-fallback="true"]')?.focus();
      });
    };
  }, [initialFocusRef]);

  function cancel() {
    if (!busy) onCancel();
  }

  function trapFocus(event: KeyboardEvent<HTMLDialogElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      cancel();
      return;
    }
    if (event.key !== "Tab") return;
    const controls = [...event.currentTarget.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const first = controls.at(0);
    const last = controls.at(-1);
    if (!first || !last) return;
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <dialog
      ref={dialogRef}
      className={className}
      aria-labelledby={ariaLabelledBy}
      onCancel={(event) => {
        event.preventDefault();
        cancel();
      }}
      onKeyDown={trapFocus}
      onClick={(event) => {
        if (busy || event.target !== event.currentTarget) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        if (
          event.clientX < bounds.left ||
          event.clientX > bounds.right ||
          event.clientY < bounds.top ||
          event.clientY > bounds.bottom
        ) {
          cancel();
        }
      }}
    >
      {children}
    </dialog>
  );
}
