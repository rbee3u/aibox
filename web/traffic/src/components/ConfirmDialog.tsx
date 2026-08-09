import { AlertTriangle, Trash2 } from "lucide-react";
import { useEffect, useRef } from "react";
import styles from "./ConfirmDialog.module.css";

interface ConfirmDialogProps {
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  busy?: boolean;
}

export function ConfirmDialog({
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
  busy = false,
}: ConfirmDialogProps) {
  const dialog = useRef<HTMLDialogElement>(null);
  const confirmButton = useRef<HTMLButtonElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const element = dialog.current;
    restoreFocus.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (element) {
      if (typeof element.showModal === "function") element.showModal();
      else element.setAttribute("open", "");
    }
    confirmButton.current?.focus();
    return () => {
      const target = restoreFocus.current;
      window.setTimeout(() => {
        if (target?.isConnected) target.focus();
        else document.querySelector<HTMLElement>('[data-dialog-focus-fallback="true"]')?.focus();
      });
    };
  }, []);

  function cancelDialog() {
    if (busy) return;
    const element = dialog.current;
    if (element?.open && typeof element.close === "function") element.close();
    onCancel();
  }

  function handleKeyDown(event: React.KeyboardEvent<HTMLDialogElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      cancelDialog();
      return;
    }
    if (event.key !== "Tab") return;
    const first = cancelButton.current;
    const last = confirmButton.current;
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
      ref={dialog}
      className={styles.dialog}
      aria-labelledby="confirm-title"
      onCancel={(event) => {
        event.preventDefault();
        cancelDialog();
      }}
      onKeyDown={handleKeyDown}
      onClick={(event) => {
        if (busy || event.target !== event.currentTarget) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        if (
          event.clientX < bounds.left ||
          event.clientX > bounds.right ||
          event.clientY < bounds.top ||
          event.clientY > bounds.bottom
        ) {
          cancelDialog();
        }
      }}
    >
      <section className={styles.content}>
        <div className={styles.icon}>
          <AlertTriangle size={20} aria-hidden="true" />
        </div>
        <h2 id="confirm-title">{title}</h2>
        <p>{message}</p>
        <div className={styles.actions}>
          <button ref={cancelButton} type="button" onClick={cancelDialog} disabled={busy}>
            Cancel
          </button>
          <button
            ref={confirmButton}
            type="button"
            className={styles.danger}
            onClick={onConfirm}
            disabled={busy}
          >
            <Trash2 size={15} aria-hidden="true" />
            {busy ? "Deleting…" : confirmLabel}
          </button>
        </div>
      </section>
    </dialog>
  );
}
