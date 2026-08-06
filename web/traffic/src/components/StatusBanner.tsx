import { AlertCircle, CheckCircle2, Info, X } from "lucide-react";
import styles from "./StatusBanner.module.css";

interface StatusBannerProps {
  kind: "error" | "info" | "success";
  message: string;
  action?: { label: string; onClick: () => void };
  onDismiss?: () => void;
}

export function StatusBanner({ kind, message, action, onDismiss }: StatusBannerProps) {
  const Icon = kind === "error" ? AlertCircle : kind === "success" ? CheckCircle2 : Info;
  return (
    <div
      className={`${styles.banner} ${styles[kind]}`}
      role={kind === "error" ? "alert" : "status"}
    >
      <Icon size={16} aria-hidden="true" />
      <span>{message}</span>
      {action && (
        <button className={styles.action} type="button" onClick={action.onClick}>
          {action.label}
        </button>
      )}
      {onDismiss && (
        <button
          className={styles.dismiss}
          type="button"
          aria-label="Dismiss message"
          onClick={onDismiss}
        >
          <X size={15} aria-hidden="true" />
        </button>
      )}
    </div>
  );
}
