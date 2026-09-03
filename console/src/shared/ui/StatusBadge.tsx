import type { ReactNode } from "react";
import styles from "@/shared/ui/StatusBadge.module.css";

export type StatusTone = "good" | "neutral" | "warning" | "error" | "active";
export type StatusVariant = "inline" | "badge";

interface StatusBadgeProps {
  tone: StatusTone;
  children: ReactNode;
  variant: StatusVariant;
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
  dot = variant === "inline",
  wrapLabel = true,
  className,
  title,
}: StatusBadgeProps) {
  return (
    <span
      className={`${styles.root} ${styles[variant]} ${styles[tone]} ${className ?? ""}`}
      data-status-tone={tone}
      data-status-variant={variant}
      title={title}
    >
      {dot && <span className={styles.dot} aria-hidden="true" />}
      {wrapLabel ? <span className={styles.label}>{children}</span> : children}
    </span>
  );
}
