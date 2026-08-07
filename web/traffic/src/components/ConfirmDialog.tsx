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
  const confirmButton = useRef<HTMLButtonElement>(null);

  useEffect(() => confirmButton.current?.focus(), []);

  return (
    <div
      className={styles.backdrop}
      role="presentation"
      onMouseDown={(event) => event.target === event.currentTarget && onCancel()}
    >
      <section
        className={styles.dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
      >
        <div className={styles.icon}>
          <AlertTriangle size={20} aria-hidden="true" />
        </div>
        <h2 id="confirm-title">{title}</h2>
        <p>{message}</p>
        <div className={styles.actions}>
          <button type="button" onClick={onCancel} disabled={busy}>
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
    </div>
  );
}
