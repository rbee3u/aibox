import { AlertTriangle, LoaderCircle } from "lucide-react";
import { RefreshButton } from "@/shared/ui/RefreshButton";
import { AlertBanner } from "@/shared/ui/SurfacePrimitives";
import styles from "@/shared/ui/ManagementFeedback.module.css";

export function PageError({ error, onRetry }: { error: string | null; onRetry?: () => void }) {
  if (!error) return null;
  return (
    <AlertBanner
      variant="page"
      tone="danger"
      icon={<AlertTriangle size={16} aria-hidden="true" />}
      action={
        onRetry ? (
          <RefreshButton type="button" label="Retry" onClick={onRetry}>
            Retry
          </RefreshButton>
        ) : undefined
      }
    >
      {error}
    </AlertBanner>
  );
}

export function Loading() {
  return (
    <div className={styles.loading}>
      <LoaderCircle className="spin" size={22} aria-label="Loading" />
    </div>
  );
}

export function MutationUnavailable({ operation }: { operation?: { state: string } | null }) {
  if (operation?.state !== "running") return null;
  return (
    <AlertBanner variant="page" tone="info">
      A Management Operation is active. Changes are temporarily unavailable; browsing and refresh
      remain available.
    </AlertBanner>
  );
}
