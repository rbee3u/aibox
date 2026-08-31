import { AlertTriangle, Check, LoaderCircle, Trash2 } from "lucide-react";
import { useId, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { ActionButton } from "@/shared/ui/ActionButton";
import { Dialog } from "@/shared/ui/Dialog";
import { TextInput } from "@/shared/ui/FormControls";
import styles from "@/shared/ui/ConfirmDialog.module.css";

interface ConfirmDialogProps {
  title: string;
  message?: string;
  description?: ReactNode;
  confirmation?: string;
  confirmLabel: string;
  variant?: "danger" | "primary";
  onConfirm: () => void;
  onCancel: () => void;
  busy?: boolean;
}

export function ConfirmDialog({
  title,
  message,
  description,
  confirmation,
  confirmLabel,
  variant = "danger",
  onConfirm,
  onCancel,
  busy = false,
}: ConfirmDialogProps) {
  const titleId = useId();
  const [typed, setTyped] = useState("");
  const [copied, copy] = useClipboardFeedback();
  const inputRef = useRef<HTMLInputElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const enabled = !confirmation || typed === confirmation;

  return (
    <Dialog
      className={styles.dialog}
      ariaLabelledBy={titleId}
      busy={busy}
      initialFocusRef={confirmation ? inputRef : cancelRef}
      onCancel={onCancel}
    >
      <section className={styles.content}>
        <div className={`${styles.icon} ${variant === "primary" ? styles.primaryIcon : ""}`}>
          <AlertTriangle size={20} aria-hidden="true" />
        </div>
        <h2 id={titleId}>{title}</h2>
        {message && <p>{message}</p>}
        {description}
        {confirmation && (
          <label className={styles.confirmation}>
            <span className={styles.confirmationPrompt}>
              Type{" "}
              <button
                type="button"
                className={styles.confirmationPhrase}
                onClick={() => void copy(confirmation, true)}
                aria-label={copied ? `Copied ${confirmation}` : `Copy ${confirmation}`}
                title={copied ? "Copied" : "Click to copy"}
              >
                <code>{confirmation}</code>
                {copied ? <Check size={12} aria-hidden="true" /> : null}
              </button>{" "}
              to confirm
            </span>
            <TextInput
              ref={inputRef}
              value={typed}
              onChange={(event) => setTyped(event.target.value)}
            />
          </label>
        )}
        <div className={styles.actions}>
          <ActionButton ref={cancelRef} onClick={onCancel} disabled={busy}>
            Cancel
          </ActionButton>
          <ActionButton
            tone={variant === "danger" ? "danger" : "primary"}
            onClick={onConfirm}
            disabled={!enabled || busy}
          >
            {busy ? (
              <LoaderCircle className="spin" size={15} aria-hidden="true" />
            ) : variant === "danger" ? (
              <Trash2 size={15} aria-hidden="true" />
            ) : null}
            {busy ? (variant === "danger" ? "Deleting…" : `${confirmLabel}…`) : confirmLabel}
          </ActionButton>
        </div>
      </section>
    </Dialog>
  );
}
