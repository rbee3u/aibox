import { AlertTriangle, LoaderCircle } from "lucide-react";
import { useId, useRef, useState } from "react";
import type { ReactNode } from "react";
import { ActionButton } from "@/shared/ui/ActionButton";
import { Dialog } from "@/shared/ui/Dialog";
import { TextInput } from "@/shared/ui/FormControls";
import styles from "@/shared/ui/ConfirmDialog.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

export interface ConfirmDialogFact {
  label: string;
  value: ReactNode;
}

interface ConfirmDialogProps {
  title: string;
  facts?: ReadonlyArray<ConfirmDialogFact>;
  message?: string;
  description?: ReactNode;
  confirmation?: string;
  confirmLabel: string;
  busyLabel?: string;
  variant?: "danger" | "primary";
  onConfirm: () => void;
  onCancel: () => void;
  busy?: boolean;
}

export function ConfirmDialog({
  title,
  facts,
  message,
  description,
  confirmation,
  confirmLabel,
  busyLabel,
  variant = "danger",
  onConfirm,
  onCancel,
  busy = false,
}: ConfirmDialogProps) {
  const titleId = useId();
  const inputId = useId();
  const [typed, setTyped] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const enabled = !confirmation || typed === confirmation;
  const pendingLabel = busyLabel ?? (variant === "danger" ? "Deleting…" : "Applying…");

  return (
    <Dialog
      className={styles.dialog}
      ariaLabelledBy={titleId}
      busy={busy}
      initialFocusRef={confirmation ? inputRef : cancelRef}
      onCancel={onCancel}
    >
      {/*
       * A form so Enter in the typed confirmation confirms, the way the
       * pattern promises; the button stays disabled until the name matches.
       */}
      <form
        className={styles.content}
        onSubmit={(event) => {
          event.preventDefault();
          if (enabled && !busy) onConfirm();
        }}
      >
        <div className={`${styles.icon} ${variant === "primary" ? styles.primaryIcon : ""}`}>
          <AlertTriangle size={iconSize.md} aria-hidden="true" />
        </div>
        <h2 id={titleId}>{title}</h2>
        {facts && facts.length > 0 && (
          <dl className={styles.facts}>
            {facts.map((fact) => (
              <div key={fact.label}>
                <dt>{fact.label}</dt>
                <dd>{fact.value}</dd>
              </div>
            ))}
          </dl>
        )}
        {message && <p className={styles.message}>{message}</p>}
        {description}
        {confirmation && (
          <div className={styles.confirmation}>
            <label htmlFor={inputId} className={styles.confirmationPrompt}>
              Type <code className={styles.confirmationName}>{confirmation}</code> to confirm
            </label>
            <TextInput
              id={inputId}
              ref={inputRef}
              value={typed}
              onChange={(event) => setTyped(event.target.value)}
            />
          </div>
        )}
        <div className={styles.actions}>
          <ActionButton
            ref={cancelRef}
            type="button"
            tone="secondary"
            onClick={onCancel}
            disabled={busy}
          >
            Cancel
          </ActionButton>
          <ActionButton
            type="submit"
            tone={variant === "danger" ? "dangerPrimary" : "primary"}
            disabled={!enabled || busy}
          >
            {busy && <LoaderCircle className="spin" size={iconSize.xs} aria-hidden="true" />}
            {busy ? pendingLabel : confirmLabel}
          </ActionButton>
        </div>
      </form>
    </Dialog>
  );
}
