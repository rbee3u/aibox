import type { ButtonHTMLAttributes, ReactNode, RefObject } from "react";
import styles from "./IconButton.module.css";

export function IconButton({
  label,
  children,
  className,
  buttonRef,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  label: string;
  children: ReactNode;
  buttonRef?: RefObject<HTMLButtonElement | null>;
}) {
  return (
    <button
      ref={buttonRef}
      className={`${styles.button} ${className ?? ""}`}
      data-icon-button
      type="button"
      title={label}
      aria-label={label}
      {...props}
    >
      {children}
    </button>
  );
}
