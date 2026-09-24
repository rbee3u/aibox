import { forwardRef, type ReactNode } from "react";
import { ActionButton, type ActionButtonProps } from "@/shared/ui/ActionButton";
import styles from "@/shared/ui/IconLabelButton.module.css";

export interface IconLabelButtonProps extends ActionButtonProps {
  icon: ReactNode;
  compactOnNarrow?: boolean;
}

export const IconLabelButton = forwardRef<HTMLButtonElement, IconLabelButtonProps>(
  function IconLabelButton(
    { icon, children, compactOnNarrow = false, className, tone = "ghost", ...props },
    ref,
  ) {
    return (
      <ActionButton
        {...props}
        ref={ref}
        tone={tone}
        className={`${styles.button} ${compactOnNarrow ? styles.compact : ""} ${className ?? ""}`}
      >
        {icon}
        <span className={styles.label}>{children}</span>
      </ActionButton>
    );
  },
);
