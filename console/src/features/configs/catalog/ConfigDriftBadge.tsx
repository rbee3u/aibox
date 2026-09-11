import type { ApplicationStatus } from "@/api/configs";
import { driftCatalogLabel } from "@/features/configs/configCatalog";
import { StatusBadge } from "@/shared/ui/StatusBadge";
import styles from "@/features/configs/ConfigPage.module.css";

export function ConfigDriftBadge({ status }: { status: ApplicationStatus }) {
  const driftLabel = driftCatalogLabel(status.drift);
  const routine = status.drift === "clean" || status.drift === "untracked";
  return (
    <StatusBadge
      className={styles.configDriftBadge}
      variant={routine ? "inline" : "badge"}
      wrapLabel={false}
      tone={
        status.drift === "clean" ? "good" : status.drift === "untracked" ? "neutral" : "warning"
      }
      title={status.detail}
    >
      {driftLabel}
    </StatusBadge>
  );
}
