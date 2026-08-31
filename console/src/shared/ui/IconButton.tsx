import type { ComponentProps, ReactNode, RefObject } from "react";
import { ActionButton, type ActionButtonTone } from "@/shared/ui/ActionButton";
import styles from "@/shared/ui/IconButton.module.css";

export function IconButton({
  label,
  children,
  className,
  buttonRef,
  tone = "default",
  ...props
}: Omit<ComponentProps<typeof ActionButton>, "children"> & {
  label: string;
  children: ReactNode;
  buttonRef?: RefObject<HTMLButtonElement | null>;
  tone?: Exclude<ActionButtonTone, "quiet" | "primary">;
}) {
  return (
    <ActionButton
      {...props}
      ref={buttonRef}
      className={`${styles.button} ${className ?? ""}`}
      data-icon-button="true"
      type="button"
      tone={tone}
      aria-label={label}
    >
      {children}
    </ActionButton>
  );
}
