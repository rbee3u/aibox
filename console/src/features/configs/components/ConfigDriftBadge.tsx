import type { ApplicationStatus } from "@/api/configs";
import layout from "@/shared/ui/layout/catalog.module.css";
import styles from "@/features/configs/ConfigPage.module.css";

export function ConfigDriftBadge({ status }: { status: ApplicationStatus }) {
  const driftLabel =
    status.drift === "comparison-error"
      ? "Comparison error"
      : status.drift === "source-missing"
        ? "Source missing"
        : status.drift[0].toUpperCase() + status.drift.slice(1);
  return (
    <span
      className={`${styles.configDriftBadge} ${
        status.drift === "clean"
          ? layout.statusGood
          : status.drift === "untracked"
            ? layout.statusNeutral
            : layout.statusWarn
      }`}
      title={status.detail ?? status.last_application?.applied_at}
    >
      {driftLabel}
    </span>
  );
}
