import type { ButtonHTMLAttributes } from "react";
import { forwardRef } from "react";
import styles from "@/shared/ui/ActionButton.module.css";

export type ActionButtonTone = "primary" | "secondary" | "ghost" | "dangerQuiet" | "danger";

export interface ActionButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  tone?: ActionButtonTone;
}

export const ActionButton = forwardRef<HTMLButtonElement, ActionButtonProps>(function ActionButton(
  { type = "button", tone = "secondary", className, children, ...props },
  ref,
) {
  return (
    <button
      {...props}
      ref={ref}
      className={`${styles.button} ${styles[tone]} ${className ?? ""}`}
      type={type}
    >
      {children}
    </button>
  );
});
