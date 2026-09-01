import type { ComponentProps, ReactNode, RefObject } from "react";
import { forwardRef } from "react";
import { ActionButton, type ActionButtonTone } from "@/shared/ui/ActionButton";
import styles from "@/shared/ui/IconButton.module.css";

type IconButtonProps = Omit<ComponentProps<typeof ActionButton>, "children"> & {
  label: string;
  children: ReactNode;
  /** @deprecated Prefer the forwarded ref. Kept for existing call sites. */
  buttonRef?: RefObject<HTMLButtonElement | null>;
  tone?: Extract<ActionButtonTone, "ghost" | "dangerQuiet" | "danger">;
};

export const IconButton = forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { label, children, className, buttonRef, tone = "ghost", ...props },
  ref,
) {
  return (
    <ActionButton
      {...props}
      ref={buttonRef ?? ref}
      className={`${styles.button} ${className ?? ""}`}
      data-icon-button="true"
      type="button"
      tone={tone}
      aria-label={label}
    >
      {children}
    </ActionButton>
  );
});
