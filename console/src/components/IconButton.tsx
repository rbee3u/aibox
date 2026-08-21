import { Tooltip } from "antd";
import type { ComponentProps, ReactNode, RefObject } from "react";
import { ActionButton } from "./ActionButton";
import styles from "./IconButton.module.css";

export function IconButton({
  label,
  children,
  className,
  buttonRef,
  ...props
}: Omit<ComponentProps<typeof ActionButton>, "children" | "htmlType" | "tone"> & {
  label: string;
  children: ReactNode;
  buttonRef?: RefObject<HTMLButtonElement | null>;
}) {
  return (
    <Tooltip title={label} mouseEnterDelay={0.45}>
      <ActionButton
        ref={buttonRef}
        className={`${styles.button} ${className ?? ""}`}
        data-icon-button="true"
        htmlType="button"
        tone="quiet"
        title={label}
        aria-label={label}
        {...props}
      >
        {children}
      </ActionButton>
    </Tooltip>
  );
}
