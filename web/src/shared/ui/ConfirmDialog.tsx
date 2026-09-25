import { ArrowRightLeft, Check, Copy, LoaderCircle, Trash2 } from "lucide-react";
import { useId, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useClipboardFeedback } from "@/shared/hooks/useClipboardFeedback";
import { ActionButton } from "@/shared/ui/ActionButton";
import { Dialog } from "@/shared/ui/Dialog";
import { TextInput } from "@/shared/ui/FormControls";
import styles from "@/shared/ui/ConfirmDialog.module.css";
import { iconSize } from "@/shared/icons/iconSizes";

export interface ConfirmDialogFact {
  label: string;
  value: ReactNode;
  fullWidth?: boolean;
}

export interface ContextPillProps {
  icon?: ReactNode;
  label?: ReactNode;
  value: ReactNode;
  className?: string;
}

export function ContextPill({ icon, label, value, className }: ContextPillProps) {
  return (
    <span className={`${styles.contextPill} ${className ?? ""}`.trim()}>
      {icon}
      {label !== undefined && <small>{label}: </small>}
      <strong>{value}</strong>
    </span>
  );
}

export interface ConfirmDialogProps {
  title: string;
  icon?: ReactNode;
  pills?: ReactNode;
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
  icon,
  pills,
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
  const [copied, copy] = useClipboardFeedback();
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
          {icon ??
            (variant === "primary" ? (
              <ArrowRightLeft size={iconSize.md} aria-hidden="true" />
            ) : (
              <Trash2 size={iconSize.md} aria-hidden="true" />
            ))}
        </div>
        <h2 id={titleId}>{title}</h2>
        {pills && <div className={styles.pills}>{pills}</div>}
        {facts && facts.length > 0 && (
          <dl className={styles.facts}>
            {facts.map((fact) => (
              <div key={fact.label} className={fact.fullWidth ? styles.fullWidthFact : undefined}>
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
            <div className={styles.confirmationPrompt}>
              <span>Type</span>
              <div className={styles.confirmationShortcuts}>
                <button
                  type="button"
                  className={styles.confirmationFill}
                  onClick={() => {
                    setTyped(confirmation);
                    inputRef.current?.focus();
                  }}
                  title="Click to fill"
                  aria-label={`Fill ${confirmation}`}
                >
                  <code className={styles.confirmationName}>{confirmation}</code>
                </button>
                <button
                  type="button"
                  className={styles.confirmationCopy}
                  onClick={() => void copy(confirmation, true)}
                  aria-label={copied ? `Copied ${confirmation}` : `Copy ${confirmation}`}
                  title={copied ? "Copied" : "Copy to clipboard"}
                >
                  {copied ? (
                    <Check size={iconSize.xs} aria-hidden="true" />
                  ) : (
                    <Copy size={iconSize.xs} aria-hidden="true" />
                  )}
                </button>
              </div>
              <span>to confirm</span>
            </div>
            <TextInput
              id={inputId}
              ref={inputRef}
              value={typed}
              onChange={(event) => setTyped(event.target.value)}
              aria-label={`Type ${confirmation} to confirm`}
              placeholder={`Type "${confirmation}" or click the text above`}
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
