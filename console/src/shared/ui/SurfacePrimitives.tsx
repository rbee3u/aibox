import type { HTMLAttributes, ReactNode } from "react";
import styles from "@/shared/ui/SurfacePrimitives.module.css";

export function SectionHeader({
  eyebrow,
  title,
  id,
  action,
  className,
  level = 2,
}: {
  eyebrow: string;
  title: string;
  id?: string;
  action?: ReactNode;
  className?: string;
  level?: 2 | 3;
}) {
  const Heading = level === 2 ? "h2" : "h3";
  return (
    <div className={`${styles.sectionHeader} ${className ?? ""}`}>
      <div>
        <span>{eyebrow}</span>
        <Heading id={id}>{title}</Heading>
      </div>
      {action}
    </div>
  );
}

export function AlertBanner({
  tone = "danger",
  icon,
  className,
  children,
  ...props
}: HTMLAttributes<HTMLDivElement> & {
  tone?: "danger" | "warning" | "success" | "neutral";
  icon?: ReactNode;
}) {
  return (
    <div
      {...props}
      className={`${styles.alert} ${styles[tone]} ${className ?? ""}`}
      role={props.role ?? (tone === "danger" ? "alert" : "status")}
    >
      {icon}
      <span>{children}</span>
    </div>
  );
}
