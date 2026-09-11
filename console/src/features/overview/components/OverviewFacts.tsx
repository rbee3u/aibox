import type { ReactNode } from "react";
import type { Tone } from "@/features/overview/viewTypes";
import { StatusBadge } from "@/shared/ui/StatusBadge";
import styles from "@/features/overview/OverviewPage.module.css";

/**
 * One Overview status strip fact.
 *
 * The label is the caption and the status is the answer, so the status takes
 * the shared status treatment the rest of the Console reads — a dot plus a
 * tone — one ladder step above the label beside it.
 */
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
