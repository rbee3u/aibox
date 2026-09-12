import type { ApplicationStatus } from "@/api/configs";
import { appliedConfigPresentation } from "@/features/configs/configCatalog";
import { StatusBadge } from "@/shared/ui/StatusBadge";
import styles from "@/features/configs/ConfigPage.module.css";

/** Marks the Last Application source: `Applied` when clean, the drift label otherwise. */
export function ConfigDriftBadge({
  status,
  className,
}: {
  status: ApplicationStatus;
  className?: string;
}) {
  const presentation = appliedConfigPresentation(status);
  return (
    <StatusBadge
      className={`${styles.configDriftBadge} ${className ?? ""}`}
      variant={presentation.variant}
      wrapLabel={false}
      tone={presentation.tone}
      title={status.detail}
    >
      {presentation.label}
    </StatusBadge>
  );
}
