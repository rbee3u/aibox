import { AlertTriangle, LoaderCircle } from "lucide-react";
import type { Operation } from "../controlApi";
import styles from "./ManagementFeedback.module.css";

export function PageError({ error, onRetry }: { error: string | null; onRetry?: () => void }) {
  if (!error) return null;
  return (
    <div className={styles.errorBanner} role="alert">
      <AlertTriangle size={16} aria-hidden="true" />
      <span>{error}</span>
      {onRetry && (
        <button type="button" onClick={onRetry}>
          Retry
        </button>
      )}
    </div>
  );
}

export function Loading() {
  return (
    <div className={styles.loading}>
      <LoaderCircle className="spin" size={22} aria-label="Loading" />
    </div>
  );
}

export function MutationUnavailable({ operation }: { operation?: Operation | null }) {
  if (operation?.state !== "running") return null;
  return (
    <div className={styles.mutationUnavailable} role="status">
      A Management Operation is active. Changes are temporarily unavailable; browsing and refresh
      remain available.
    </div>
  );
}
