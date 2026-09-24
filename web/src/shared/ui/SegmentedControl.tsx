import type { HTMLAttributes } from "react";
import styles from "@/shared/ui/SegmentedControl.module.css";

export type SegmentedControlVariant = "filled" | "tabs";

export interface SegmentedControlProps extends HTMLAttributes<HTMLDivElement> {
  variant?: SegmentedControlVariant;
}

/** Shared visual base for compact mode switches and page-level tabs. */
export function SegmentedControl({
  variant = "filled",
  className,
  children,
  ...props
}: SegmentedControlProps) {
  return (
    <div {...props} className={`${styles.root} ${styles[variant]} ${className ?? ""}`}>
      {children}
    </div>
  );
}
