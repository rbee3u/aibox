import type { HTMLAttributes, ReactNode } from "react";
import styles from "./EmptyState.module.css";

type EmptyStateProps = Omit<HTMLAttributes<HTMLDivElement>, "children" | "title"> & {
  variant: "list" | "detail";
  icon: ReactNode;
  title?: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
};

export function EmptyState({
  variant,
  icon,
  title,
  description,
  children,
  className,
  ...props
}: EmptyStateProps) {
  return (
    <div
      className={`${styles.root} ${styles[variant]} ${className ?? ""}`}
      data-empty-state={variant}
      {...props}
    >
      <span className={styles.icon}>{icon}</span>
      {title && (variant === "detail" ? <h2>{title}</h2> : <strong>{title}</strong>)}
      {description && <p>{description}</p>}
      {children}
    </div>
  );
}
