import type { ButtonHTMLAttributes } from "react";
import { forwardRef } from "react";
import styles from "@/shared/ui/ActionButton.module.css";

export type ActionButtonTone = "default" | "primary" | "danger" | "quiet";

export interface ActionButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  tone?: ActionButtonTone;
}

export const ActionButton = forwardRef<HTMLButtonElement, ActionButtonProps>(function ActionButton(
  { type = "button", tone = "default", className, children, ...props },
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
