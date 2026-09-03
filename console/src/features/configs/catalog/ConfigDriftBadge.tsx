import type { ApplicationStatus } from "@/api/configs";
import { StatusBadge } from "@/shared/ui/StatusBadge";
import styles from "@/features/configs/ConfigPage.module.css";

export function ConfigDriftBadge({ status }: { status: ApplicationStatus }) {
  const driftLabel =
    status.drift === "comparison-error"
      ? "Comparison error"
      : status.drift === "source-missing"
        ? "Source missing"
        : status.drift[0].toUpperCase() + status.drift.slice(1);
  const routine = status.drift === "clean" || status.drift === "untracked";
  return (
    <StatusBadge
      className={styles.configDriftBadge}
      variant={routine ? "inline" : "badge"}
      wrapLabel={false}
      tone={
        status.drift === "clean" ? "good" : status.drift === "untracked" ? "neutral" : "warning"
      }
      title={status.detail ?? status.last_application?.applied_at}
    >
      {driftLabel}
    </StatusBadge>
  );
}
