import { RefreshCw } from "lucide-react";
import { forwardRef, type ReactNode } from "react";
import { ActionButton, type ActionButtonProps } from "@/shared/ui/ActionButton";
import styles from "@/shared/ui/RefreshButton.module.css";

export interface RefreshButtonProps extends Omit<
  ActionButtonProps,
  "aria-label" | "children" | "title" | "tone"
> {
  label: string;
  busy?: boolean;
  busyLabel?: string;
  children?: ReactNode;
  iconOnly?: boolean;
  iconSize?: number;
}

export const RefreshButton = forwardRef<HTMLButtonElement, RefreshButtonProps>(
  function RefreshButton(
    {
      label,
      busy = false,
      busyLabel,
      children,
      className,
      iconOnly = false,
      iconSize = 14,
      ...props
    },
    ref,
  ) {
    return (
      <ActionButton
        {...props}
        ref={ref}
        className={`${styles.button} ${iconOnly ? styles.iconOnly : ""} ${className ?? ""}`}
        data-refresh-button="true"
        tone="default"
        aria-label={busy && busyLabel ? busyLabel : label}
        aria-busy={busy || undefined}
      >
        <RefreshCw className={busy ? "spin" : undefined} size={iconSize} aria-hidden="true" />
        {!iconOnly && (children ?? label)}
      </ActionButton>
    );
  },
);
