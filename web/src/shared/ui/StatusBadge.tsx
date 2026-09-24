import type { ReactNode } from "react";
import styles from "@/shared/ui/StatusBadge.module.css";

export type StatusTone = "good" | "neutral" | "warning" | "error" | "active";
export type StatusVariant = "inline" | "badge";
/** Ladder step the label takes; a status annotating a row stays at the floor. */
export type StatusSize = "xs" | "sm";

interface StatusBadgeProps {
  tone: StatusTone;
  children: ReactNode;
  variant: StatusVariant;
  size?: StatusSize;
  dot?: boolean;
  /** Keep the label in the root when a parent owns the inline text layout. */
  wrapLabel?: boolean;
  className?: string;
  title?: string;
}

/** Shared status treatment; variant controls emphasis while tone controls semantics. */
export function StatusBadge({
  tone,
  children,
  variant,
  size = "xs",
  dot = true,
  wrapLabel = true,
  className,
  title,
}: StatusBadgeProps) {
  return (
    <span
      className={`${styles.root} ${styles[variant]} ${styles[size]} ${styles[tone]} ${className ?? ""}`}
      data-status-tone={tone}
      data-status-variant={variant}
      data-status-size={size}
      title={title}
    >
      {dot && <span className={styles.dot} aria-hidden="true" />}
      {wrapLabel ? <span className={styles.label}>{children}</span> : children}
    </span>
  );
}
