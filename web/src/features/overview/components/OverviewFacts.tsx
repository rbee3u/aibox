import type { ReactNode } from "react";
import type { Tone } from "@/features/overview/viewTypes";
import { StatusBadge } from "@/shared/ui/StatusBadge";
import styles from "@/features/overview/OverviewPage.module.css";

export function RuntimeStatus({
  icon,
  label,
  value,
  tone,
}: {
  icon: ReactNode;
  label: string;
  value: string;
  tone: Tone;
}) {
  return (
    <div className={styles.statusItem}>
      <span className={styles.factLabel}>
        <span aria-hidden="true">{icon}</span>
        {label}
      </span>
      <StatusBadge tone={tone} variant="inline" size="sm">
        {value}
      </StatusBadge>
    </div>
  );
}
